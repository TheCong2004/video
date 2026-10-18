// SPDX-License-Identifier: AGPL-3.0-or-later
//! Owned browser adapter process; no changes to global browser/AI configuration.
use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum Event {
    Connected,
    Result {
        applied: bool,
        ok: bool,
        count: usize,
    },
    Error {
        message: String,
    },
}

pub(super) struct WebChat {
    child: Option<Child>,
    events: Receiver<Event>,
    pub provider: String,
}

impl WebChat {
    pub fn start(config: &str, profile: &Path, provider: &str) -> Result<Self, String> {
        if !matches!(provider, "chatgpt" | "gemini" | "claude") {
            return Err("AI web không được hỗ trợ.".into());
        }
        let config: serde_json::Value =
            serde_json::from_str(config).map_err(|_| "Không đọc được runtime chat Concat.")?;
        let server = &config["mcpServers"]["concat"];
        let node = server["command"].as_str().ok_or("Thiếu Node runtime.")?;
        let script = Path::new(server["args"][0].as_str().ok_or("Thiếu chat adapter.")?)
            .with_file_name("web-chat.mjs");
        let connection = server["args"][1].as_str().ok_or("Thiếu kết nối project.")?;
        if !script.is_file() {
            return Err("Bộ cài chưa có browser adapter. Cần cập nhật Concat.".into());
        }
        let mut command = Command::new(node);
        command
            .arg(script)
            .arg(connection)
            .arg(profile)
            .arg(provider)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().map_err(|_| "Không mở được chat adapter.")?;
        let stdout = child
            .stdout
            .take()
            .ok_or("Không đọc được trạng thái chat.")?;
        let (sender, events) = mpsc::sync_channel(16);
        let worker = std::thread::Builder::new()
            .name("ai-web-chat".into())
            .spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    let Ok(line) = line else { break };
                    if line.len() > 8192 {
                        break;
                    }
                    if let Ok(event) = serde_json::from_str::<Event>(&line) {
                        if sender.send(event).is_err() {
                            break;
                        }
                    }
                }
            });
        if worker.is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Không theo dõi được chat adapter.".into());
        }
        Ok(Self {
            child: Some(child),
            events,
            provider: provider.into(),
        })
    }
    pub fn next_event(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }
    pub fn finished(&mut self) -> bool {
        self.child
            .as_mut()
            .is_none_or(|child| child.try_wait().ok().flatten().is_some())
    }
    pub fn focus(&mut self) {
        if let Some(stdin) = self.child.as_mut().and_then(|child| child.stdin.as_mut()) {
            let _ = writeln!(stdin, "focus");
        }
    }
}

impl Drop for WebChat {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = writeln!(stdin, "stop");
        }
        // The adapter closes only its own dedicated browser. Never wait on the UI.
        let _ = std::thread::Builder::new()
            .name("ai-web-chat-stop".into())
            .spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(20);
                loop {
                    if child.try_wait().ok().flatten().is_some() {
                        return;
                    }
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            });
    }
}
