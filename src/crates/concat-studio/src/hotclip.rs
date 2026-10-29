// SPDX-License-Identifier: AGPL-3.0-or-later
//! Own real local Hotclip; private nonce-authenticated loopback control, no HTTP API.
use std::{
    io::{BufReader, Read, Write},
    net::{Shutdown, TcpListener},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

/// Only session-verified native window metadata crosses the private pipe.
#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    /// Native browser window is loaded; PID is the owned Electron main process.
    Ready {
        /// Native top-level HWND before Concat reparents it.
        hwnd: usize,
        /// Owned Electron main-process PID.
        pid: u32,
    },
    /// Startup/runtime failure category; never arbitrary process output.
    Failed(&'static str),
}

/// A single owned lifetime. Startup and process polling never run on the UI thread.
pub struct Process {
    stop: mpsc::Sender<Control>,
    events: mpsc::Receiver<Event>,
    worker: Option<thread::JoinHandle<()>>,
}
enum Control {
    Shutdown,
    Visible(bool),
}
impl Process {
    /// Discover the adjacent Hotclip checkout/runtime and start it hidden.
    pub fn start(data: PathBuf) -> Self {
        let (stop, shutdown) = mpsc::channel();
        let (output, events) = mpsc::channel();
        let worker = thread::spawn(move || {
            let result = run(data, shutdown, &output);
            if let Err(error) = result {
                let _ = output.send(Event::Failed(error));
            }
        });
        Self {
            stop,
            events,
            worker: Some(worker),
        }
    }
    /// Nonblocking UI polling.
    pub fn poll(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }
    /// Notify Electron after native attachment; no socket I/O on the UI thread.
    pub fn set_visible(&self, visible: bool) {
        let _ = self.stop.send(Control::Visible(visible));
    }
    /// Graceful quit then bounded owned-tree fallback; only after host UI exits.
    pub fn shutdown(mut self) {
        let _ = self.stop.send(Control::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.stop.send(Control::Shutdown);
    }
}

fn runtime_at(root: &Path) -> Option<(PathBuf, PathBuf)> {
    let package = std::fs::read(root.join("package.json")).ok()?;
    if package.len() > 64 * 1024
        || serde_json::from_slice::<serde_json::Value>(&package).ok()?["name"] != "hotclip"
    {
        return None;
    }
    let electron = root.join("node_modules/electron/dist/electron.exe");
    let main = root.join("out/main/index.js");
    if !electron.is_file()
        || !main.is_file()
        || !root.join("out/preload/index.js").is_file()
        || !root.join("out/renderer/index.html").is_file()
    {
        return None;
    }
    Some((electron, main))
}
fn discover() -> Option<(PathBuf, PathBuf, PathBuf)> {
    if let Some(root) = std::env::var_os("CONCAT_HOTCLIP_ROOT") {
        let root = PathBuf::from(root);
        if !root.is_absolute() {
            return None;
        }
        let (exe, main) = runtime_at(&root)?;
        return Some((root, exe, main));
    }
    let exe = std::env::current_exe().ok()?;
    for parent in exe.ancestors().skip(1).take(8) {
        let root = parent.join("hotclip");
        if let Some((exe, main)) = runtime_at(&root) {
            return Some((root, exe, main));
        }
    }
    None
}
fn ready(line: &str, nonce: &str, pid: u32) -> Option<Event> {
    if line.len() > 4096 {
        return None;
    }
    let value: serde_json::Value =
        serde_json::from_str(line.strip_prefix("CONCAT_HOTCLIP ")?).ok()?;
    if value["nonce"] != nonce
        || value["event"] != "ready"
        || value["pid"].as_u64()? != u64::from(pid)
    {
        return None;
    }
    let hwnd = value["hwnd"].as_str()?.parse::<usize>().ok()?;
    (hwnd != 0).then_some(Event::Ready { hwnd, pid })
}

#[cfg(windows)]
fn own_tree(child: &Child) -> Result<std::os::windows::io::OwnedHandle, &'static str> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::System::JobObjects::*;
    // SAFETY: new non-inheritable job, owned once; initialized limits and live child handle.
    let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if raw.is_null() {
        return Err("Không tạo được phiên Hotclip riêng.");
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
        return Err("Không quản lý được tiến trình Hotclip riêng.");
    }
    Ok(job)
}

fn run(
    data: PathBuf,
    shutdown: mpsc::Receiver<Control>,
    output: &mpsc::Sender<Event>,
) -> Result<(), &'static str> {
    #[cfg(not(windows))]
    {
        let _ = (data, shutdown, output);
        return Err("Hotclip nhúng hiện hỗ trợ Windows.");
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let data = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|p| p.join("portable")))
            .filter(|p| p.is_dir())
            .map(|p| p.join("data").join("hotclip"))
            .unwrap_or(data);
        let (root, exe, _main) = discover().ok_or(
            "Chưa tìm thấy bản Hotclip đã chuẩn bị. Kiểm tra thư mục hotclip cạnh Concat.",
        )?;
        if !data.is_absolute() {
            return Err("Thư mục dữ liệu Hotclip không hợp lệ.");
        }
        std::fs::create_dir_all(&data).map_err(|_| "Không tạo được vùng dữ liệu Hotclip.")?;
        let nonce = format!("{:032x}", rand::random::<u128>());
        let listener =
            TcpListener::bind("127.0.0.1:0").map_err(|_| "Không tạo được kênh Hotclip nội bộ.")?;
        listener
            .set_nonblocking(true)
            .map_err(|_| "Không tạo được kênh Hotclip nội bộ.")?;
        let port = listener
            .local_addr()
            .map_err(|_| "Không tạo được kênh Hotclip nội bộ.")?
            .port();
        let mut command = Command::new(exe);
        command
            .arg(&root)
            .current_dir(&root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x08000000);
        // Do not share Concat/provider credentials or developer Node injection.
        let allowed = [
            "SystemRoot",
            "WINDIR",
            "PATH",
            "PATHEXT",
            "TEMP",
            "TMP",
            "APPDATA",
            "LOCALAPPDATA",
            "USERPROFILE",
            "HOMEDRIVE",
            "HOMEPATH",
            "COMSPEC",
            "NUMBER_OF_PROCESSORS",
            "PROCESSOR_ARCHITECTURE",
        ];
        command.env_clear();
        for key in allowed {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        command
            .env("HOTCLIP_CONCAT_EMBED", "1")
            .env("HOTCLIP_CONCAT_NONCE", &nonce)
            .env("HOTCLIP_CONCAT_DATA", data)
            .env("HOTCLIP_CONCAT_PORT", port.to_string());
        #[cfg(debug_assertions)]
        if std::env::var("CONCAT_HOTCLIP_SMOKE").as_deref() == Ok("1") {
            if let Some(media) = std::env::var_os("CONCAT_HOTCLIP_SMOKE_MEDIA") {
                command.env("HOTCLIP_CONCAT_SMOKE_MEDIA", media);
            }
        }
        let mut child = command
            .spawn()
            .map_err(|_| "Không khởi động được Hotclip.")?;
        let job = match own_tree(&child) {
            Ok(job) => job,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let pid = child.id();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let mut socket: Option<std::net::TcpStream> = None;
        let started = Instant::now();
        let mut loaded = false;
        let result = loop {
            let mut stop = false;
            let mut visibility = None;
            while let Ok(control) = shutdown.try_recv() {
                match control {
                    Control::Shutdown => stop = true,
                    Control::Visible(show) => visibility = Some(show),
                }
            }
            if !stop {
                if let (Some(show), Some(socket)) = (visibility, socket.as_mut()) {
                    let _ = writeln!(
                        socket,
                        "{}",
                        serde_json::json!({"nonce":nonce,"command":if show {"show"}else{"hide"}})
                    );
                }
            }
            if stop {
                if let Some(socket) = socket.as_mut() {
                    let _ = writeln!(
                        socket,
                        "{}",
                        serde_json::json!({"nonce":nonce,"command":"shutdown"})
                    );
                }
                let deadline = Instant::now() + Duration::from_secs(4);
                while Instant::now() < deadline {
                    if child.try_wait().ok().flatten().is_some() {
                        break;
                    }
                    thread::sleep(Duration::from_millis(50));
                }
                break Ok(());
            }
            if socket.is_none() {
                if let Ok((stream, address)) = listener.accept() {
                    if !address.ip().is_loopback() {
                        continue;
                    }
                    // Windows accepts inherit the listener's nonblocking mode.
                    // The dedicated reader must wait for Electron's later ready
                    // message instead of exiting immediately on WouldBlock.
                    stream
                        .set_nonblocking(false)
                        .map_err(|_| "Không đọc được kênh Hotclip nội bộ.")?;
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(32)));
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
                    if let Ok(reader) = stream.try_clone() {
                        let reader_nonce = nonce.clone();
                        let ready_tx = ready_tx.clone();
                        thread::spawn(move || {
                            let mut line = Vec::new();
                            let mut oversized = false;
                            for byte in BufReader::new(reader).bytes() {
                                let Ok(byte) = byte else {
                                    break;
                                };
                                if byte == b'\n' {
                                    if !oversized {
                                        if let Ok(text) = std::str::from_utf8(&line) {
                                            if let Some(event) = ready(text, &reader_nonce, pid) {
                                                let _ = ready_tx.try_send(event);
                                            }
                                        }
                                    }
                                    line.clear();
                                    oversized = false;
                                } else if line.len() < 4096 {
                                    line.push(byte);
                                } else {
                                    oversized = true;
                                }
                            }
                        });
                        socket = Some(stream);
                    }
                }
            }
            if !loaded {
                if let Ok(event) = ready_rx.try_recv() {
                    loaded = true;
                    let _ = output.send(event);
                }
            }
            if child
                .try_wait()
                .map_err(|_| "Không đọc được tiến trình Hotclip.")?
                .is_some()
            {
                break Err("Hotclip đã dừng. Bấm Mở lại Hotclip để thử lại.");
            }
            if !loaded && started.elapsed() > Duration::from_secs(30) {
                break Err("Hotclip mở quá lâu hoặc chưa sẵn sàng. Bấm Mở lại Hotclip.");
            }
            thread::sleep(Duration::from_millis(100));
        };
        if let Some(socket) = socket {
            let _ = socket.shutdown(Shutdown::Both);
        }
        drop(job);
        let _ = child.wait();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handshake_requires_exact_nonce_owned_pid_and_nonzero_handle() {
        let nonce = "a".repeat(32);
        let message = |n: &str, pid, hwnd: &str| {
            format!(
                "CONCAT_HOTCLIP {}",
                serde_json::json!({"nonce":n,"event":"ready","pid":pid,"hwnd":hwnd})
            )
        };
        assert_eq!(
            ready(&message(&nonce, 7, "123"), &nonce, 7),
            Some(Event::Ready { hwnd: 123, pid: 7 })
        );
        for text in [
            message("wrong", 7, "123"),
            message(&nonce, 8, "123"),
            message(&nonce, 7, "0"),
            message(&nonce, 7, "-2"),
            "x".repeat(4097),
        ] {
            assert_eq!(ready(&text, &nonce, 7), None);
        }
    }
}
