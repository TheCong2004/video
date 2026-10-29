//! Own only the Playwright adapter process, never the user's Nexora browser.
use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
pub struct BrowserRuntime {
    child: Option<Child>,
    #[cfg(windows)]
    _job: Option<std::os::windows::io::OwnedHandle>,
}
fn healthy() -> bool {
    let response = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(Duration::from_secs(1))
        .build()
        .get("http://127.0.0.1:9223/health")
        .call();
    response
        .ok()
        .and_then(|r| {
            use std::io::Read;
            let mut bytes = vec![];
            r.into_reader().take(4097).read_to_end(&mut bytes).ok()?;
            serde_json::from_slice::<serde_json::Value>(&bytes).ok()
        })
        .is_some_and(|v| v["service"] == "floword-playwright-runtime" && v["protocolVersion"] == 1)
}
impl BrowserRuntime {
    pub fn ensure() -> Result<Self, String> {
        if healthy() {
            return Ok(Self {
                child: None,
                #[cfg(windows)]
                _job: None,
            });
        }
        let app = std::env::current_exe().map_err(|_| "RUNTIME_PATH_UNAVAILABLE")?;
        let base = app.parent().ok_or("RUNTIME_PATH_UNAVAILABLE")?;
        let resources = [
            base.join("resources/playwright-sidecar"),
            base.join("../../../../artcraft/tools/playwright-sidecar"),
        ]
        .into_iter()
        .find(|p| {
            p.join("src/server.js").is_file()
                && p.join("node_modules/playwright/package.json").is_file()
        })
        .ok_or("PLAYWRIGHT_RUNTIME_NOT_INSTALLED")?;
        let node = [
            base.join("resources/node/node.exe"),
            base.join("../../../.deps/omniroute/node.exe"),
        ]
        .into_iter()
        .find(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("node"));
        let mut command = Command::new(node);
        command
            .arg(
                crate::local_ai::node_path(&resources.join("src/server.js"))
                    .map_err(|_| "RUNTIME_PATH_UNAVAILABLE")?,
            )
            .current_dir(&resources)
            .env("PLAYWRIGHT_SIDECAR_PORT", "9223")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let child = command
            .spawn()
            .map_err(|_| "PLAYWRIGHT_RUNTIME_START_FAILED")?;
        let mut owned = Self {
            child: Some(child),
            #[cfg(windows)]
            _job: None,
        };
        #[cfg(windows)]
        {
            owned._job = Some(
                crate::local_ai::owned_job(owned.child.as_ref().ok_or("RUNTIME_PROCESS_MISSING")?)
                    .map_err(|_| "RUNTIME_PROCESS_OWNERSHIP_FAILED")?,
            );
        }
        let until = Instant::now() + Duration::from_secs(15);
        while Instant::now() < until {
            if owned
                .child
                .as_mut()
                .and_then(|c| c.try_wait().ok())
                .flatten()
                .is_some()
            {
                return Err("PLAYWRIGHT_RUNTIME_START_FAILED".into());
            }
            if healthy() {
                return Ok(owned);
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        Err("PLAYWRIGHT_RUNTIME_START_FAILED".into())
    }
}
impl Drop for BrowserRuntime {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
