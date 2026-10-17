// SPDX-License-Identifier: AGPL-3.0-or-later
//! User-configured local Nexora integration, not a browser implementation.
use crate::browser::{BrowserClient, BrowserError, BrowserProfile};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const ENTROPY: &str = "https://concat-nexora-local-settings.invalid";
/// Public endpoint/path and a private optional API credential. Never Debug.
#[derive(Serialize, Deserialize, Clone)]
pub struct NexoraSettings {
    /// Numeric loopback origin of the installed Nexora local API.
    pub origin: String,
    /// An explicitly chosen Nexora executable, not a shell command.
    pub executable: Option<PathBuf>,
    token: Option<String>,
}
impl Default for NexoraSettings {
    fn default() -> Self {
        Self {
            origin: "http://127.0.0.1:10108".into(),
            executable: None,
            token: None,
        }
    }
}
impl NexoraSettings {
    /// Load only Concat's own encrypted configuration, never browser cookies/stores.
    pub fn load(config: &Path) -> Result<Self, BrowserError> {
        let path = config.join("studio/nexora.dpapi");
        if !path.exists() {
            return Ok(Self::default());
        }
        let mut sealed = Vec::new();
        fs::File::open(path)
            .map_err(|_| BrowserError::Configuration)?
            .take(32769)
            .read_to_end(&mut sealed)
            .map_err(|_| BrowserError::Configuration)?;
        if sealed.len() > 32768 {
            return Err(BrowserError::Configuration);
        }
        let plain =
            crate::auth::unprotect(&sealed, ENTROPY).map_err(|_| BrowserError::Configuration)?;
        let value: Self =
            serde_json::from_slice(&plain).map_err(|_| BrowserError::Configuration)?;
        value.client()?;
        Ok(value)
    }
    /// Replace the optional token. An empty value explicitly forgets it.
    pub fn set_token(&mut self, value: &str) -> Result<(), BrowserError> {
        let token = if value.trim().is_empty() {
            None
        } else {
            Some(value.trim().to_string())
        };
        BrowserClient::with_token(&self.origin, token.clone())?;
        self.token = token;
        Ok(())
    }
    /// Only a presence bit is suitable for UI display.
    pub fn has_token(&self) -> bool {
        self.token.is_some()
    }
    /// Build the same local client for Studio, diagnostics and native OAuth.
    pub fn client(&self) -> Result<BrowserClient, BrowserError> {
        let origin = std::env::var("DONUT_BROWSER_API_URL").unwrap_or_else(|_| self.origin.clone());
        BrowserClient::with_token(&origin, self.token.clone())
    }
    /// Atomically store DPAPI ciphertext. No token is written in plaintext.
    pub fn save(&self, config: &Path) -> Result<(), BrowserError> {
        BrowserClient::with_token(&self.origin, self.token.clone())?;
        if let Some(exe) = &self.executable {
            if !exe.is_absolute()
                || exe
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_none_or(|s| !s.eq_ignore_ascii_case("exe"))
            {
                return Err(BrowserError::Configuration);
            }
        }
        let dir = config.join("studio");
        fs::create_dir_all(&dir).map_err(|_| BrowserError::Configuration)?;
        let bytes = serde_json::to_vec(self).map_err(|_| BrowserError::Configuration)?;
        let sealed =
            crate::auth::protect(&bytes, ENTROPY).map_err(|_| BrowserError::Configuration)?;
        let temporary = dir.join(format!("nexora-{}.tmp", rand::random::<u64>()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(&sealed)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, dir.join("nexora.dpapi"))
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result.map_err(|_| BrowserError::Configuration)
    }
    /// Resolve a packaged/installed or explicitly selected app, never a dev .bat.
    pub fn discover_executable(&self) -> Option<PathBuf> {
        if let Some(value) = std::env::var_os("CONCAT_NEXORA_EXE") {
            return valid_executable(PathBuf::from(value));
        }
        if let Some(value) = &self.executable {
            return valid_executable(value.clone());
        }
        let mut candidates = Vec::new();
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.extend([
                    dir.join("Nexora/nexorabrowser.exe"),
                    dir.join("resources/Nexora/nexorabrowser.exe"),
                ]);
            }
        }
        for variable in ["LOCALAPPDATA", "ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(base) = std::env::var_os(variable) {
                let base = PathBuf::from(base);
                candidates.extend([
                    base.join("Nexora/nexorabrowser.exe"),
                    base.join("Programs/Nexora/nexorabrowser.exe"),
                ]);
            }
        }
        candidates.into_iter().find_map(valid_executable)
    }
    /// Explicit user action only. Reuse a ready manager; never kill port owners.
    pub fn start(&self) -> Result<Vec<BrowserProfile>, BrowserError> {
        let client = self.client()?;
        match client.health() {
            Ok(()) => return client.profiles(),
            Err(BrowserError::Unavailable) => {}
            Err(error) => return Err(error),
        }
        // An occupied/unresponsive port isn't permission to start a duplicate.
        let origin = std::env::var("DONUT_BROWSER_API_URL").unwrap_or_else(|_| self.origin.clone());
        let url = url::Url::parse(&origin).map_err(|_| BrowserError::Configuration)?;
        let host = url
            .host_str()
            .ok_or(BrowserError::Configuration)?
            .trim_matches(['[', ']']);
        let ip: std::net::IpAddr = host.parse().map_err(|_| BrowserError::Configuration)?;
        if std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::new(ip, url.port_or_known_default().unwrap_or(80)),
            Duration::from_millis(200),
        )
        .is_ok()
        {
            return Err(BrowserError::Unavailable);
        }
        let executable = self
            .discover_executable()
            .ok_or(BrowserError::Configuration)?;
        let mut command = Command::new(&executable);
        command
            .current_dir(executable.parent().ok_or(BrowserError::Configuration)?)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().map_err(|_| BrowserError::Unavailable)?;
        let deadline = Instant::now() + Duration::from_secs(35);
        while Instant::now() < deadline {
            if child
                .try_wait()
                .map_err(|_| BrowserError::Unavailable)?
                .is_some_and(|status| !status.success())
            {
                return Err(BrowserError::Unavailable);
            }
            if client.health().is_ok() {
                return client.profiles();
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        // Nexora is an independent app; leave it running and don't replay launch.
        Err(BrowserError::Unavailable)
    }
}
fn valid_executable(path: PathBuf) -> Option<PathBuf> {
    (path.is_absolute()
        && path.is_file()
        && path
            .extension()
            .is_some_and(|v| v.eq_ignore_ascii_case("exe")))
    .then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "read-only check against an explicitly running Nexora local manager"]
    fn running_nexora_health_and_profiles_are_real() {
        let client = NexoraSettings::default().client().unwrap();
        client.health().expect("actual Nexora protocol");
        let profiles = client.profiles().expect("actual profile catalogue");
        assert!(profiles.len() <= 1000);
        for profile in profiles {
            assert!(!profile.id.is_empty());
        }
    }
    #[cfg(windows)]
    #[test]
    fn settings_keep_token_encrypted_and_preserve_it_on_unrelated_edits() {
        let root =
            std::env::temp_dir().join(format!("concat-nexora-test-{}", rand::random::<u64>()));
        let mut settings = NexoraSettings::default();
        settings.set_token("fixture-private-local-token").unwrap();
        settings.save(&root).unwrap();
        let bytes = std::fs::read(root.join("studio/nexora.dpapi")).unwrap();
        let needle = b"fixture-private-local-token";
        assert!(!bytes.windows(needle.len()).any(|v| v == needle));
        let mut loaded = NexoraSettings::load(&root).unwrap();
        assert!(loaded.has_token());
        loaded.origin = "http://127.0.0.1:10109".into();
        loaded.save(&root).unwrap();
        assert!(NexoraSettings::load(&root).unwrap().has_token());
        loaded.set_token("").unwrap();
        loaded.save(&root).unwrap();
        assert!(!NexoraSettings::load(&root).unwrap().has_token());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn settings_refuse_cloud_endpoints_and_header_injection() {
        let mut settings = NexoraSettings::default();
        assert!(settings.set_token("bad\r\nheader").is_err());
        settings.origin = "https://remote.example".into();
        assert!(settings.client().is_err());
    }
}
