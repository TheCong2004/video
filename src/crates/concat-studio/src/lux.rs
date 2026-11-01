// SPDX-License-Identifier: AGPL-3.0-or-later
//! Adapter and runner for the `lux` video downloader CLI.
//!
//! Spawns `lux` as a child process (bundled in `resources/bin/lux` or discovered in PATH)
//! to query video/playlist metadata and download videos, parsing progress and errors.

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Stream / quality option for an online video.
pub struct LuxStream {
    /// Stream ID (e.g. "default", "1080p", "720p", "mp4").
    pub id: String,
    /// Human-readable quality label (e.g. "1080p", "1920x1080", "HD").
    pub quality: String,
    /// File extension after download (e.g. "mp4", "webm").
    pub ext: String,
    /// Total stream size in bytes, if reported.
    pub size_bytes: u64,
    /// Formatted stream size (e.g. "45.2 MB").
    pub size_display: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Parsed video metadata extracted from `lux`.
pub struct LuxVideoInfo {
    /// Source URL.
    pub url: String,
    /// Video source site/platform name (e.g. "bilibili", "youtube", "tiktok").
    pub site: String,
    /// Title of the video.
    pub title: String,
    /// Type of media ("video", "audio", "image").
    pub media_type: String,
    /// Available streams / quality levels sorted best to lowest.
    pub streams: Vec<LuxStream>,
    /// Public cover URL, never a video/media download URL.
    #[serde(default)]
    pub thumbnail: String,
    /// Duration when reported by the platform; zero means unknown.
    #[serde(default)]
    pub duration_seconds: u32,
}

#[derive(Debug, Clone, Default)]
/// Options for downloading a video via `lux`.
pub struct LuxDownloadOptions {
    /// Cooperative cancellation; kills only this download's process tree.
    pub cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Target video or playlist URL.
    pub url: String,
    /// Selected stream ID (e.g. "1080p", or default if None).
    pub stream_id: Option<String>,
    /// Download playlist if true (`-p`).
    pub playlist: bool,
    /// Destination folder (`-o`).
    pub output_dir: Option<PathBuf>,
    /// Download audio only (`-ao`).
    pub audio_only: bool,
}

#[derive(Debug, Clone)]
/// Real-time progress events from the download process.
pub enum LuxProgressEvent {
    /// Starting extraction or connection.
    Started {
        /// Message description.
        message: String,
    },
    /// Active download progress.
    Progress {
        /// Download completion percentage (0.0 .. 100.0).
        percent: f32,
        /// Current transfer speed (e.g. "2.45 MiB/s").
        speed: String,
        /// Estimated time remaining (e.g. "15s").
        eta: String,
        /// Raw output line.
        raw: String,
    },
    /// Fragment merging phase.
    Merging {
        /// Output file being merged.
        destination: String,
    },
    /// Successfully downloaded file.
    Completed {
        /// Full path to downloaded file.
        path: PathBuf,
    },
    /// Process failed with error description.
    Failed {
        /// Error message.
        error: String,
    },
}

/// Locates the `lux` binary bundled with Concat or in the system PATH.
pub fn find_lux_binary() -> Result<PathBuf, String> {
    if let Ok(custom) = std::env::var("CONCAT_LUX_PATH").or_else(|_| std::env::var("LUX_PATH")) {
        let p = PathBuf::from(custom);
        if p.is_file() {
            return Ok(p);
        }
    }

    let mut candidates = Vec::new();

    // 1. Check beside current executable: resources/bin/lux(.exe)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("resources").join("bin").join(lux_exe_name()));
            candidates.push(parent.join(lux_exe_name()));
            // When running from target/debug or target/release:
            if let Some(target_dir) = parent.parent() {
                if let Some(src_dir) = target_dir.parent() {
                    if let Some(concat_root) = src_dir.parent() {
                        candidates.push(
                            concat_root
                                .join("resources")
                                .join("bin")
                                .join(lux_exe_name()),
                        );
                        candidates.push(concat_root.join("lux").join(lux_exe_name()));
                    }
                }
            }
        }
    }

    // 2. Relative to current working directory
    candidates.push(PathBuf::from("resources").join("bin").join(lux_exe_name()));
    candidates.push(PathBuf::from(lux_exe_name()));

    // 3. Known project workspace paths on this machine
    candidates.push(PathBuf::from(r"d:\capcutpolot\concat\resources\bin").join(lux_exe_name()));
    candidates.push(PathBuf::from(r"d:\capcutpolot\lux").join(lux_exe_name()));

    for candidate in candidates {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    // 4. Try PATH lookup
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let p = dir.join(lux_exe_name());
            if p.is_file() {
                return Ok(p);
            }
        }
    }

    Err(format!(
        "Không tìm thấy binary '{}'. Vui lòng đặt binary vào thư mục resources/bin/ hoặc cài đặt vào PATH.",
        lux_exe_name()
    ))
}

fn lux_exe_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "lux.exe"
    }
    #[cfg(not(target_os = "windows"))]
    {
        "lux"
    }
}

/// Creates a `std::process::Command` configured with no window on Windows and FFmpeg in PATH.
pub fn make_command(binary: &Path) -> Command {
    let mut cmd = Command::new(binary);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    // Ensure ffmpeg is accessible to lux for merging separate video/audio streams
    let mut path_dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            path_dirs.extend([
                parent.to_owned(),
                parent.join("resources/node"),
                parent.join("OmniRoute"),
            ]);
        }
    }
    if let Some(parent) = binary.parent() {
        path_dirs.push(parent.to_path_buf());
    }
    if let Ok(ffmpeg_env) = std::env::var("FFMPEG_DIR") {
        let p = PathBuf::from(ffmpeg_env).join("bin");
        if p.exists() {
            path_dirs.push(p);
        }
    }
    let deps_ffmpeg = PathBuf::from(
        r"d:\capcutpolot\concat\.deps\ffmpeg\ffmpeg-n8.1.3-9-g29e619e767-win64-gpl-shared-8.1\bin",
    );
    if deps_ffmpeg.exists() {
        path_dirs.push(deps_ffmpeg);
    }
    let resources_bin = PathBuf::from(r"d:\capcutpolot\concat\resources\bin");
    if resources_bin.exists() {
        path_dirs.push(resources_bin);
    }
    let bundled_node = PathBuf::from(r"d:\capcutpolot\concat\.deps\omniroute");
    if bundled_node.join("node.exe").is_file() {
        path_dirs.push(bundled_node);
    }

    let existing_path = std::env::var("PATH").unwrap_or_default();
    let prefix = path_dirs
        .iter()
        .map(|d| d.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(";");
    if !prefix.is_empty() {
        let new_path = format!("{};{}", prefix, existing_path);
        cmd.env("PATH", new_path);
    }

    cmd
}

