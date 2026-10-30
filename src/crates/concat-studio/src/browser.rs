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
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[allow(missing_docs)]
pub struct BrowserProfile {
    pub id: String,
    pub name: String,
    pub running: bool,
    pub pid: Option<u32>,
    pub generation: Option<u64>,
}

/// A local client only. Never starts Nexora or owns browser child processes.
#[derive(Clone)]
pub struct BrowserClient {
    origin: String,
    token: Option<String>,
}

impl BrowserClient {
    /// Read actual Grok readiness for the selected profile, never infer it from
    /// a running browser or a nonempty Page catalogue.
    pub fn production_browser_ready(&self, profile: &str) -> Result<bool, String> {
        let value = self
            .request_bounded(
                "GET",
                "/v1/workers",
                None,
                Duration::from_secs(8),
                1024 * 1024,
            )
            .map_err(|_| "GROK_WORKER_UNAVAILABLE")?;
        let workers = value["workers"].as_array().ok_or("GROK_WORKER_INVALID")?;
        Ok(workers.iter().any(|w| {
            w["profile_id"] == profile
                && w["state"] == "READY"
                && w["extension_ready"] == true
                && w["grok_logged_in"] == true
                && w["capabilities"].as_array().is_some_and(|caps| {
                    [
                        "grok.image.edit",
                        "grok.image.expand_9_16",
                        "grok.video.generate",
                    ]
                    .iter()
                    .all(|cap| caps.iter().any(|v| v == cap))
                })
        }))
    }
    /// Non-secret identity of Nexora's existing local device principal.
    pub fn production_worker_identity(
        &self,
        profile: &str,
    ) -> Result<crate::promax_worker::WorkerIdentity, String> {
        let req = ureq::AgentBuilder::new()
            .redirects(0)
            .timeout(Duration::from_secs(30))
            .build()
            .post(&format!(
                "{}/v1/local/concat/production/worker-identity",
                self.origin
            ))
            .set("X-Concat-Integration", "1")
            .set("Content-Type", "application/json");
        let response = req
            .send_string(&json!({"profileId":profile}).to_string())
            .map_err(|_| "NEXORA_WORKER_BRIDGE_REQUIRED")?;
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(8193)
            .read_to_end(&mut bytes)
            .map_err(|_| "NEXORA_WORKER_IDENTITY_INVALID")?;
        if bytes.len() > 8192 {
            return Err("NEXORA_WORKER_IDENTITY_INVALID".into());
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| "NEXORA_WORKER_IDENTITY_INVALID")?;
        if value["ok"] != true {
            return Err("NEXORA_WORKER_BRIDGE_REQUIRED".into());
        }
        let identity: crate::promax_worker::WorkerIdentity =
            serde_json::from_value(value["data"].clone())
                .map_err(|_| "NEXORA_WORKER_IDENTITY_INVALID")?;
        if [
            &identity.client_id,
            &identity.device_id,
            &identity.tenant_id,
            &identity.user_id,
        ]
        .iter()
        .any(|s| !crate::promax_worker_protocol::safe_id(s))
        {
            return Err("NEXORA_WORKER_IDENTITY_INVALID".into());
        }
        Ok(identity)
    }
    pub(crate) fn production_worker_url(&self, profile: &str) -> Result<url::Url, String> {
        let mut url = url::Url::parse(&self.origin).map_err(|_| "NEXORA_ORIGIN_INVALID")?;
        if url.host_str() != Some("127.0.0.1") || url.scheme() != "http" {
            return Err("NEXORA_ORIGIN_INVALID".into());
        }
        url.set_scheme("ws").map_err(|_| "NEXORA_ORIGIN_INVALID")?;
        url.set_path("/v1/local/concat/production/worker");
        url.query_pairs_mut().append_pair("profileId", profile);
        Ok(url)
    }
    /// Scoped bridge into Nexora's native Promax connection. No cloud token export.
    pub(crate) fn production_request(
        &self,
        profile: &str,
        method: &str,
        path: &str,
        body: &Value,
        timeout: Duration,
    ) -> Result<Value, crate::workflows::WorkflowError> {
        use crate::workflows::WorkflowError as E;
        if !self.origin.starts_with("http://127.0.0.1:")
            || !matches!(method, "GET" | "POST")
            || !path.starts_with("/api/v1/artcraft/")
            || path.len() > 1024
            || path.contains('%')
            || path.contains("..")
            || path.contains('#')
        {
            return Err(E::Configuration);
        }
        let encoded = serde_json::to_vec(
            &serde_json::json!({"profileId":profile,"method":method,"path":path,"body":body}),
        )
        .map_err(|_| E::Configuration)?;
        if encoded.len() > 1024 * 1024 {
            return Err(E::Configuration);
        }
        let req = ureq::AgentBuilder::new()
            .redirects(0)
            .timeout(timeout.max(Duration::from_secs(25)))
            .build()
            .post(&format!(
                "{}/v1/local/concat/production/request",
                self.origin
            ))
            .set("Content-Type", "application/json")
            .set("Accept", "application/json")
            .set("X-Concat-Integration", "1");
        let response = match req.send_bytes(&encoded) {
            Ok(r) if matches!(r.status(), 200 | 201) => r,
            Err(ureq::Error::Status(401, _)) => return Err(E::Authentication),
            Err(ureq::Error::Status(403, _)) => return Err(E::Forbidden),
            Err(ureq::Error::Status(429, _)) => return Err(E::RateLimited),
            _ => return Err(E::Unavailable),
        };
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| E::InvalidResponse)?;
        if bytes.len() > 1024 * 1024 {
            return Err(E::InvalidResponse);
        }
        serde_json::from_slice(&bytes).map_err(|_| E::InvalidResponse)
    }
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

    /// User-triggered production startup. Reuse any Grok tab in this exact
    /// profile; never navigate other tabs or repeat an uncertain page creation.
    pub fn ensure_production_grok(&self, selected: &BrowserProfile) -> Result<(), BrowserError> {
        self.check_current(selected)?;
        if !selected.running {
            self.open_grok(selected)?;
        }
        let current = self
            .profiles()?
            .into_iter()
            .find(|p| p.id == selected.id)
            .filter(|p| p.running && p.pid.is_some() && p.generation.is_some())
            .ok_or(BrowserError::Stale)?;
        let path = format!("{}/pages", profile_path(&current.id)?);
        let pages = self.request("GET", &path, None)?;
        if pages["profileId"] != current.id
            || pages["browserPid"].as_u64() != current.pid.map(u64::from)
            || pages["launchGeneration"].as_u64() != current.generation
        {
            return Err(BrowserError::Stale);
        }
        let pages = pages["pages"]
            .as_array()
            .ok_or(BrowserError::InvalidResponse)?;
        if pages.iter().any(|page| is_grok_page(&page["url"])) {
            return Ok(());
        }
        self.check_current(&current)?;
        let created = self.request("POST", &path, Some(&json!({
            "url":"https://grok.com/imagine", "purpose":"concat-production", "targetKind":"GROK"
        }))).map_err(|_| BrowserError::UnknownOutcome)?;
        let page = &created["page"];
        if !is_grok_page(&page["url"])
            || page["browserPid"].as_u64() != current.pid.map(u64::from)
            || page["launchGeneration"].as_u64() != current.generation
            || page["targetId"].as_str().is_none_or(str::is_empty)
        {
            return Err(BrowserError::UnknownOutcome);
        }
        Ok(())
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
        self.request_bounded(
            method,
            path,
            body,
            Duration::from_secs(if body.is_some() { 25 } else { 8 }),
            1024 * 1024,
        )
    }

    pub(crate) fn request_bounded(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
        timeout: Duration,
        limit: u64,
    ) -> Result<Value, BrowserError> {
        if !path.starts_with("/v1/") || path.contains('?') || path.contains('#') {
            return Err(BrowserError::Configuration);
        }
        let agent = ureq::AgentBuilder::new()
            .redirects(0)
            .timeout(timeout)
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
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| BrowserError::InvalidResponse)?;
        if bytes.len() as u64 > limit {
            return Err(BrowserError::InvalidResponse);
        }
        serde_json::from_slice(&bytes).map_err(|_| BrowserError::InvalidResponse)
    }
}

