// SPDX-License-Identifier: AGPL-3.0-or-later
//! Opt-in loopback handoff for the live window. Never executes editor commands.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

const ENTROPY: &str = "concat-ai-edit-mcp-v1";
/// Bound inbound tool request, including the local transport credential.
pub const MAX_REQUEST: usize = 96 * 1024;

/// Parse a user-pasted/file draft, accepting only JSON or a single JSON fence.
/// Native schema/scope validation and human approval still happen in the editor.
pub fn parse_external_draft(text: &str) -> Result<(String, Value), &'static str> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Draft {
        scope: String,
        plan: Value,
    }
    if text.len() > MAX_REQUEST {
        return Err("Bản nháp vượt giới hạn 96 KiB.");
    }
    let text = text.trim();
    let candidate = if let Some(start_idx) = text.find("```") {
        let after_start = &text[start_idx + 3..];
        let body = if let Some((_header, rest)) = after_start.split_once('\n') {
            rest
        } else {
            after_start
        };
        if let Some(end_idx) = body.find("```") {
            &body[..end_idx]
        } else {
            body
        }
    } else if let Some(first_brace) = text.find('{') {
        if let Some(last_brace) = text.rfind('}') {
            if last_brace > first_brace {
                &text[first_brace..=last_brace]
            } else {
                text
            }
        } else {
            text
        }
    } else {
        text
    }
    .trim();

    let draft: Draft = serde_json::from_str(candidate)
        .or_else(|_| serde_json::from_str(text))
        .map_err(|_| "Cần JSON {scope,plan}; hãy kiểm tra lại câu trả lời từ AI.")?;
    if draft.scope.is_empty() || draft.scope.len() > 128 || !draft.plan.is_object() {
        return Err("Scope hoặc plan không hợp lệ. Hãy lấy context mới.");
    }
    Ok((draft.scope, draft.plan))
}

