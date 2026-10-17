// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native boundary to Nexora's existing local Chromium manager, not a browser.
use serde_json::{Value, json};
use std::{io::Read, time::Duration};

/// Sanitized local browser failures. No upstream body, cookie or path is exposed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BrowserError {
    /// Only numeric loopback HTTP origins are accepted.
    Configuration,
    /// Discovery failed, including authorization failure.
    Unavailable,
    /// Unsupported or oversized response.
    InvalidResponse,
    /// Selected profile has changed; refresh and explicitly confirm again.
    Stale,
    /// A lifecycle request may have succeeded; do not automatically repeat it.
    UnknownOutcome,
    /// Local API requires its own token, not a Promax/cloud credential.
    Authentication,
    /// Nexora is present but its local API version is incompatible.
    UnsupportedApi,
}

/// Profile identity without cookies, proxy passwords or filesystem paths.
#[derive(Clone, Debug, PartialEq)]
#[allow(missing_docs)]
pub struct BrowserProfile {
    pub id: String,
    pub name: String,
    pub running: bool,
    pub pid: Option<u32>,
    pub generation: Option<u64>,
}

/// A local client only. Never starts Nexora or owns browser child processes.
pub struct BrowserClient {
    origin: String,
    token: Option<String>,
}

impl BrowserClient {
    /// Connect explicitly to the local Nexora manager.
    pub fn new(origin: &str) -> Result<Self, BrowserError> {
        let url = url::Url::parse(origin).map_err(|_| BrowserError::Configuration)?;
        let local = match url.host() {
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            _ => false,
        };
        if url.scheme() != "http"
            || !local
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
            || url.port() == Some(0)
        {
            return Err(BrowserError::Configuration);
        }
        Ok(Self {
            origin: url.as_str().trim_end_matches('/').into(),
            token: None,
        })
    }
    /// Optional local bearer, scoped to numeric loopback with no redirects.
    pub fn with_token(origin: &str, token: Option<String>) -> Result<Self, BrowserError> {
        let mut client = Self::new(origin)?;
        if token.as_ref().is_some_and(|s| {
            s.is_empty() || s.len() > 8192 || !s.bytes().all(|b| b.is_ascii_graphic())
        }) {
            return Err(BrowserError::Configuration);
        }
        client.token = token;
        Ok(client)
    }
    /// Confirm Nexora's actual runtime protocol, not just an arbitrary open port.
    pub fn health(&self) -> Result<(), BrowserError> {
        let value = self.request("GET", "/v1/runtime/health", None)?;
        if value["protocol"] != "floword-production"
            || value["protocolVersion"] != 1
            || value["status"] != "READY"
            || !matches!(
                value["runtime"].as_str(),
                Some("nexorabrowser" | "floword-nexora-runtime")
            )
        {
            return Err(BrowserError::UnsupportedApi);
        }
        Ok(())
    }

    /// Read the canonical Chromium profile catalogue.
    pub fn profiles(&self) -> Result<Vec<BrowserProfile>, BrowserError> {
        parse_profiles(&self.request("GET", "/v1/local/browser/profiles", None)?)
    }

    /// Explicitly open Grok in exactly the chosen persistent profile.
    pub fn open_grok(&self, selected: &BrowserProfile) -> Result<(), BrowserError> {
        self.open_https(selected, "https://grok.com/")
    }

    /// Open an explicitly approved HTTPS page inside the selected profile.
    /// Used by native OAuth; never accepts file/script/plain HTTP URLs.
    pub fn open_https(&self, selected: &BrowserProfile, target: &str) -> Result<(), BrowserError> {
        let url = url::Url::parse(target).map_err(|_| BrowserError::Configuration)?;
        if url.scheme() != "https"
            || url.host().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || target.len() > 8192
        {
            return Err(BrowserError::Configuration);
        }
        self.check_current(selected)?;
        let path = profile_path(&selected.id)?;
        let result=self.request("POST",&format!("{path}/run"),Some(&json!({
            "url":target, "headless":false,"cold_start_only":true,"browser_engine":"chromium"
        }))).map_err(|_|BrowserError::UnknownOutcome)?;
        if result.get("profileId").and_then(Value::as_str) != Some(selected.id.as_str())
            || result
                .get("browserPid")
                .and_then(Value::as_u64)
                .filter(|pid| *pid > 0 && *pid <= u32::MAX as u64)
                .is_none()
            || result
                .get("launchGeneration")
                .and_then(Value::as_u64)
                .filter(|g| *g > 0)
                .is_none()
        {
            return Err(BrowserError::UnknownOutcome);
        }
        Ok(())
    }