/// Strips ANSI color escape sequences from a string.
pub fn strip_ansi(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            output.push(ch);
        }
    }
    output
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    } else if bytes > 0 {
        format!("{} B", bytes)
    } else {
        "N/A".to_string()
    }
}

#[derive(Deserialize)]
struct RawLuxPart {
    #[serde(default)]
    ext: String,
    #[serde(default)]
    size: i64,
}

#[derive(Deserialize)]
struct RawLuxStream {
    #[serde(default)]
    id: String,
    #[serde(default)]
    quality: String,
    #[serde(default)]
    ext: String,
    #[serde(default)]
    size: i64,
    #[serde(default)]
    parts: Vec<RawLuxPart>,
}

#[derive(Deserialize)]
struct RawLuxData {
    #[serde(default)]
    url: String,
    #[serde(default)]
    site: String,
    #[serde(default)]
    title: String,
    #[serde(rename = "type", default)]
    media_type: String,
    #[serde(default)]
    streams: std::collections::HashMap<String, RawLuxStream>,
}

/// Helper to determine if a URL points to YouTube.
pub fn is_youtube_url(url: &str) -> bool {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .is_some_and(|host| {
            host == "youtu.be" || host == "youtube.com" || host.ends_with(".youtube.com")
        })
}

/// Smart URL classifier and normalizer:
/// Returns (normalized_url, is_account_or_playlist).
/// - Single video (Douyin modal_id, TikTok /video/, YouTube /watch): is_account = false
/// - Account / Channel (Douyin /user/, TikTok /@..., YouTube /@...): is_account = true
pub fn normalize_and_classify_url(input: &str) -> (String, bool) {
    let trimmed = input.trim();
    if let Ok(parsed) = url::Url::parse(trimmed) {
        let host = parsed.host_str().unwrap_or("").to_lowercase();
        let path = parsed.path();

        // 1. Douyin
        if host == "douyin.com" || host.ends_with(".douyin.com") {
            if let Some((_, modal_id)) = parsed.query_pairs().find(|(k, _)| k == "modal_id") {
                if !modal_id.is_empty() {
                    return (format!("https://www.douyin.com/video/{}", modal_id), false);
                }
            }
            if path.contains("/video/") || path.contains("/note/") {
                return (trimmed.to_string(), false);
            }
            if path.contains("/user/") {
                return (trimmed.to_string(), true);
            }
        }

        // 2. TikTok
        if host == "tiktok.com" || host.ends_with(".tiktok.com") {
            if path.contains("/video/") {
                return (trimmed.to_string(), false);
            }
            if path.trim_matches('/').starts_with('@') {
                return (trimmed.to_string(), true);
            }
        }

        // 3. YouTube
        if host == "youtube.com" || host.ends_with(".youtube.com") || host == "youtu.be" {
            if path.contains("/watch") || path.contains("/shorts/") || host == "youtu.be" {
                return (trimmed.to_string(), false);
            }
            if path.starts_with("/@") || path.contains("/channel/") || path.contains("/c/") {
                return (trimmed.to_string(), true);
            }
            if parsed.query_pairs().any(|(k, _)| k == "list") {
                return (trimmed.to_string(), true);
            }
        }

        // 4. Bilibili
        if host == "bilibili.com" || host.ends_with(".bilibili.com") {
            if path.contains("/video/") {
                return (trimmed.to_string(), false);
            }
            if path.contains("/space/") {
                return (trimmed.to_string(), true);
            }
        }
    }
    (trimmed.to_string(), false)
}

/// Validate video links before invoking an extractor (profile feeds are not videos).
pub fn validate_video_url(input: &str) -> Result<String, String> {
    validate_video_request(input, false)
}

/// A profile feed is accepted only when the user explicitly requests a playlist/channel.
pub fn validate_video_request(input: &str, playlist: bool) -> Result<String, String> {
    let (normalized, is_profile) = normalize_and_classify_url(input);
    let url = url::Url::parse(normalized.trim()).map_err(|_| "Nhập liên kết video https:// hợp lệ.")?;
    if !matches!(url.scheme(), "https" | "http")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Chỉ nhận liên kết video HTTP/HTTPS, không kèm mật khẩu.".into());
    }
    let host = url.host_str().unwrap_or_default();
    if !playlist && is_profile {
        if host.contains("douyin.com") {
            return Err("Đây là trang tài khoản Douyin. Bật 'Tải danh sách phát / kênh' để quét danh sách video.".into());
        }
        if host.contains("tiktok.com") {
            return Err("Đây là trang hồ sơ TikTok. Bật 'Tải danh sách phát / kênh' để tải cả hồ sơ, hoặc dán liên kết một video /@tên/video/…".into());
        }
    }
    Ok(url.into())
}