/// Only these operations can enter the window; approval is deliberately absent.
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum EditRequest {
    /// Redacted current timeline plus the exact native edit schema.
    Context {},
    /// Current draft status, never source paths or credentials.
    Status {},
    /// Untrusted proposal for human inspection, not an approved edit.
    Propose {
        /// Window epoch and editor revision from the context tool.
        scope: String,
        /// Strict native EditPlan, never arbitrary commands or executable code.
        plan: Value,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    token: String,
    request: EditRequest,
}

#[derive(Serialize)]
struct Connection {
    version: u32,
    host: &'static str,
    port: u16,
    token: String,
}

/// Pending call. A timed-out caller must not replace the visible draft later.
pub struct Pending {
    /// Validated wire operation, still requiring live Session validation.
    pub request: EditRequest,
    reply: mpsc::SyncSender<Value>,
    live: Arc<AtomicBool>,
    deadline: Instant,
}
impl Pending {
    /// Whether the caller is still waiting within the bounded handoff window.
    pub fn active(&self) -> bool {
        self.live.load(Ordering::Acquire) && Instant::now() < self.deadline
    }
    /// Return sanitized structured results. Nothing is automatically retried.
    pub fn respond(self, result: Value) {
        let _ = self.reply.try_send(result);
    }
}

/// Owns one private listener and its encrypted per-run connection file.
pub struct EditBridge {
    receiver: mpsc::Receiver<Pending>,
    stop: Arc<AtomicBool>,
    connection_file: PathBuf,
    worker: Option<thread::JoinHandle<()>>,
}
impl EditBridge {
    /// Start explicitly from a worker thread; never opens a public interface.
    pub fn start(directory: &Path) -> Result<Self, &'static str> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|_| "Không mở được cổng local")?;
        listener
            .set_nonblocking(true)
            .map_err(|_| "Không cấu hình được cổng local")?;
        let connection = Connection {
            version: 1,
            host: "127.0.0.1",
            port: listener
                .local_addr()
                .map_err(|_| "Không đọc được cổng")?
                .port(),
            token: random_secret(),
        };
        fs::create_dir_all(directory).map_err(|_| "Không tạo được thư mục MCP")?;
        let bytes = serde_json::to_vec(&connection).map_err(|_| "Không tạo được cấu hình MCP")?;
        let encrypted =
            crate::auth::protect(&bytes, ENTROPY).map_err(|_| "Không mã hóa được kết nối MCP")?;
        let path = directory.join(format!("{}.dpapi", random_secret()));
        fs::write(&path, encrypted).map_err(|_| "Không lưu được kết nối MCP")?;
        let (sender, receiver) = mpsc::sync_channel(4);
        let stop = Arc::new(AtomicBool::new(false));
        let signal = stop.clone();
        let token = connection.token;
        let worker = match thread::Builder::new()
            .name("ai-edit-bridge".into())
            .spawn(move || {
                while !signal.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((stream, _)) => serve(stream, &token, &sender, &signal),
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(20))
                        }
                        Err(_) => break,
                    }
                }
            }) {
            Ok(worker) => worker,
            Err(_) => {
                let _ = fs::remove_file(&path);
                return Err("Không khởi động được MCP bridge");
            }
        };
        Ok(Self {
            receiver,
            stop,
            connection_file: path,
            worker: Some(worker),
        })
    }
    /// Non-blocking poll on the UI event loop; no filesystem/network I/O.
    pub fn try_next(&self) -> Option<Pending> {
        self.receiver.try_recv().ok()
    }
    /// A local file path, never the transport secret. Bound to this Windows user.
    pub fn connection_file(&self) -> &Path {
        &self.connection_file
    }
}
impl Drop for EditBridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Do not block the UI on a socket. Thread and file cleanup are bounded.
        if let Some(worker) = self.worker.take() {
            let path = self.connection_file.clone();
            let _ = thread::Builder::new()
                .name("ai-edit-stop".into())
                .spawn(move || {
                    let _ = worker.join();
                    let _ = fs::remove_file(path);
                });
        }
    }
}
fn random_secret() -> String {
    rand::random::<[u8; 32]>()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn authorized(actual: &str, expected: &str) -> bool {
    actual.len() == expected.len()
        && actual
            .bytes()
            .zip(expected.bytes())
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            == 0
}
fn serve(
    mut stream: TcpStream,
    token: &str,
    sender: &mpsc::SyncSender<Pending>,
    stop: &AtomicBool,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let result = (|| -> Option<Value> {
        let mut bytes = Vec::new();
        let mut reader = BufReader::new((&stream).take(MAX_REQUEST as u64 + 1));
        reader.read_until(b'\n', &mut bytes).ok()?;
        if bytes.len() > MAX_REQUEST || bytes.last() != Some(&b'\n') { return None; }
        let wire: Wire = serde_json::from_slice(&bytes).ok()?;
        if !authorized(&wire.token, token) || stop.load(Ordering::Acquire) { return None; }
        let (tx, rx) = mpsc::sync_channel(1);
        let live = Arc::new(AtomicBool::new(true));
        let pending = Pending { request: wire.request, reply: tx, live: live.clone(), deadline: Instant::now() + Duration::from_secs(8) };
        sender.try_send(pending).ok()?;
        let started = Instant::now();
        let result = loop {
            if stop.load(Ordering::Acquire) { break None; }
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(result) => break Some(result),
                Err(mpsc::RecvTimeoutError::Timeout) if live.load(Ordering::Acquire) => {
                    // The UI marks active() false after its deadline. Keep the
                    // transport bounded even when the event loop is paused.
                    if started.elapsed() >= Duration::from_secs(8) { break None; }
                }
                _ => break None,
            }
        };
        live.store(false, Ordering::Release);
        result
    })().unwrap_or_else(|| json!({"ok":false,"error":"Bridge unavailable, invalid request or timeout; no edit was applied. Do not replay automatically."}));
    if let Ok(bytes) = serde_json::to_vec(&result) {
        let _ = stream.write_all(&bytes);
        let _ = stream.write_all(b"\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pasted_draft_accepts_json_or_one_fence_but_never_extra_commands() {
        let raw = r#"{"scope":"1:0","plan":{"version":1}}"#;
        assert_eq!(parse_external_draft(raw).unwrap().0, "1:0");
        assert!(parse_external_draft(&format!("```json\n{raw}\n``` ")).is_ok());
        assert!(parse_external_draft(&format!("```\n{raw}\n```")).is_ok());
        for text in [
            "not json",
            r#"{"scope":"","plan":{}}"#,
            r#"{"scope":"1:0","plan":{},"approve":true}"#,
            r#"{"scope":"1:0","plan":"run shell"}"#,
        ] {
            assert!(parse_external_draft(text).is_err());
        }
        assert!(parse_external_draft(&format!("Explanation\n```json\n{raw}\n```")).is_ok());
        assert!(parse_external_draft(&"x".repeat(MAX_REQUEST + 1)).is_err());
    }
    #[test]
    fn authority_never_includes_approval_or_arbitrary_commands() {
        for request in [
            json!({"action":"approve"}),
            json!({"action":"execute","command":"delete"}),
            json!({"action":"context","path":"secret"}),
        ] {
            assert!(serde_json::from_value::<EditRequest>(request).is_err());
        }
        assert!(serde_json::from_value::<EditRequest>(json!({"action":"context"})).is_ok());
        assert!(!authorized("wrong", "private"));
        assert!(!authorized("privatf", "private"));
        assert!(authorized("private", "private"));
    }

    #[cfg(windows)]
    #[test]
    fn encrypted_connection_authentication_and_real_queue_handoff() {
        let root = std::env::temp_dir().join(format!("concat-bridge-test-{}", random_secret()));
        let bridge = EditBridge::start(&root).unwrap();
        let encrypted = fs::read(bridge.connection_file()).unwrap();
        let plain = crate::auth::unprotect(&encrypted, ENTROPY).unwrap();
        let connection: Value = serde_json::from_slice(&plain).unwrap();
        let token = connection["token"].as_str().unwrap().to_string();
        assert!(
            !encrypted
                .windows(token.len())
                .any(|w| w == token.as_bytes())
        );
        let address = format!("127.0.0.1:{}", connection["port"].as_u64().unwrap());
        let request = |token: &str, action: Value| {
            let mut socket = TcpStream::connect(&address).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            writeln!(socket, "{}", json!({"token":token,"request":action})).unwrap();
            let mut output = String::new();
            BufReader::new(socket).read_line(&mut output).unwrap();
            serde_json::from_str::<Value>(&output).unwrap()
        };
        assert_eq!(request("wrong", json!({"action":"context"}))["ok"], false);
        assert!(bridge.try_next().is_none());
        assert_eq!(request(&token, json!({"action":"approve"}))["ok"], false);
        assert!(bridge.try_next().is_none());
        let child = thread::spawn(move || {
            let mut socket = TcpStream::connect(address).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            writeln!(
                socket,
                "{}",
                json!({"token":token,"request":{"action":"context"}})
            )
            .unwrap();
            let mut output = String::new();
            BufReader::new(socket).read_line(&mut output).unwrap();
            serde_json::from_str::<Value>(&output).unwrap()
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        let pending = loop {
            if let Some(call) = bridge.try_next() {
                break call;
            }
            assert!(Instant::now() < deadline);
            thread::yield_now();
        };
        assert!(pending.active());
        assert!(matches!(pending.request, EditRequest::Context {}));
        pending.respond(json!({"ok":true,"fixture":true}));
        assert_eq!(child.join().unwrap()["fixture"], true);
        drop(bridge);
        let _ = fs::remove_dir_all(root);
    }
}