    /// Stop only the confirmed PID/generation. Never issue a blanket process kill.
    pub fn stop(&self, selected: &BrowserProfile) -> Result<(), BrowserError> {
        if !selected.running || selected.pid.is_none() || selected.generation.is_none() {
            return Err(BrowserError::Stale);
        }
        self.check_current(selected)?;
        let path = profile_path(&selected.id)?;
        let result=self.request("POST",&format!("{path}/stop"),Some(&json!({
            "profileId":selected.id,"browserPid":selected.pid,"launchGeneration":selected.generation
        }))).map_err(|_|BrowserError::UnknownOutcome)?;
        if result.get("stopped_pid").and_then(Value::as_u64) != selected.pid.map(u64::from)
            || result.get("ok").and_then(Value::as_bool) != Some(true)
            || result.get("profile_id").and_then(Value::as_str) != Some(selected.id.as_str())
            || result.get("launch_generation").and_then(Value::as_u64) != selected.generation
        {
            return Err(BrowserError::UnknownOutcome);
        }
        Ok(())
    }

    fn check_current(&self, selected: &BrowserProfile) -> Result<(), BrowserError> {
        profile_path(&selected.id)?;
        if !self.profiles()?.iter().any(|p| p == selected) {
            return Err(BrowserError::Stale);
        }
        Ok(())
    }

    fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, BrowserError> {
        let agent = ureq::AgentBuilder::new()
            .redirects(0)
            .timeout(Duration::from_secs(if body.is_some() { 25 } else { 8 }))
            .build();
        let mut req = agent
            .request(method, &format!("{}{path}", self.origin))
            .set("Accept", "application/json");
        if let Some(token) = &self.token {
            req = req.set("Authorization", &format!("Bearer {token}"));
        }
        let response = if let Some(body) = body {
            req.set("Content-Type", "application/json")
                .send_bytes(&serde_json::to_vec(body).map_err(|_| BrowserError::Configuration)?)
        } else {
            req.call()
        };
        let response = response.map_err(|error| match error {
            ureq::Error::Status(401 | 403, _) => BrowserError::Authentication,
            ureq::Error::Status(404, _) => BrowserError::UnsupportedApi,
            _ => BrowserError::Unavailable,
        })?;
        if !matches!(response.status(), 200 | 201) {
            return Err(BrowserError::Unavailable);
        }
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| BrowserError::InvalidResponse)?;
        if bytes.len() > 1024 * 1024 {
            return Err(BrowserError::InvalidResponse);
        }
        serde_json::from_slice(&bytes).map_err(|_| BrowserError::InvalidResponse)
    }
}

fn profile_path(id: &str) -> Result<String, BrowserError> {
    if id.is_empty()
        || id.len() > 256
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
    {
        return Err(BrowserError::Configuration);
    }
    Ok(format!("/v1/local/browser/profiles/{id}"))
}