/// Concise actionable errors; stack traces stay out of the dialog.
pub fn friendly_error(error: &str) -> String {
    if error == "DOWNLOAD_CANCELLED" {
        return "Đã hủy tải. Không nhập tệp chưa hoàn tất.".into();
    }
    let lower = error.to_lowercase();
    if lower.contains("secondary user id") || lower.contains("private or has embedding disabled") {
        return "Không đọc được danh sách video công khai của hồ sơ TikTok này. TikTok có thể giới hạn truy cập hoặc hồ sơ không công khai. Thử liên kết một video cụ thể; chưa có video nào được xác nhận tải xong.".into();
    }
    if lower.contains("403") || lower.contains("forbidden") {
        return "Máy chủ từ chối tải (HTTP 403). Liên kết có thể hết hạn hoặc yêu cầu phiên truy cập; thử lấy lại thông tin. Không thể kết luận do bản quyền.".into();
    }
    if lower.contains("sign in") || lower.contains("login") || lower.contains("cookies") {
        return "Nền tảng yêu cầu xác minh hoặc phiên đăng nhập cho video này. Chưa tải được video.".into();
    }
    if lower.contains("url parse failed") || lower.contains("unable to extract") {
        return "Bộ tải chưa đọc được video này. Kiểm tra liên kết video cụ thể; nền tảng có thể đã thay đổi hoặc chặn truy cập.".into();
    }
    strip_ansi(error)
        .lines()
        .filter(|s| !s.contains("github.com/") && !s.contains(".go:") && !s.contains("runtime."))
        .take(3)
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .take(650)
        .collect()
}

pub(crate) fn ytdlp_binary() -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "yt-dlp.exe"
    } else {
        "yt-dlp"
    };
    if let Ok(path) = std::env::var("CONCAT_YTDLP_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let mut candidates = Vec::new();
    if let Ok(lux) = find_lux_binary() {
        if let Some(parent) = lux.parent() {
            candidates.push(parent.join(name));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("resources/bin").join(name));
        }
    }
    if let Some(paths) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&paths).map(|dir| dir.join(name)));
    }
    candidates.into_iter().find(|p| p.is_file())
}

fn prefer_ytdlp(url: &str) -> bool {
    crate::provider_registry::provider_for_url(url)
        .is_some_and(|provider| provider.backend == crate::provider_registry::DownloadBackend::YtDlp)
}