fn is_grok_page(value: &Value) -> bool {
    value
        .as_str()
        .and_then(|s| url::Url::parse(s).ok())
        .is_some_and(|url| {
            url.scheme() == "https"
                && url.username().is_empty()
                && url.password().is_none()
                && url
                    .host_str()
                    .is_some_and(|h| h == "grok.com" || h.ends_with(".grok.com"))
        })
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
    fn grok_fixture(
        steps: Vec<(&'static str, Value)>,
    ) -> (BrowserClient, std::thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let client =
            BrowserClient::new(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let worker = std::thread::spawn(move || {
            let mut requests = vec![];
            for (expected, reply) in steps {
                let until = std::time::Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    if let Ok((stream, _)) = listener.accept() {
                        break stream;
                    }
                    assert!(
                        std::time::Instant::now() < until,
                        "missing request: {expected}"
                    );
                    std::thread::sleep(Duration::from_millis(5));
                };
                // Windows can inherit the listener's nonblocking mode on accept.
                // Keep the accept loop bounded, but read each fixture request normally.
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut bytes = vec![];
                let mut chunk = [0u8; 4096];
                loop {
                    let n = stream.read(&mut chunk).unwrap();
                    assert_ne!(n, 0);
                    bytes.extend_from_slice(&chunk[..n]);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&bytes[..end]);
                        let length = head
                            .lines()
                            .find_map(|line| {
                                line.split_once(':')
                                    .filter(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                                    .and_then(|(_, v)| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                    assert!(bytes.len() < 16384);
                }
                let request = String::from_utf8(bytes).unwrap();
                assert!(request.starts_with(expected), "{request}");
                requests.push(request);
                let body = reply.to_string();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
            requests
        });
        (client, worker)
    }
    fn running_catalogue() -> Value {
        json!({"profiles":[{"id":"a","name":"main","is_running":true,"process_id":12,"last_launch":99}]})
    }
    fn grok_pages(urls: &[&str]) -> Value {
        json!({"profileId":"a","browserPid":12,"launchGeneration":99,"pages":urls.iter().map(|url|json!({"url":url})).collect::<Vec<_>>()})
    }
    #[test]
    fn production_reuses_existing_grok_without_run_or_page_creation() {
        let mut steps = vec![];
        for _ in 0..2 {
            steps.extend([
                ("GET /v1/local/browser/profiles ", running_catalogue()),
                ("GET /v1/local/browser/profiles ", running_catalogue()),
                (
                    "GET /v1/local/browser/profiles/a/pages ",
                    grok_pages(&["https://grok.com/imagine?existing=1"]),
                ),
            ]);
        }
        let (client, worker) = grok_fixture(steps);
        client.ensure_production_grok(&selected()).unwrap();
        client.ensure_production_grok(&selected()).unwrap();
        assert!(worker.join().unwrap().iter().all(|r| r.starts_with("GET ")));
    }
    #[test]
    fn production_opens_missing_grok_once_without_navigating_other_tabs() {
        let (client, worker) = grok_fixture(vec![
            ("GET /v1/local/browser/profiles ", running_catalogue()),
            ("GET /v1/local/browser/profiles ", running_catalogue()),
            (
                "GET /v1/local/browser/profiles/a/pages ",
                grok_pages(&["https://example.com", "https://grok.com.evil.test"]),
            ),
            ("GET /v1/local/browser/profiles ", running_catalogue()),
            (
                "POST /v1/local/browser/profiles/a/pages ",
                json!({"page":{"targetId":"grok-one","url":"https://grok.com/imagine","browserPid":12,"launchGeneration":99}}),
            ),
        ]);
        client.ensure_production_grok(&selected()).unwrap();
        let requests = worker.join().unwrap();
        assert_eq!(
            requests.iter().filter(|r| r.starts_with("POST ")).count(),
            1
        );
        assert!(
            requests
                .last()
                .unwrap()
                .contains("https://grok.com/imagine")
        );
    }
    #[test]
    fn production_starts_stopped_profile_and_reuses_its_created_grok_tab() {
        let mut stopped = selected();
        stopped.running = false;
        stopped.pid = None;
        stopped.generation = None;
        let catalogue = json!({"profiles":[{"id":"a","name":"main","is_running":false,"process_id":null,"last_launch":null}]});
        let (client, worker) = grok_fixture(vec![
            ("GET /v1/local/browser/profiles ", catalogue.clone()),
            ("GET /v1/local/browser/profiles ", catalogue),
            (
                "POST /v1/local/browser/profiles/a/run ",
                json!({"profileId":"a","browserPid":12,"launchGeneration":99}),
            ),
            ("GET /v1/local/browser/profiles ", running_catalogue()),
            (
                "GET /v1/local/browser/profiles/a/pages ",
                grok_pages(&["https://grok.com/"]),
            ),
        ]);
        client.ensure_production_grok(&stopped).unwrap();
        assert_eq!(
            worker
                .join()
                .unwrap()
                .iter()
                .filter(|r| r.starts_with("POST "))
                .count(),
            1
        );
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
