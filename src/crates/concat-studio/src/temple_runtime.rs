// SPDX-License-Identifier: AGPL-3.0-or-later
//! Launch the bundled local service only when absent. It survives closing the
//! UI, keeping OBS independent. Startup never connects a TikTok account.
use crate::temple_live::Endpoint;
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn healthy(endpoint: &Endpoint) -> bool {
    ureq::get(&format!("{}/status", endpoint.origin))
        .timeout(Duration::from_secs(1))
        .call()
        .ok()
        .and_then(|r| serde_json::from_reader::<_, serde_json::Value>(r.into_reader()).ok())
        .is_some_and(|v| {
            v["running"] == true && v["settings"].is_object() && v["capacity"].is_number()
        })
}

/// Reuses a running server. Returns true only when starting the bundled one.
pub fn ensure(endpoint: &Endpoint) -> Result<bool, &'static str> {
    if healthy(endpoint) {
        return Ok(false);
    }
    let url = url::Url::parse(&endpoint.origin).map_err(|_| "Địa chỉ LIVE không hợp lệ.")?;
    if url.scheme() != "http"
        || !matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
    {
        return Err("Máy chủ LIVE được cấu hình chưa sẵn sàng.");
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let executable = std::env::current_exe().map_err(|_| "Không xác định được ứng dụng.")?;
    let parent = executable.parent().ok_or("Không xác định được ứng dụng.")?;
    let server = if let Some(value) = std::env::var_os("CONCAT_TEMPLE_SERVER") {
        let file = PathBuf::from(value);
        if !file.is_file() {
            return Err("Không tìm thấy runtime LIVE đã cấu hình.");
        }
        file
    } else {
        [
            parent.join("resources/temple-live/live/server.mjs"),
            root.join("integrations/temple-live/live/server.mjs"),
        ]
        .into_iter()
        .find(|p| p.is_file())
        .ok_or("Chưa có runtime Chánh Điện trong gói Concat.")?
    };
    let server = server
        .canonicalize()
        .map_err(|_| "Không mở được runtime Chánh Điện.")?;
    // Node's entry-point resolver cannot load Windows verbatim drive paths
    // returned by canonicalize (\\?\D:\...). Keep its normal absolute spelling.
    #[cfg(windows)]
    let server = {
        let value = server.to_string_lossy();
        PathBuf::from(if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{rest}")
        } else {
            value.strip_prefix(r"\\?\").unwrap_or(&value).to_string()
        })
    };
    if !server
        .parent()
        .is_some_and(|p| p.join("node_modules/ws").is_dir())
    {
        return Err("Runtime Chánh Điện thiếu dependencies; cần stage runtime.");
    }
    let node = std::env::var_os("CONCAT_TEMPLE_NODE")
        .map(PathBuf::from)
        .or_else(|| {
            [
                parent.join("resources/temple-live/node/node.exe"),
                parent.join("resources/node/node.exe"),
                root.join(".deps/temple-live-node/node.exe"),
            ]
            .into_iter()
            .find(|p| p.is_file())
        })
        .unwrap_or_else(|| PathBuf::from("node"));
    let port = url
        .port_or_known_default()
        .ok_or("Cổng LIVE không hợp lệ.")?;
    let data = std::env::var_os("CONCAT_TEMPLE_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            if root.join("src/Cargo.toml").is_file() {
                root.join(".runtime/temple-live").join(port.to_string())
            } else {
                std::env::var_os("LOCALAPPDATA")
                    .map(PathBuf::from)
                    .unwrap_or_else(std::env::temp_dir)
                    .join("app.concat.editor/temple-live")
                    .join(port.to_string())
            }
        });
    std::fs::create_dir_all(&data).map_err(|_| "Không mở được thư mục dữ liệu LIVE.")?;
    let stdout = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(data.join("server.stdout.log"))
        .map_err(|_| "Không tạo được nhật ký LIVE.")?;
    let stderr = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(data.join("server.stderr.log"))
        .map_err(|_| "Không tạo được nhật ký LIVE.")?;
    let mut command = Command::new(node);
    command
        .arg(&server)
        .current_dir(server.parent().unwrap())
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .env_clear();
    for name in [
        "SystemRoot",
        "WINDIR",
        "PATH",
        "TEMP",
        "TMP",
        "USERPROFILE",
        "LOCALAPPDATA",
        "APPDATA",
        "PROGRAMDATA",
        "HTTPS_PROXY",
        "HTTP_PROXY",
        "NO_PROXY",
        "NODE_EXTRA_CA_CERTS",
        "SSL_CERT_FILE",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    if let Some(value) = std::env::var_os("CONCAT_TEMPLE_HANDOVER") {
        command.env("CONCAT_TEMPLE_HANDOVER", value);
    }
    command
        .env("PORT", port.to_string())
        .env(
            "HOST",
            if url.host_str() == Some("[::1]") {
                "::1"
            } else {
                "127.0.0.1"
            },
        )
        .env("CONCAT_TEMPLE_DATA_DIR", &data);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|_| "Không khởi động được Node runtime Chánh Điện.")?;
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if healthy(endpoint) {
            let _ = std::fs::write(data.join("server.pid"), child.id().to_string());
            return Ok(true);
        }
        if child.try_wait().ok().flatten().is_some() {
            return Err("Runtime LIVE chưa khởi động được; xem nhật ký runtime.");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err("Máy chủ LIVE đang khởi động chậm; kết nối sẽ tiếp tục thử lại.")
}