fn parse_ytdlp_info(text: &str, source: &str) -> Result<Vec<LuxVideoInfo>, String> {
    let raw: serde_json::Value =
        serde_json::from_str(text).map_err(|_| "Không đọc được thông tin video")?;
    let rows = raw
        .get("entries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_else(|| vec![raw]);
    let mut result = Vec::new();
    for row in rows.into_iter().filter(|row| !row.is_null()) {
        let mut streams = vec![LuxStream {
            id: "yt:auto".into(),
            quality: "Tự động · hình và âm thanh".into(),
            ext: "mp4/mkv".into(),
            size_bytes: 0,
            size_display: "Theo nguồn".into(),
        }];
        if let Some(formats) = row["formats"].as_array() {
            for f in formats.iter().rev() {
                if f["vcodec"].as_str().is_none_or(|s| s == "none") {
                    continue;
                }
                let Some(id) = f["format_id"].as_str() else {
                    continue;
                };
                let size = f["filesize"]
                    .as_u64()
                    .or_else(|| f["filesize_approx"].as_u64())
                    .unwrap_or(0);
                streams.push(LuxStream {
                    id: format!("yt:{id}"),
                    quality: format!(
                        "{}p · {}",
                        f["height"].as_u64().unwrap_or(0),
                        if f["acodec"].as_str().is_some_and(|s| s != "none") {
                            "có âm thanh"
                        } else {
                            "ghép thêm âm thanh"
                        }
                    ),
                    ext: f["ext"].as_str().unwrap_or("mp4").into(),
                    size_bytes: size,
                    size_display: format_bytes(size),
                });
            }
        }
        let item_url = row["webpage_url"]
            .as_str()
            .or_else(|| row["url"].as_str())
            .unwrap_or(source);
        let item_url = if !item_url.starts_with("http") && row["ie_key"].as_str() == Some("Youtube")
        {
            format!("https://www.youtube.com/watch?v={item_url}")
        } else {
            item_url.to_owned()
        };
        result.push(LuxVideoInfo {
            url: item_url,
            site: row["extractor"].as_str().unwrap_or("Trực tuyến").into(),
            title: row["title"].as_str().unwrap_or("Video").into(),
            media_type: "video".into(),
            streams,
            thumbnail: row["thumbnail"]
                .as_str()
                .or_else(|| {
                    row["thumbnails"]
                        .as_array()
                        .and_then(|v| v.last())
                        .and_then(|v| v["url"].as_str())
                })
                .unwrap_or("")
                .into(),
            duration_seconds: row["duration"]
                .as_f64()
                .filter(|s| s.is_finite() && *s > 0.0)
                .map(|s| s.ceil() as u32)
                .unwrap_or(0),
        });
    }
    if result.is_empty() {
        return Err("Không tìm thấy video trong liên kết".into());
    }
    Ok(result)
}

/// Bounded channel/playlist enumeration; never starts media downloads.
pub fn list_account_videos(
    url: &str,
    limit: usize,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<Vec<LuxVideoInfo>, String> {
    let source = validate_video_request(url, true)?;
    let binary = ytdlp_binary().ok_or("Thiếu yt-dlp để lấy danh sách tài khoản.")?;
    let mut cmd = make_command(&binary);
    cmd.args([
        "--ignore-config",
        "--no-warnings",
        "--socket-timeout",
        "15",
        "--retries",
        "1",
        "--js-runtimes",
        "node",
        "--flat-playlist",
        "--yes-playlist",
        "--skip-download",
        "--ignore-errors",
        "-J",
        "--playlist-end",
    ])
    .arg(limit.clamp(1, 500).to_string())
    .arg("--")
    .arg(&source);
    let output = capture_metadata(cmd, cancel)?;
    let mut videos = parse_ytdlp_info(&String::from_utf8_lossy(&output.stdout), &source)
        .map_err(|_| friendly_error(&String::from_utf8_lossy(&output.stderr)))?;
    let mut seen = std::collections::HashSet::new();
    videos.retain(|v| {
        v.url != source && crate::video_library::source_key(&v.url).is_some_and(|k| seen.insert(k))
    });
    videos.truncate(limit.clamp(1, 500));
    if videos.is_empty() {
        return Err("Chưa đọc được video công khai. Với tài khoản TikTok, dùng nút 'Lấy qua trình duyệt' và hoàn tất xác minh nếu có.".into());
    }
    Ok(videos)
}

/// Queries metadata for a video or playlist URL.
///
/// Attempts `-j` (JSON) extraction first for maximum fidelity, and falls back to
/// parsing `-i` (text output) if needed.
pub fn probe_video_info(url: &str, playlist: bool) -> Result<Vec<LuxVideoInfo>, String> {
    probe_video_info_cancellable(
        url,
        playlist,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    )
}

/// Metadata query with cancellation and a total time limit.
pub fn probe_video_info_cancellable(
    url: &str,
    playlist: bool,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<Vec<LuxVideoInfo>, String> {
    let validated = validate_video_request(url, playlist)?;
    let url = validated.as_str();
    if prefer_ytdlp(url) {
        if let Some(binary) = ytdlp_binary() {
            let mut cmd = make_command(&binary);
            cmd.args([
                "--ignore-config",
                "--no-warnings",
                "--socket-timeout",
                "20",
                "--retries",
                "1",
                "--js-runtimes",
                "node",
                "-J",
                "--skip-download",
            ]);
            cmd.arg(if playlist {
                "--yes-playlist"
            } else {
                "--no-playlist"
            });
            if playlist {
                cmd.arg("--flat-playlist");
            }
            cmd.arg("--").arg(url);
            let output = capture_metadata(cmd, cancel.clone())?;
            if !output.status.success() {
                return Err(friendly_error(&String::from_utf8_lossy(&output.stderr)));
            }
            return parse_ytdlp_info(&String::from_utf8_lossy(&output.stdout), url);
        }
    }
    let binary = find_lux_binary()?;

    // 1. Try with -j flag
    let mut cmd = make_command(&binary);
    cmd.arg("-j");
    if playlist {
        cmd.arg("-p");
    }
    cmd.arg(url);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let output = capture_metadata(cmd, cancel.clone())?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    if output.status.success() && !stdout.trim().is_empty() {
        if let Ok(raw_list) = serde_json::from_str::<Vec<RawLuxData>>(&stdout) {
            let mut result = Vec::new();
            for raw in raw_list {
                let mut streams = Vec::new();
                for (id, stream) in raw.streams {
                    let size_bytes = if stream.size > 0 {
                        stream.size as u64
                    } else {
                        stream.parts.iter().map(|p| p.size.max(0) as u64).sum()
                    };
                    let ext = if !stream.ext.is_empty() {
                        stream.ext.clone()
                    } else if let Some(first_part) = stream.parts.first() {
                        first_part.ext.clone()
                    } else {
                        "mp4".to_string()
                    };

                    let quality = if is_youtube_url(url) && (id == "18" || stream.id == "18") {
                        "360p (MP4 - Tương thích cao / Ổn định)".to_string()
                    } else if is_youtube_url(url) && (id == "22" || stream.id == "22") {
                        "720p (MP4 - Tương thích cao / Ổn định)".to_string()
                    } else if !stream.quality.is_empty() {
                        stream.quality.clone()
                    } else {
                        id.clone()
                    };

                    streams.push(LuxStream {
                        id: if !stream.id.is_empty() { stream.id } else { id },
                        quality,
                        ext,
                        size_bytes,
                        size_display: format_bytes(size_bytes),
                    });
                }
                // Sort streams by size descending
                streams.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));

                result.push(LuxVideoInfo {
                    url: if !raw.url.is_empty() {
                        raw.url
                    } else {
                        url.to_string()
                    },
                    site: raw.site,
                    title: raw.title,
                    media_type: if !raw.media_type.is_empty() {
                        raw.media_type
                    } else {
                        "video".to_string()
                    },
                    streams,
                    thumbnail: String::new(),
                    duration_seconds: 0,
                });
            }
            if !result.is_empty() {
                return Ok(result);
            }
        }
    }

    // 2. Fallback to -i text output parsing
    let mut cmd_i = make_command(&binary);
    cmd_i.arg("-i");
    if playlist {
        cmd_i.arg("-p");
    }
    cmd_i.arg(url);
    cmd_i.stdout(Stdio::piped()).stderr(Stdio::piped());

    let output_i = capture_metadata(cmd_i, cancel)?;
    let stdout_i = String::from_utf8_lossy(&output_i.stdout);
    let stderr_i = String::from_utf8_lossy(&output_i.stderr);

    if !stdout_i.trim().is_empty() {
        let parsed = parse_info_text(&stdout_i, url);
        if !parsed.is_empty() {
            return Ok(parsed);
        }
    }

    let err_msg = if !stderr_i.trim().is_empty() {
        stderr_i.trim().to_string()
    } else if !stdout_i.trim().is_empty() {
        stdout_i.trim().to_string()
    } else {
        "Không thể trích xuất thông tin video từ liên kết này.".to_string()
    };
    Err(friendly_error(&err_msg))
}

pub(crate) fn capture_metadata(
    mut cmd: Command,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<std::process::Output, String> {
    use std::sync::atomic::Ordering;
    if cancel.load(Ordering::Relaxed) {
        return Err("DOWNLOAD_CANCELLED".into());
    }
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    #[cfg(windows)]
    let tree = match own_process_tree(&child) {
        Ok(tree) => tree,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e.into());
        }
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let out = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(pipe) = stdout {
            let _ = pipe.take(64 * 1024 * 1024).read_to_end(&mut bytes);
        }
        bytes
    });
    let err = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(pipe) = stderr {
            let _ = pipe.take(1024 * 1024).read_to_end(&mut bytes);
        }
        bytes
    });
    let started = std::time::Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) || started.elapsed() > std::time::Duration::from_secs(120)
        {
            #[cfg(windows)]
            drop(tree);
            let _ = child.kill();
            let _ = child.wait();
            let _ = out.join();
            let _ = err.join();
            return Err(if cancel.load(Ordering::Relaxed) {
                "DOWNLOAD_CANCELLED"
            } else {
                "Quá thời gian lấy thông tin video. Hãy thử lại."
            }
            .into());
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Ok(std::process::Output {
                status,
                stdout: out.join().unwrap_or_default(),
                stderr: err.join().unwrap_or_default(),
            });
        }
        std::thread::sleep(std::time::Duration::from_millis(80));
    }
}

