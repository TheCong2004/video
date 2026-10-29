// SPDX-License-Identifier: AGPL-3.0-or-later
//! App-owned SLive process; no cloud, Promax or user-managed backend.
use crate::promax_avatars::{AvatarClient, AvatarError};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

static RUNTIME: OnceLock<Mutex<Option<Runtime>>> = OnceLock::new();

struct Runtime {
    child: Child,
    origin: String,
    token: String,
    _lock: File,
    #[cfg(windows)]
    _job: Option<std::os::windows::io::OwnedHandle>,
}

impl Drop for Runtime {
    fn drop(&mut self) {
        // Closing stdin tells the Python service to shut down its workers.
        self.child.stdin.take();
        #[cfg(windows)]
        {
            self._job.take();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn resources() -> Result<PathBuf, AvatarError> {
    let mut roots = Vec::new();
    if let Some(path) = std::env::var_os("CONCAT_AVATAR_RUNTIME_DIR") {
        roots.push(PathBuf::from(path));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            roots.push(parent.join("AvatarRuntime"));
        }
    }
    roots.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.."));
    roots
        .into_iter()
        .find(|root| {
            root.join("integrations/slive/runtime/avatar_server.py")
                .is_file()
        })
        .ok_or(AvatarError::Configuration)
}

fn python(root: &Path) -> Result<(PathBuf, PathBuf), AvatarError> {
    // Use the copied base Python plus venv packages: no absolute pyvenv.cfg
    // reference back to Promax and no system-Python dependency after relocation.
    #[cfg(windows)]
    let (exe, packages) = (
        root.join(".runtime/python/cpython-3.11.15-windows-x86_64-none/python.exe"),
        root.join(".runtime/slive-venv/Lib/site-packages"),
    );
    #[cfg(not(windows))]
    let (exe, packages) = (
        root.join(".runtime/slive-venv/bin/python"),
        root.join(".runtime/slive-venv/lib/python3.11/site-packages"),
    );
    if !exe.is_file() || !packages.is_dir() {
        return Err(AvatarError::Configuration);
    }
    Ok((exe, packages))
}

fn start(data: &Path) -> Result<Runtime, AvatarError> {
    let root = resources()?
        .canonicalize()
        .map_err(|_| AvatarError::Configuration)?;
    let (python, packages) = python(&root)?;
    fs::create_dir_all(data).map_err(|_| AvatarError::Configuration)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(data.join("runtime.lock"))
        .map_err(|_| AvatarError::Configuration)?;
    lock.try_lock().map_err(|_| AvatarError::NotReady)?;
    let token = rand::random::<[u8; 32]>()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let receipt = data.join(format!("endpoint-{}.json", std::process::id()));
    let _ = fs::remove_file(&receipt);
    let mut cmd = Command::new(python);
    cmd.arg("-B")
        .arg("-s")
        .arg(root.join("integrations/slive/runtime/avatar_server.py"))
        .env_clear()
        .env("PYTHONPATH", packages)
        .env("PYTHONUTF8", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env(
            "CONCAT_SLIVE_S3FD",
            root.join(".runtime/slive-models/s3fd.pth"),
        )
        .current_dir(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for key in [
        "PATH",
        "SystemRoot",
        "WINDIR",
        "TEMP",
        "TMP",
        "CUDA_VISIBLE_DEVICES",
    ] {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    // Prefer bundled FFmpeg/FFprobe. The dev launcher also supplies these on PATH.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            for (key, name) in [
                ("CONCAT_SLIVE_FFMPEG", "ffmpeg.exe"),
                ("CONCAT_SLIVE_FFPROBE", "ffprobe.exe"),
            ] {
                if parent.join(name).is_file() {
                    cmd.env(key, parent.join(name));
                }
            }
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let child = cmd.spawn().map_err(|_| AvatarError::Unavailable)?;
    let mut runtime = Runtime {
        child,
        origin: String::new(),
        token,
        _lock: lock,
        #[cfg(windows)]
        _job: None,
    };
    #[cfg(windows)]
    {
        runtime._job =
            Some(crate::local_ai::owned_job(&runtime.child).map_err(|_| AvatarError::Unavailable)?);
    }
    let config = serde_json::json!({"token": runtime.token, "data": data.join("library"), "receipt": receipt});
    writeln!(
        runtime
            .child
            .stdin
            .as_mut()
            .ok_or(AvatarError::Unavailable)?,
        "{config}"
    )
    .map_err(|_| AvatarError::Unavailable)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if runtime
            .child
            .try_wait()
            .map_err(|_| AvatarError::Unavailable)?
            .is_some()
        {
            break;
        }
        if let Ok(bytes) = fs::read(&receipt) {
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                if let Some(port) = value["port"]
                    .as_u64()
                    .filter(|port| (1..=65535).contains(port))
                {
                    runtime.origin = format!("http://127.0.0.1:{port}");
                    let _ = fs::remove_file(&receipt);
                    return Ok(runtime);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = fs::remove_file(&receipt);
    Err(AvatarError::Unavailable)
}

/// Lazily start the owned local service on a worker thread. No upload on startup.
pub fn client(data: &Path) -> Result<AvatarClient, AvatarError> {
    let mut slot = RUNTIME
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| AvatarError::Unavailable)?;
    if let Some(runtime) = slot.as_mut() {
        if runtime
            .child
            .try_wait()
            .map_err(|_| AvatarError::Unavailable)?
            .is_some()
        {
            slot.take();
        }
    }
    if slot.is_none() {
        *slot = Some(start(data)?);
    }
    let runtime = slot.as_ref().ok_or(AvatarError::Unavailable)?;
    AvatarClient::new(&runtime.origin, Some(runtime.token.clone()))
}

/// Stop only the process tree owned by this app instance.
pub fn shutdown() {
    if let Some(slot) = RUNTIME.get() {
        if let Ok(mut slot) = slot.lock() {
            slot.take();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_loopback_rejects_browser_and_stops_on_drop() {
        // Explicit opt-in to the staged runtime, never downloads dependencies.
        if std::env::var_os("CONCAT_LOCAL_AVATAR_RUNTIME_TEST").is_none() {
            return;
        }
        let data =
            std::env::temp_dir().join(format!("concat-local-avatar-{}", rand::random::<u64>()));
        let mut runtime = start(&data).unwrap();
        let url = format!("{}/api/v1/concat/avatars", runtime.origin);
        assert!(matches!(
            ureq::get(&url).call(),
            Err(ureq::Error::Status(403, _))
        ));
        let authorization = format!("Bearer {}", runtime.token);
        assert!(matches!(
            ureq::get(&url)
                .set("Authorization", &authorization)
                .set("Origin", "https://example.invalid")
                .call(),
            Err(ureq::Error::Status(403, _))
        ));
        assert_eq!(
            AvatarClient::new(&runtime.origin, Some(runtime.token.clone()))
                .unwrap()
                .list()
                .unwrap()
                .len(),
            0
        );
        assert!(matches!(start(&data), Err(AvatarError::NotReady)));
        assert!(runtime.child.try_wait().unwrap().is_none());
        drop(runtime);
        assert!(
            ureq::get(&url)
                .timeout(Duration::from_secs(1))
                .call()
                .is_err()
        );
        // Leave this small isolated fixture for diagnosis; no recursive cleanup.
    }
}
