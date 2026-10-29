// SPDX-License-Identifier: AGPL-3.0-or-later
//! Read public TikTok profile links in a visible, isolated browser. No cookie export.
use crate::lux::{LuxStream, LuxVideoInfo};
use serde_json::{Value, json};
use std::{
    io::Read,
    net::TcpStream,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

fn account(source: &str) -> Result<(String, String), String> {
    let url = url::Url::parse(source).map_err(|_| "Link tài khoản không hợp lệ.")?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || !matches!(url.host_str(), Some("www.tiktok.com" | "tiktok.com"))
    {
        return Err("Cách lấy qua trình duyệt chỉ nhận trang tài khoản TikTok HTTPS.".into());
    }
    let handle = url
        .path()
        .trim_matches('/')
        .strip_prefix('@')
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 64
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._".contains(&b))
        })
        .ok_or("Dán link tài khoản TikTok dạng https://www.tiktok.com/@tên.")?;
    Ok((
        format!("https://www.tiktok.com/@{handle}"),
        handle.to_ascii_lowercase(),
    ))
}

fn browser_binary() -> Result<PathBuf, String> {
    for var in ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"] {
        if let Some(root) = std::env::var_os(var) {
            for rel in [
                "Microsoft/Edge/Application/msedge.exe",
                "Google/Chrome/Application/chrome.exe",
            ] {
                let path = PathBuf::from(&root).join(rel);
                if path.is_file() {
                    return Ok(path);
                }
            }
        }
    }
    Err("Cần Microsoft Edge hoặc Chrome để đọc tài khoản bị chặn truy cập trực tiếp.".into())
}

fn local_json(port: u16, method: &str, path: &str) -> Result<Value, String> {
    let response = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(Duration::from_secs(3))
        .build()
        .request(method, &format!("http://127.0.0.1:{port}{path}"))
        .call()
        .map_err(|_| "Không kết nối được trình duyệt đọc danh sách.")?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Không đọc được trình duyệt.")?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("Danh sách trình duyệt quá lớn.".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "Trình duyệt trả dữ liệu không hợp lệ.".into())
}

fn port_file(profile: &Path) -> Option<u16> {
    let bytes = std::fs::read(profile.join("DevToolsActivePort")).ok()?;
    if bytes.len() > 1024 {
        return None;
    }
    String::from_utf8(bytes)
        .ok()?
        .lines()
        .next()?
        .parse::<u16>()
        .ok()
        .filter(|p| *p > 0)
}

fn owned_port(profile: &Path) -> Option<u16> {
    let port = port_file(profile)?;
    let marker = std::fs::read_to_string(profile.join("DevToolsActivePort")).ok()?;
    let expected = marker.lines().nth(1)?;
    let version = local_json(port, "GET", "/json/version").ok()?;
    let endpoint = url::Url::parse(version["webSocketDebuggerUrl"].as_str()?).ok()?;
    (endpoint.scheme() == "ws"
        && endpoint.host_str() == Some("127.0.0.1")
        && endpoint.port() == Some(port)
        && endpoint.path() == expected
        && expected.starts_with("/devtools/browser/"))
    .then_some(port)
}