/// Read a public thumbnail from an allowlisted platform CDN. No credentials,
/// redirects, browser session export, or arbitrary local/private host access.
pub fn fetch_thumbnail(source: &str) -> Result<Vec<u8>, String> {
    let url = url::Url::parse(source).map_err(|_| "Invalid cover URL")?;
    let host = url.host_str().ok_or("Invalid cover host")?;
    let allowed = [
        "tiktokcdn.com",
        "tiktokcdn-us.com",
        "tiktokcdn-eu.com",
        "tiktokv.com",
        "tiktok.com",
        "byteoversea.com",
        "ibytedtos.com",
        "ttwstatic.com",
        "ytimg.com",
        "googleusercontent.com",
        "hdslb.com",
        "bilivideo.com",
    ];
    if url.scheme() != "https"
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || !allowed
            .iter()
            .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
    {
        return Err("Cover host is not allowed".into());
    }
    let response = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .get(source)
        .call()
        .map_err(|_| "Cover not available")?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cover read failed")?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("Cover exceeds size limit".into());
    }
    Ok(bytes)
}

/// Metadata adapter: parses human-readable text output from `lux -i`.
pub fn parse_info_text(raw_text: &str, default_url: &str) -> Vec<LuxVideoInfo> {
    let clean = strip_ansi(raw_text);
    let mut videos = Vec::new();

    let mut current_site = String::new();
    let mut current_title = String::new();
    let mut current_type = "video".to_string();
    let mut current_streams = Vec::new();

    let mut current_stream_id = String::new();
    let mut current_stream_quality = String::new();
    let mut current_stream_size = 0u64;

    let is_yt = is_youtube_url(default_url);
    let flush_stream =
        |streams: &mut Vec<LuxStream>, id: &mut String, quality: &mut String, size: &mut u64| {
            if !id.is_empty() {
                let q = if is_yt && id == "18" {
                    "360p (MP4 - Tương thích cao / Ổn định)".to_string()
                } else if is_yt && id == "22" {
                    "720p (MP4 - Tương thích cao / Ổn định)".to_string()
                } else if quality.is_empty() {
                    id.clone()
                } else {
                    quality.clone()
                };
                streams.push(LuxStream {
                    id: id.clone(),
                    quality: q,
                    ext: "mp4".to_string(),
                    size_bytes: *size,
                    size_display: format_bytes(*size),
                });
                id.clear();
                quality.clear();
                *size = 0;
            }
        };

    let flush_video = |videos: &mut Vec<LuxVideoInfo>,
                       site: &mut String,
                       title: &mut String,
                       media_type: &mut String,
                       streams: &mut Vec<LuxStream>| {
        if !title.is_empty() || !streams.is_empty() {
            streams.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
            videos.push(LuxVideoInfo {
                url: default_url.to_string(),
                site: site.clone(),
                title: if title.is_empty() {
                    "Video".to_string()
                } else {
                    title.clone()
                },
                media_type: media_type.clone(),
                streams: std::mem::take(streams),
                thumbnail: String::new(),
                duration_seconds: 0,
            });
            site.clear();
            title.clear();
            *media_type = "video".to_string();
        }
    };

    for line in clean.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Site:") {
            if !current_title.is_empty() {
                flush_stream(
                    &mut current_streams,
                    &mut current_stream_id,
                    &mut current_stream_quality,
                    &mut current_stream_size,
                );
                flush_video(
                    &mut videos,
                    &mut current_site,
                    &mut current_title,
                    &mut current_type,
                    &mut current_streams,
                );
            }
            current_site = trimmed.trim_start_matches("Site:").trim().to_string();
        } else if trimmed.starts_with("Title:") {
            current_title = trimmed.trim_start_matches("Title:").trim().to_string();
        } else if trimmed.starts_with("Type:") {
            current_type = trimmed.trim_start_matches("Type:").trim().to_string();
        } else if trimmed.starts_with('[') && trimmed.contains(']') && trimmed.contains("---") {
            flush_stream(
                &mut current_streams,
                &mut current_stream_id,
                &mut current_stream_quality,
                &mut current_stream_size,
            );
            if let Some(end_bracket) = trimmed.find(']') {
                current_stream_id = trimmed[1..end_bracket].trim().to_string();
            }
        } else if trimmed.starts_with("Quality:") {
            current_stream_quality = trimmed.trim_start_matches("Quality:").trim().to_string();
        } else if trimmed.starts_with("Size:") {
            let size_text = trimmed.trim_start_matches("Size:").trim();
            // Example: "45.20 MiB (47395840 Bytes)"
            if let Some(open_p) = size_text.find('(') {
                if let Some(close_p) = size_text.find("Bytes)") {
                    let num_str = size_text[open_p + 1..close_p].trim();
                    if let Ok(bytes) = num_str.parse::<u64>() {
                        current_stream_size = bytes;
                    }
                }
            }
        }
    }

    flush_stream(
        &mut current_streams,
        &mut current_stream_id,
        &mut current_stream_quality,
        &mut current_stream_size,
    );
    flush_video(
        &mut videos,
        &mut current_site,
        &mut current_title,
        &mut current_type,
        &mut current_streams,
    );

    videos
}