fn parse_profiles(value: &Value) -> Result<Vec<BrowserProfile>, BrowserError> {
    let rows = value
        .get("profiles")
        .and_then(Value::as_array)
        .filter(|v| v.len() <= 1000)
        .ok_or(BrowserError::InvalidResponse)?;
    let mut seen = std::collections::HashSet::new();
    rows.iter()
        .map(|v| {
            let id = v
                .get("id")
                .and_then(Value::as_str)
                .ok_or(BrowserError::InvalidResponse)?;
            profile_path(id).map_err(|_| BrowserError::InvalidResponse)?;
            if !seen.insert(id) {
                return Err(BrowserError::InvalidResponse);
            }
            let name = v
                .get("name")
                .and_then(Value::as_str)
                .filter(|s| s.len() <= 512 && !s.chars().any(char::is_control))
                .ok_or(BrowserError::InvalidResponse)?;
            let running = v
                .get("is_running")
                .and_then(Value::as_bool)
                .ok_or(BrowserError::InvalidResponse)?;
            let pid = v
                .get("process_id")
                .and_then(Value::as_u64)
                .filter(|v| *v > 0 && *v <= u32::MAX as u64)
                .map(|v| v as u32);
            let generation = v
                .get("last_launch")
                .and_then(Value::as_u64)
                .filter(|v| *v > 0);
            if running && (pid.is_none() || generation.is_none()) {
                return Err(BrowserError::InvalidResponse);
            }
            Ok(BrowserProfile {
                id: id.into(),
                name: name.into(),
                running,
                pid,
                generation,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, net::TcpListener};
    #[test]
    fn bearer_is_scoped_to_loopback_and_auth_failure_is_explicit() {
        assert!(
            BrowserClient::with_token("https://cloud.example", Some("fixture".into())).is_err()
        );
        assert!(
            BrowserClient::with_token("http://127.0.0.1:1", Some("bad\r\nheader".into())).is_err()
        );
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = BrowserClient::with_token(
            &format!("http://{}", listener.local_addr().unwrap()),
            Some("local-fixture".into()),
        )
        .unwrap();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut bytes = [0; 4096];
            let count = stream.read(&mut bytes).unwrap();
            let sent = String::from_utf8_lossy(&bytes[..count]).to_lowercase();
            assert!(sent.contains("authorization: bearer local-fixture"));
            stream.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").unwrap();
        });
        assert_eq!(client.profiles(), Err(BrowserError::Authentication));
        worker.join().unwrap();
    }
    fn lifecycle_fixture(
        stop: bool,
        reply: Value,
    ) -> (BrowserClient, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client =
            BrowserClient::new(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let worker = std::thread::spawn(move || {
            let mut last = String::new();
            for step in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut chunk = [0; 2048];
                loop {
                    let n = stream.read(&mut chunk).unwrap();
                    bytes.extend_from_slice(&chunk[..n]);
                    let s = String::from_utf8_lossy(&bytes);
                    if let Some(end) = s.find("\r\n\r\n") {
                        let length = s[..end]
                            .lines()
                            .find_map(|l| {
                                l.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|s| s.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                    assert_ne!(n, 0);
                }
                last = String::from_utf8(bytes).unwrap();
                let body=if step==0 {
                    assert!(last.starts_with("GET /v1/local/browser/profiles "));
                    json!({"profiles":[{"id":"a","name":"main","is_running":true,"process_id":12,"last_launch":99}]})
                } else {
                    assert!(last.starts_with(if stop {"POST /v1/local/browser/profiles/a/stop "} else {"POST /v1/local/browser/profiles/a/run "}));
                    reply.clone()
                }.to_string();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
            last
        });
        (client, worker)
    }
    fn selected() -> BrowserProfile {
        BrowserProfile {
            id: "a".into(),
            name: "main".into(),
            running: true,
            pid: Some(12),
            generation: Some(99),
        }
    }
    #[test]
    fn open_uses_existing_profile_and_checks_actual_launch_identity() {
        let (client, worker) = lifecycle_fixture(
            false,
            json!({"profileId":"a","browserPid":12,"launchGeneration":99}),
        );
        client.open_grok(&selected()).unwrap();
        let request = worker.join().unwrap();
        assert!(request.contains("https://grok.com/"));
        assert!(request.contains("\"headless\":false"));
    }
    #[test]
    fn stop_sends_exact_confirmed_session_and_rejects_unproven_success() {
        let reply = json!({"profile_id":"a","ok":true,"stopped_pid":12,"launch_generation":99});
        let (client, worker) = lifecycle_fixture(true, reply);
        client.stop(&selected()).unwrap();
        let request = worker.join().unwrap();
        assert!(request.contains("\"browserPid\":12"));
        assert!(request.contains("\"launchGeneration\":99"));
        let (client, worker) = lifecycle_fixture(true, json!({"ok":true,"stopped_pid":22}));
        assert_eq!(client.stop(&selected()), Err(BrowserError::UnknownOutcome));
        worker.join().unwrap();
    }
    #[test]
    fn loopback_and_path_restrictions() {
        for origin in [
            "https://example.com",
            "http://localhost:10108",
            "http://127.0.0.1:1/x",
            "http://user@127.0.0.1:1",
            "http://127.0.0.1:1?token=x",
        ] {
            assert!(BrowserClient::new(origin).is_err());
        }
        assert!(BrowserClient::new("http://127.0.0.1:10108").is_ok());
        assert!(profile_path("../other").is_err());
    }
    #[test]
    fn profile_metadata_is_bounded_and_credentials_are_discarded() {
        let value = json!({"profiles":[{"id":"a","name":"main","is_running":true,"process_id":12,"last_launch":99,"cookies":"secret"}]});
        let parsed = parse_profiles(&value).unwrap();
        assert!(!format!("{parsed:?}").contains("secret"));
        assert!(
            parse_profiles(&json!({"profiles":[{"id":"a","name":"x","is_running":true}]})).is_err()
        );
    }
    #[test]
    fn stale_profile_prevents_any_mutation() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client =
            BrowserClient::new(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0; 4096];
            let n = stream.read(&mut buffer).unwrap();
            assert!(
                String::from_utf8_lossy(&buffer[..n])
                    .starts_with("GET /v1/local/browser/profiles ")
            );
            let body=json!({"profiles":[{"id":"a","name":"main","is_running":true,"process_id":22,"last_launch":100}]}).to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        let selected = BrowserProfile {
            id: "a".into(),
            name: "main".into(),
            running: true,
            pid: Some(12),
            generation: Some(99),
        };
        assert_eq!(client.stop(&selected), Err(BrowserError::Stale));
        worker.join().unwrap();
    }
}