/// Open a dedicated profile, wait for visible access verification and scroll only
/// the requested account. The caller explicitly invokes this fallback.
pub fn list_tiktok(
    source: &str,
    limit: usize,
    profile: &Path,
    cancel: Arc<AtomicBool>,
) -> Result<Vec<LuxVideoInfo>, String> {
    let (target, handle) = account(source)?;
    if cancel.load(Ordering::Relaxed) {
        return Err("DOWNLOAD_CANCELLED".into());
    }
    std::fs::create_dir_all(profile).map_err(|_| "Không tạo được hồ sơ trình duyệt tải video.")?;
    let mut browser = None;
    if owned_port(profile).is_none() {
        let mut cmd = crate::lux::make_command(&browser_binary()?);
        cmd.arg(format!("--user-data-dir={}", profile.display()))
            .args([
                "--remote-debugging-address=127.0.0.1",
                "--remote-debugging-port=0",
                "--no-first-run",
                "--no-default-browser-check",
            ])
            .arg(&target)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        browser = Some(
            cmd.spawn()
                .map_err(|_| "Không mở được trình duyệt đọc tài khoản.")?,
        );
    }
    let start = Instant::now();
    let port = loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("DOWNLOAD_CANCELLED".into());
        }
        if let Some(port) = owned_port(profile) {
            break port;
        }
        if start.elapsed() > Duration::from_secs(20) {
            return Err("Trình duyệt chưa sẵn sàng. Đóng cửa sổ đọc danh sách rồi thử lại.".into());
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    // Reuse the same account tab in Concat's dedicated profile; never navigate
    // unrelated tabs or create another copy every time the user rescans.
    // The DevTools endpoint takes the literal URL as its query, not a form field.
    // `account` has reduced it to HTTPS + an ASCII handle (no query/fragment).
    let pages = local_json(port, "GET", "/json/list")?;
    let page = if let Some(page) = pages.as_array().and_then(|rows| {
        rows.iter().find(|page| {
            page["type"] == "page"
                && page["url"]
                    .as_str()
                    .and_then(|s| account(s).ok())
                    .is_some_and(|(_, h)| h == handle)
        })
    }) {
        page.clone()
    } else {
        local_json(port, "PUT", &format!("/json/new?{target}"))?
    };
    let ws = page["webSocketDebuggerUrl"]
        .as_str()
        .ok_or("Thiếu kết nối trang tài khoản.")?;
    let ws_url = url::Url::parse(ws).map_err(|_| "Địa chỉ trình duyệt không hợp lệ.")?;
    if ws_url.scheme() != "ws"
        || ws_url.host_str() != Some("127.0.0.1")
        || ws_url.port() != Some(port)
    {
        return Err("Từ chối kết nối trình duyệt ngoài máy.".into());
    }
    let stream = TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}")
            .parse()
            .map_err(|_| "Cổng trình duyệt không hợp lệ.")?,
        Duration::from_secs(3),
    )
    .map_err(|_| "Không kết nối được trang tài khoản.")?;
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .map_err(|e| e.to_string())?;
    let config = tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(2 * 1024 * 1024))
        .max_frame_size(Some(2 * 1024 * 1024));
    let (mut socket, _) = tungstenite::client::client_with_config(ws, stream, Some(config))
        .map_err(|_| "Không mở được kết nối trang tài khoản.")?;
    socket
        .send(tungstenite::Message::Text(
            json!({"id":9999,"method":"Page.bringToFront"})
                .to_string()
                .into(),
        ))
        .map_err(|_| "Không hiển thị được trang tài khoản.")?;
    let limit = limit.clamp(1, 500);
    let mut videos = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut idle = 0;
    let began = Instant::now();
    for id in 1..=180u64 {
        if cancel.load(Ordering::Relaxed) {
            return Err("DOWNLOAD_CANCELLED".into());
        }
        // Constant script: no source interpolation, no cookies/storage/network API calls.
        let expression = r#"JSON.stringify({url:location.href,challenge:(document.body?.innerText || '').includes('Drag the slider to fit the puzzle'), rows:Array.from(document.querySelectorAll('a[href*="/video/"]')).map(a=>({url:a.href,title:(a.querySelector('img')?.alt || a.innerText || '').slice(0,300),thumbnail:a.querySelector('img')?.currentSrc || a.querySelector('img')?.src || '',duration_seconds:Math.ceil(a.querySelector('video')?.duration || 0)}))})"#;
        socket.send(tungstenite::Message::Text(json!({"id":id,"method":"Runtime.evaluate","params":{"expression":expression,"returnByValue":true}}).to_string().into())).map_err(|_| "Trang đọc danh sách đã đóng.")?;
        let pending = Instant::now();
        let value = loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("DOWNLOAD_CANCELLED".into());
            }
            if pending.elapsed() > Duration::from_secs(5) {
                return Err("Trang tài khoản không phản hồi. Thử lại sau khi xác minh.".into());
            }
            match socket.read() {
                Ok(tungstenite::Message::Text(text)) => {
                    let msg: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                    if msg["id"].as_u64() == Some(id) {
                        break msg["result"]["result"]["value"]
                            .as_str()
                            .and_then(|s| serde_json::from_str::<Value>(s).ok())
                            .unwrap_or(Value::Null);
                    }
                }
                Err(tungstenite::Error::Io(e))
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) =>
                {
                    continue;
                }
                Ok(_) => continue,
                _ => return Err("Trang đọc danh sách đã đóng.".into()),
            }
        };
        let previous = videos.len();
        let same_account = value["url"]
            .as_str()
            .and_then(|s| account(s).ok())
            .is_some_and(|(_, h)| h == handle);
        if same_account {
            if value["challenge"] == true
                && videos.is_empty()
                && began.elapsed() > Duration::from_secs(8)
            {
                return Err("TikTok đang yêu cầu xác minh. Kéo thanh xác minh trong cửa sổ TikTok vừa mở, rồi bấm Lấy danh sách lại. Không cần đăng nhập Concat.".into());
            }
            if let Some(rows) = value["rows"].as_array() {
                for row in rows.iter().take(2000) {
                    if videos.len() == limit {
                        break;
                    }
                    if let Some(video) = parse_row(row, &handle) {
                        if seen.insert(crate::video_library::source_key(&video.url)) {
                            videos.push(video);
                        }
                    }
                }
            }
            if videos.len() >= limit {
                break;
            }
            if previous == videos.len() {
                idle += 1;
            } else {
                idle = 0;
            }
            if !videos.is_empty() && idle >= 12 {
                break;
            }
            if !videos.is_empty() {
                let expected = serde_json::to_string(value["url"].as_str().unwrap_or(""))
                    .map_err(|_| "Trang không hợp lệ.")?;
                let scroll = format!(
                    "if(location.href === {expected}) window.scrollTo(0, document.documentElement.scrollHeight)"
                );
                socket.send(tungstenite::Message::Text(json!({"method":"Runtime.evaluate","id":1000+id,"params":{"expression":scroll}}).to_string().into())).map_err(|_| "Trang tài khoản đã đóng.")?;
            }
        }
        if began.elapsed() > Duration::from_secs(180) {
            break;
        }
        for _ in 0..10 {
            if cancel.load(Ordering::Relaxed) {
                return Err("DOWNLOAD_CANCELLED".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    // Do not close/kill the visible browser: the user may be completing verification.
    // Reap the launcher if it exited; retain the one dedicated profile for future scans.
    if let Some(mut child) = browser {
        let _ = child.try_wait();
    }
    if videos.is_empty() {
        return Err("Chưa đọc được video. Hoàn tất xác minh trong trình duyệt vừa mở rồi bấm Lấy danh sách lại. Không vượt quyền xem tài khoản riêng tư.".into());
    }
    Ok(videos)
}

fn parse_row(row: &Value, handle: &str) -> Option<LuxVideoInfo> {
    let url = url::Url::parse(row["url"].as_str()?).ok()?;
    if url.scheme() != "https" || !matches!(url.host_str(), Some("www.tiktok.com" | "tiktok.com")) {
        return None;
    }
    let parts: Vec<_> = url.path_segments()?.collect();
    if parts.len() != 3
        || parts[0].to_ascii_lowercase() != format!("@{handle}")
        || parts[1] != "video"
        || parts[2].is_empty()
        || !parts[2].bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    Some(LuxVideoInfo {
        url: format!("https://www.tiktok.com/@{handle}/video/{}", parts[2]),
        title: row["title"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(parts[2])
            .chars()
            .take(300)
            .collect(),
        site: "TikTok".into(),
        media_type: "video".into(),
        thumbnail: row["thumbnail"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(8192)
            .collect(),
        duration_seconds: row["duration_seconds"]
            .as_u64()
            .filter(|s| *s <= 86400)
            .unwrap_or(0) as u32,
        streams: vec![LuxStream {
            id: "yt:auto".into(),
            quality: "Tự động".into(),
            ext: "mp4".into(),
            size_bytes: 0,
            size_display: "Theo nguồn".into(),
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browser_only_reads_exact_tiktok_account() {
        assert!(account("https://www.tiktok.com/@abc").is_ok());
        for url in [
            "https://tiktok.com.evil.test/@abc",
            "file:///abc",
            "https://u:p@tiktok.com/@abc",
            "https://tiktok.com/@abc/video/123",
        ] {
            assert!(account(url).is_err());
        }
        assert!(
            parse_row(
                &json!({"url":"https://www.tiktok.com/@abc/video/123", "title":"Demo"}),
                "abc"
            )
            .is_some()
        );
        for url in [
            "https://www.tiktok.com/@other/video/123",
            "https://tiktok.com.evil.test/@abc/video/123",
            "https://www.tiktok.com/@abc/video/not-id",
        ] {
            assert!(parse_row(&json!({"url":url}), "abc").is_none());
        }
    }
}