/// Downloads a video using `lux` and reports live progress.
pub fn download_video<F>(
    options: &LuxDownloadOptions,
    on_progress: F,
) -> Result<Vec<PathBuf>, String>
where
    F: Fn(LuxProgressEvent) + Send + Sync + 'static,
{
    let source = validate_video_request(&options.url, options.playlist)?;
    let cancel = options.cancel.clone();
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("DOWNLOAD_CANCELLED".into());
    }
    let supplement = if prefer_ytdlp(&source)
        || options
            .stream_id
            .as_deref()
            .is_some_and(|s| s.starts_with("yt:"))
    {
        ytdlp_binary()
    } else {
        None
    };
    let binary = supplement.clone().map(Ok).unwrap_or_else(find_lux_binary)?;

    let output_dir = if let Some(dir) = &options.output_dir {
        dir.clone()
    } else {
        std::env::temp_dir().join("concat-lux-downloads")
    };
    std::fs::create_dir_all(&output_dir)
        .map_err(|e| format!("Không thể tạo thư mục lưu tải về: {e}"))?;
    let parent = output_dir.canonicalize().map_err(|e| e.to_string())?;
    let output_dir = parent.join(format!("download-{:032x}", rand::random::<u128>()));
    std::fs::create_dir(&output_dir).map_err(|e| e.to_string())?;
    let mut attempt = DownloadAttempt {
        path: output_dir.clone(),
        parent,
        keep: false,
    };

    on_progress(LuxProgressEvent::Started {
        message: "Đang kết nối nguồn video…".into(),
    });

    let mut cmd = make_command(&binary);
    if supplement.is_some() {
        cmd.args([
            "--ignore-config",
            "--newline",
            "--no-colors",
            "--socket-timeout",
            "20",
            "--retries",
            "2",
            "--js-runtimes",
            "node",
            "--windows-filenames",
            "--merge-output-format",
            "mp4/mkv",
        ]);
        cmd.arg(if options.playlist {
            "--yes-playlist"
        } else {
            "--no-playlist"
        });
        cmd.arg("-P")
            .arg(&output_dir)
            .args(["-o", "%(title).120B [%(id)s].%(ext)s"]);
        let selected = options
            .stream_id
            .as_deref()
            .and_then(|s| s.strip_prefix("yt:"))
            .unwrap_or("auto");
        let format = if selected == "auto" {
            "bv*+ba/b".into()
        } else {
            format!("{selected}+ba/{selected}")
        };
        cmd.arg("-f").arg(format).arg("--").arg(&source);
    } else {
        cmd.arg("-o").arg(&output_dir);
        if options.playlist {
            cmd.arg("-p");
        }
        if let Some(stream_id) = &options.stream_id {
            if !stream_id.is_empty() {
                cmd.arg("-f").arg(stream_id);
            }
        }
        if options.audio_only {
            cmd.arg("-ao");
        }
        cmd.arg("--retry").arg("5");
        cmd.arg(&source);
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Không thể khởi chạy tiến trình lux: {e}"))?;

    #[cfg(windows)]
    let tree = match own_process_tree(&child) {
        Ok(tree) => tree,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error.into());
        }
    };

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let on_progress_arc = std::sync::Arc::new(on_progress);
    let p_clone = on_progress_arc.clone();

    // Unified error log lines capturing from both stdout and stderr
    let error_lines = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

    // Spawn thread to read stdout
    let p_out = p_clone.clone();
    let err_out = error_lines.clone();
    let stdout_handle = std::thread::spawn(move || {
        if let Some(mut out) = stdout {
            let mut buf = [0u8; 1024];
            let mut line_buf = Vec::new();
            while let Ok(n) = out.read(&mut buf) {
                if n == 0 {
                    break;
                }
                for &b in &buf[..n] {
                    if b == b'\r' || b == b'\n' {
                        if !line_buf.is_empty() {
                            if let Ok(text) = String::from_utf8(std::mem::take(&mut line_buf)) {
                                let clean = strip_ansi(&text);
                                let trimmed = clean.trim();
                                if !trimmed.is_empty() {
                                    let is_prog = parse_progress_line(trimmed, &p_out);
                                    if !is_prog {
                                        if let Ok(mut l) = err_out.lock() {
                                            l.push(trimmed.to_string());
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        line_buf.push(b);
                    }
                }
            }
            if !line_buf.is_empty() {
                if let Ok(text) = String::from_utf8(line_buf) {
                    let clean = strip_ansi(&text);
                    let trimmed = clean.trim();
                    if !trimmed.is_empty() {
                        let is_prog = parse_progress_line(trimmed, &p_out);
                        if !is_prog {
                            if let Ok(mut l) = err_out.lock() {
                                l.push(trimmed.to_string());
                            }
                        }
                    }
                }
            }
        }
    });

    // Spawn thread to read stderr (lux outputs progress or errors here)
    let p_err = p_clone.clone();
    let err_err = error_lines.clone();
    let stderr_handle = std::thread::spawn(move || {
        if let Some(mut err) = stderr {
            let mut buf = [0u8; 1024];
            let mut line_buf = Vec::new();
            while let Ok(n) = err.read(&mut buf) {
                if n == 0 {
                    break;
                }
                for &b in &buf[..n] {
                    if b == b'\r' || b == b'\n' {
                        if !line_buf.is_empty() {
                            if let Ok(text) = String::from_utf8(std::mem::take(&mut line_buf)) {
                                let clean = strip_ansi(&text);
                                let trimmed = clean.trim();
                                if !trimmed.is_empty() {
                                    let is_prog = parse_progress_line(trimmed, &p_err);
                                    if !is_prog {
                                        if let Ok(mut l) = err_err.lock() {
                                            l.push(trimmed.to_string());
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        line_buf.push(b);
                    }
                }
            }
            if !line_buf.is_empty() {
                if let Ok(text) = String::from_utf8(line_buf) {
                    let clean = strip_ansi(&text);
                    let trimmed = clean.trim();
                    if !trimmed.is_empty() {
                        let is_prog = parse_progress_line(trimmed, &p_err);
                        if !is_prog {
                            if let Ok(mut l) = err_err.lock() {
                                l.push(trimmed.to_string());
                            }
                        }
                    }
                }
            }
        }
    });

    let status = loop {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            #[cfg(windows)]
            drop(tree);
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_handle.join();
            let _ = stderr_handle.join();
            return Err("DOWNLOAD_CANCELLED".into());
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        std::thread::sleep(std::time::Duration::from_millis(80));
    };
    let _ = stdout_handle.join();
    let _ = stderr_handle.join();
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("DOWNLOAD_CANCELLED".into());
    }

    if !status.success() {
        let err_text = error_lines
            .lock()
            .ok()
            .map(|l| {
                let filtered: Vec<&str> = l
                    .iter()
                    .map(|s| s.as_str())
                    .filter(|s| !s.contains("MiB / ") && !s.contains("GiB / ") && !s.contains("%"))
                    .collect();
                if filtered.is_empty() {
                    l.join("\n")
                } else {
                    filtered.join("\n")
                }
            })
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| {
                "Tiến trình tải kết thúc với lỗi. Vui lòng kiểm tra lại liên kết hoặc kết nối mạng."
                    .to_string()
            });

        on_progress_arc(LuxProgressEvent::Failed {
            error: friendly_error(&err_text),
        });
        return Err(friendly_error(&err_text));
    }

    // Identify newly downloaded files
    let mut downloaded = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&output_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                // Ignore temporary parts or partial files
                let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if matches!(
                    ext.to_lowercase().as_str(),
                    "mp4" | "webm" | "mkv" | "mov" | "avi" | "m4v" | "flv" | "mp3" | "m4a"
                ) && !name.contains(".part")
                    && !name.contains(".temp.")
                    && !name.contains("[0].")
                    && !name.contains("[1].")
                {
                    downloaded.push(path);
                }
            }
        }
    }

    // Sort by modification time newest first
    downloaded.sort_by_key(|p| {
        p.metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });
    downloaded.reverse();

    if downloaded.is_empty() {
        return Err(
            "Bộ tải kết thúc nhưng không có video hoàn chỉnh. Không nhập tệp tạm vào dự án.".into(),
        );
    }
    if let Some(first) = downloaded.first() {
        on_progress_arc(LuxProgressEvent::Completed {
            path: first.clone(),
        });
    }

    attempt.keep = true;
    Ok(downloaded)
}

// Roll back only the freshly-created directory belonging to this exact attempt.
// It is never shared with another download, project, or pre-existing user output.
struct DownloadAttempt {
    path: PathBuf,
    parent: PathBuf,
    keep: bool,
}
impl Drop for DownloadAttempt {
    fn drop(&mut self) {
        if self.keep {
            return;
        }
        let Some(name) = self.path.file_name().and_then(|n| n.to_str()) else {
            return;
        };
        if name.len() != 41
            || !name.starts_with("download-")
            || !name[9..].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return;
        }
        if self
            .path
            .canonicalize()
            .ok()
            .as_deref()
            .and_then(Path::parent)
            == Some(self.parent.as_path())
        {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

#[cfg(windows)]
/// Own a CLI process tree so closing this handle terminates its descendants.
pub fn own_process_tree(
    child: &std::process::Child,
) -> Result<std::os::windows::io::OwnedHandle, &'static str> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::System::JobObjects::*;
    // SAFETY: newly owned non-inheritable job; valid child handle and initialized limits.
    let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if raw.is_null() {
        return Err("Không quản lý được phiên tải để hủy an toàn.");
    }
    let job = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let ok = unsafe {
        SetInformationJobObject(
            raw,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const _,
            std::mem::size_of_val(&limits) as u32,
        ) != 0
            && AssignProcessToJobObject(raw, child.as_raw_handle()) != 0
    };
    if !ok {
        return Err("Không quản lý được phiên tải để hủy an toàn.");
    }
    Ok(job)
}

/// Parses progress lines from `lux` stdout or stderr.
/// Returns true if the line was recognized as progress/status information.
fn parse_progress_line(
    line: &str,
    on_progress: &std::sync::Arc<impl Fn(LuxProgressEvent)>,
) -> bool {
    let clean = strip_ansi(line);
    let trimmed = clean.trim();

    if trimmed.is_empty() {
        return false;
    }

    // Check for merging
    if trimmed.contains("[Merger]") || trimmed.contains("Merging video parts into") {
        let dest = trimmed
            .split("Merging video parts into")
            .nth(1)
            .unwrap_or("")
            .trim()
            .to_string();
        on_progress(LuxProgressEvent::Merging { destination: dest });
        return true;
    }

    // Check for progress percentage (e.g. " 12.34 MiB / 45.67 MiB [=====>---] 1.23 MiB/s 27.02% 27s" or "? p/s 17.73%")
    if let Some(pct_idx) = trimmed.find('%') {
        let before_pct = &trimmed[..pct_idx];
        if let Some(start_pct) = before_pct.rfind(' ') {
            let pct_str = &before_pct[start_pct + 1..];
            if let Ok(percent) = pct_str.parse::<f32>() {
                // Extract speed
                let mut speed = String::new();
                for marker in &["p/s", "MiB/s", "KiB/s", "GiB/s", "MB/s", "KB/s"] {
                    if let Some(pos) = trimmed.find(marker) {
                        let before_marker = trimmed[..pos].trim_end();
                        if let Some(s_start) = before_marker.rfind(' ') {
                            let num = &before_marker[s_start + 1..];
                            if *marker == "p/s" {
                                if let Some(prev_start) = before_marker[..s_start].rfind(' ') {
                                    speed = format!(
                                        "{} {}/s",
                                        num,
                                        &before_marker[prev_start + 1..s_start]
                                    );
                                } else {
                                    speed = format!("{num} /s");
                                }
                            } else {
                                speed = format!("{num} {marker}");
                            }
                            break;
                        }
                    }
                }

                // Extract eta (last token ending in 's' or 'm')
                let eta = trimmed
                    .split_whitespace()
                    .last()
                    .filter(|s| s.ends_with('s') || s.ends_with('m'))
                    .unwrap_or("")
                    .to_string();

                on_progress(LuxProgressEvent::Progress {
                    percent,
                    speed,
                    eta,
                    raw: trimmed.to_string(),
                });
                return true;
            }
        }
    }

    if trimmed.contains("MiB / ") || trimmed.contains("GiB / ") || trimmed.contains("KiB / ") {
        return true;
    }
    if trimmed.starts_with("Site:")
        || trimmed.starts_with("Title:")
        || trimmed.starts_with("Type:")
        || trimmed.starts_with("Stream:")
        || trimmed.starts_with('[')
    {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn covers_reject_arbitrary_hosts_credentials_and_non_https() {
        for url in [
            "http://i.ytimg.com/a.jpg",
            "https://i.ytimg.com.evil.test/a",
            "https://127.0.0.1/a",
            "https://user:pass@i.ytimg.com/a",
            "file:///C:/secret",
            "https://i.ytimg.com:8443/a",
        ] {
            assert!(fetch_thumbnail(url).is_err());
        }
        let rows = parse_ytdlp_info(r#"{"title":"Cover","webpage_url":"https://youtu.be/demo","thumbnail":"https://i.ytimg.com/vi/demo/hqdefault.jpg","duration":11.9}"#, "https://youtu.be/demo").unwrap();
        assert_eq!(rows[0].duration_seconds, 12);
        assert!(rows[0].thumbnail.contains("hqdefault"));
    }
    #[test]
    fn flat_account_entries_keep_individual_urls_not_the_profile() {
        let rows = parse_ytdlp_info(r#"{"entries":[{"id":"abcdefghijk","url":"https://www.youtube.com/watch?v=abcdefghijk","title":"First"},{"id":"1234567890123456789","url":"https://www.tiktok.com/@demo/video/1234567890123456789","title":"Second"},{"url":"zyxwvutsrqp","ie_key":"Youtube","title":"Third"}]}"#, "https://www.tiktok.com/@demo").unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].url, "https://www.youtube.com/watch?v=abcdefghijk");
        assert!(rows[1].url.ends_with("/video/1234567890123456789"));
        assert_eq!(rows[2].url, "https://www.youtube.com/watch?v=zyxwvutsrqp");
    }

    #[test]
    fn video_links_reject_profiles_and_credentials() {
        assert!(
            validate_video_url("https://www.tiktok.com/@oclon2026")
                .unwrap_err()
                .contains("hồ sơ")
        );
        assert!(validate_video_url("https://www.tiktok.com/@demo/video/123456789").is_ok());
        assert!(validate_video_url("https://vm.tiktok.com/example/").is_ok());
        assert!(validate_video_url("file:///C:/secret").is_err());
        assert!(validate_video_url("https://user:pass@example.com/a").is_err());
        assert!(!is_youtube_url("https://youtube.com.attacker.example/a"));
    }

    #[test]
    fn error_text_is_bounded_and_does_not_invent_copyright() {
        let text = friendly_error("HTTP 403 Forbidden");
        assert!(text.contains("403"));
        assert!(!text.contains("bảo vệ bản quyền"));
        assert!(
            !friendly_error("url parse failed\ngithub.com/a\nD:/a.go:39").contains("github.com")
        );
        assert!(friendly_error(&"x".repeat(10000)).chars().count() <= 650);
    }

    #[test]
    fn supplemented_metadata_keeps_auto_and_audio_only_is_not_video() {
        let videos = parse_ytdlp_info(r#"{"title":"Demo","extractor":"youtube","formats":[{"format_id":"a","vcodec":"none","acodec":"aac"},{"format_id":"v","vcodec":"h264","acodec":"none","height":720,"ext":"mp4"}]}"#, "https://youtu.be/demo").unwrap();
        assert_eq!(videos[0].streams.len(), 2);
        assert_eq!(videos[0].streams[0].id, "yt:auto");
        assert_eq!(videos[0].streams[1].id, "yt:v");
    }

    #[test]
    #[cfg(windows)]
    fn cancellation_stops_metadata_child_without_waiting_for_sleep() {
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let trigger = cancel.clone();
        let mut cmd = make_command(Path::new("powershell.exe"));
        cmd.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 30",
        ]);
        let started = std::time::Instant::now();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(250));
            trigger.store(true, std::sync::atomic::Ordering::Relaxed);
        });
        assert_eq!(
            capture_metadata(cmd, cancel).unwrap_err(),
            "DOWNLOAD_CANCELLED"
        );
        thread.join().unwrap();
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    #[test]
    fn test_parse_info_text_adapter() {
        let sample = r#"
 Site:      Bilibili bilibili.com
 Title:     Hướng dẫn dựng phim Concat
 Type:      video
 Streams:   # All available quality
     [1080P]  -------------------
     Quality:         1080P 60FPS
     Size:            125.50 MiB (131596288 Bytes)
     # download with: lux -f 1080P ...

     [720P]  -------------------
     Quality:         720P HD
     Size:            62.25 MiB (65273856 Bytes)
     # download with: lux -f 720P ...
"#;

        let videos = parse_info_text(sample, "https://bilibili.com/video/BV123");
        assert_eq!(videos.len(), 1);
        let v = &videos[0];
        assert_eq!(v.title, "Hướng dẫn dựng phim Concat");
        assert_eq!(v.site, "Bilibili bilibili.com");
        assert_eq!(v.media_type, "video");
        assert_eq!(v.streams.len(), 2);

        assert_eq!(v.streams[0].id, "1080P");
        assert_eq!(v.streams[0].quality, "1080P 60FPS");
        assert_eq!(v.streams[0].size_bytes, 131596288);

        assert_eq!(v.streams[1].id, "720P");
        assert_eq!(v.streams[1].quality, "720P HD");
        assert_eq!(v.streams[1].size_bytes, 65273856);
    }

    #[test]
    fn test_parse_progress_line() {
        let line = " 12.34 MiB / 45.67 MiB [=====>-------------] 1.23 MiB/s 27.02% 27s";
        let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
        let cap_clone = captured.clone();
        parse_progress_line(
            line,
            &std::sync::Arc::new(move |evt| {
                if let LuxProgressEvent::Progress {
                    percent,
                    speed,
                    eta,
                    ..
                } = evt
                {
                    *cap_clone.lock().unwrap() = Some((percent, speed, eta));
                }
            }),
        );

        let res = captured.lock().unwrap().take();
        assert!(res.is_some());
        let (pct, spd, eta) = res.unwrap();
        assert!((pct - 27.02).abs() < 0.01);
        assert_eq!(spd, "1.23 MiB/s");
        assert_eq!(eta, "27s");
    }

    #[test]
    fn test_find_lux_binary() {
        let binary = find_lux_binary();
        assert!(
            binary.is_ok(),
            "find_lux_binary should locate lux in project or resources: {:?}",
            binary
        );
    }
}
