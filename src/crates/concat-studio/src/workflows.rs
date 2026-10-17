// SPDX-License-Identifier: AGPL-3.0-or-later

//! Read-only Promax workflow adapter. Credentials never enter UI models.

use std::io::Read;
use std::time::Duration;

/// Safe failure categories; never include upstream bodies or credentials.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkflowError {
    /// Invalid endpoint, or unsafe token characters.
    Configuration,
    /// Missing/expired access token.
    Authentication,
    /// Authenticated principal lacks access.
    Forbidden,
    /// Provider requests a slower request rate.
    RateLimited,
    /// Unavailable service, timeout or unexpected redirect.
    Unavailable,
    /// Invalid or oversized response.
    InvalidResponse,
}

/// A workflow definition returned by the tenant-scoped Promax API.
#[derive(Clone, Debug)]
pub struct WorkflowDefinition {
    /// Stable backend ID, not a URL.
    pub workflow_id: String,
    /// Display name.
    pub name: String,
    /// Published definition version.
    pub version: String,
    /// Optional plain-text description.
    pub description: String,
    /// Nodes used only for counts in the first native slice.
    pub node_count: usize,
    /// Edges used only for counts in the first native slice.
    pub edge_count: usize,
}

/// A real server run, never a locally fabricated progress record.
#[derive(Clone, Debug, PartialEq)]
#[allow(missing_docs)]
pub struct RunSummary {
    pub id: String,
    pub workflow_id: String,
    pub status: String,
    pub node: String,
    pub created_at: String,
    pub completed_steps: usize,
    pub total_steps: usize,
}

/// A server-side preflight result; supported does not imply a connected worker.
#[derive(Clone, Debug, PartialEq)]
#[allow(missing_docs)]
pub struct Preflight {
    pub can_start: bool,
    pub requires_worker: bool,
    pub worker_available: bool,
}

/// State-changing calls distinguish an explicit rejection from an unknown outcome.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RunError {
    /// Invalid local input, no request was sent.
    Input,
    /// Auth, permission or limit response; not a successful mutation.
    Remote(WorkflowError),
    /// Server preflight did not allow starting this workflow.
    NotReady,
    /// Request may have been accepted. Never automatically retry it.
    UnknownOutcome,
}

/// Immutable, validated inputs shared by preflight and execution.
#[derive(Clone, PartialEq)]
#[allow(missing_docs)]
pub struct ProductionPlan {
    pub workflow: String,
    pub inputs: serde_json::Value,
}

impl ProductionPlan {
    /// Translate the native brief form into graph inputs without allowing extra
    /// JSON to override visible fields or server authorization metadata.
    pub fn new(
        workflow: String,
        brief: &str,
        sources: &str,
        scenes: f32,
        duration: f32,
        extra: &str,
    ) -> Result<Self, RunError> {
        workflow_path(&workflow)?;
        if brief.trim().is_empty()
            || brief.len() > 32768
            || sources.len() > 16384
            || extra.len() > 32768
            || !scenes.is_finite()
            || !(1.0..=30.0).contains(&scenes)
            || !duration.is_finite()
            || !(10.0..=7200.0).contains(&duration)
        {
            return Err(RunError::Input);
        }
        let extra: serde_json::Value = serde_json::from_str(extra).map_err(|_| RunError::Input)?;
        let extra = extra.as_object().ok_or(RunError::Input)?;
        let urls: Vec<&str> = sources
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if urls.len() > 30
            || urls.iter().any(|s| {
                url::Url::parse(s).map_or(true, |url| {
                    !matches!(url.scheme(), "http" | "https")
                        || url.host().is_none()
                        || !url.username().is_empty()
                        || url.password().is_some()
                })
            })
        {
            return Err(RunError::Input);
        }
        let mut inputs = serde_json::json!({"brief": brief, "source_urls":urls, "scene_count":scenes as u32, "duration_seconds":duration as u32});
        for (key, value) in extra {
            if inputs.get(key).is_some()
                || ["tenantId", "ownerId", "schedulerAuthority"].contains(&key.as_str())
            {
                return Err(RunError::Input);
            }
            inputs
                .as_object_mut()
                .ok_or(RunError::Input)?
                .insert(key.clone(), value.clone());
        }
        validate_inputs(&inputs)?;
        Ok(Self { workflow, inputs })
    }
}

/// Client restricted to two read-only routes and a validated origin.
pub struct WorkflowClient {
    origin: String,
    token: String,
    local_ai: bool,
}

impl WorkflowClient {
    /// Non-secret endpoint for native connection diagnostics.
    pub fn endpoint_origin(&self) -> &str {
        &self.origin
    }
    /// Resolve a single YouTube source through Promax without downloading media.
    pub fn source_title(&self, source: &str) -> Result<String, WorkflowError> {
        let id = youtube_id(source).ok_or(WorkflowError::Configuration)?;
        let data = self.write_with_timeout(
            "POST",
            "/api/v1/artcraft/media/source/resolve",
            &serde_json::json!({"url":source}),
            Duration::from_secs(95),
        )?;
        if data.get("id").and_then(|v| v.as_str()) != Some(id.as_str())
            || data.get("url").and_then(|v| v.as_str())
                != Some(format!("https://www.youtube.com/watch?v={id}").as_str())
        {
            return Err(WorkflowError::InvalidResponse);
        }
        data.get("title")
            .and_then(|v| v.as_str())
            .filter(|title| !title.trim().is_empty() && title.len() <= 8000)
            .map(str::to_owned)
            .ok_or(WorkflowError::InvalidResponse)
    }
    /// List the latest server runs owned by the authenticated principal.
    pub fn runs(&self) -> Result<Vec<RunSummary>, WorkflowError> {
        let data = self.get("/api/v1/artcraft/workflow-runs?limit=100&offset=0")?;
        let list = data
            .get("runs")
            .and_then(|v| v.as_array())
            .filter(|v| v.len() <= 100)
            .ok_or(WorkflowError::InvalidResponse)?;
        list.iter().map(parse_run).collect()
    }

    /// Preflight exactly these inputs; does not start an execution.
    pub fn preflight(
        &self,
        workflow_id: &str,
        inputs: &serde_json::Value,
    ) -> Result<Preflight, RunError> {
        let path = workflow_path(workflow_id)?;
        validate_inputs(inputs)?;
        let data = self
            .post(
                &format!("/api/v1/artcraft/workflows/{path}/validate"),
                &serde_json::json!({"inputs": inputs}),
            )
            .map_err(RunError::Remote)?;
        let boolean = |key| {
            data.get(key)
                .and_then(|v| v.as_bool())
                .ok_or(RunError::Remote(WorkflowError::InvalidResponse))
        };
        if boolean("supported")? != true {
            return Err(RunError::NotReady);
        }
        Ok(Preflight {
            can_start: boolean("canStart")?,
            requires_worker: boolean("requiresLocalWorker")?,
            worker_available: boolean("workerAvailable")?,
        })
    }

    /// Explicitly start one execution after a fresh server preflight.
    /// There are no retries. An ambiguous response must be reconciled with runs.
    pub fn start(
        &self,
        workflow_id: &str,
        inputs: &serde_json::Value,
    ) -> Result<RunSummary, RunError> {
        let preflight = self.preflight(workflow_id, inputs)?;
        if !preflight.can_start || (preflight.requires_worker && !preflight.worker_available) {
            return Err(RunError::NotReady);
        }
        let path = workflow_path(workflow_id)?;
        let data = self
            .post(
                &format!("/api/v1/artcraft/workflows/{path}/runs"),
                &serde_json::json!({"inputs": inputs, "schedulerAuthority":"promax"}),
            )
            .map_err(mutation_error)?;
        parse_run(&data).map_err(|_| RunError::UnknownOutcome)
    }

    /// Request cancellation of one real run; server still owns the transition.
    pub fn cancel_run(&self, run_id: &str) -> Result<RunSummary, RunError> {
        let path = workflow_path(run_id)?;
        let data = self
            .post(
                &format!("/api/v1/artcraft/workflow-runs/{path}/cancel"),
                &serde_json::json!({"reason":"Cancelled from Concat Studio"}),
            )
            .map_err(mutation_error)?;
        parse_run(&data).map_err(|_| RunError::UnknownOutcome)
    }

    /// Resume one waiting workflow run after an explicit user action.
    /// An ambiguous response is never retried automatically.
    pub fn resume_run(&self, run_id: &str) -> Result<RunSummary, RunError> {
        let path = workflow_path(run_id)?;
        let data = self
            .post(
                &format!("/api/v1/artcraft/workflow-runs/{path}/resume"),
                &serde_json::json!({}),
            )
            .map_err(mutation_error)?;
        let resumed = parse_run(&data).map_err(|_| RunError::UnknownOutcome)?;
        if resumed.id != run_id {
            return Err(RunError::UnknownOutcome);
        }
        Ok(resumed)
    }

    pub(crate) fn post(
        &self,
        route: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, WorkflowError> {
        self.write("POST", route, body)
    }

    pub(crate) fn write(
        &self,
        method: &str,
        route: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, WorkflowError> {
        self.write_with_timeout(method, route, body, Duration::from_secs(15))
    }

    pub(crate) fn write_with_timeout(
        &self,
        method: &str,
        route: &str,
        body: &serde_json::Value,
        timeout: Duration,
    ) -> Result<serde_json::Value, WorkflowError> {
        if self.local_ai {
            return crate::local_ai::request(
                &self.origin,
                &self.token,
                method,
                route,
                body,
                timeout,
            );
        }
        let agent = ureq::AgentBuilder::new()
            .timeout(timeout)
            .redirects(0)
            .build();
        let encoded = serde_json::to_vec(body).map_err(|_| WorkflowError::Configuration)?;
        let response = agent
            .request(method, &format!("{}{route}", self.origin))
            .set("Accept", "application/json")
            .set("Content-Type", "application/json")
            .set("Authorization", &format!("Bearer {}", self.token))
            .send_bytes(&encoded);
        let response = match response {
            Ok(response) if matches!(response.status(), 200 | 201) => response,
            Err(ureq::Error::Status(401, _)) => return Err(WorkflowError::Authentication),
            Err(ureq::Error::Status(403, _)) => return Err(WorkflowError::Forbidden),
            Err(ureq::Error::Status(429, _)) => return Err(WorkflowError::RateLimited),
            _ => return Err(WorkflowError::Unavailable),
        };
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| WorkflowError::InvalidResponse)?;
        if bytes.len() > 1024 * 1024 {
            return Err(WorkflowError::InvalidResponse);
        }
        let mut value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| WorkflowError::InvalidResponse)?;
        if value.get("ok").and_then(|v| v.as_bool()) != Some(true) {
            return Err(WorkflowError::InvalidResponse);
        }
        value
            .get_mut("data")
            .map(serde_json::Value::take)
            .ok_or(WorkflowError::InvalidResponse)
    }
    /// Configure an explicit Promax origin. HTTP is allowed only on loopback.
    /// Tokens are held only in process memory and must not be logged.
    pub fn new(origin: &str, token: Option<String>) -> Result<Self, WorkflowError> {
        let parsed = url::Url::parse(origin.trim()).map_err(|_| WorkflowError::Configuration)?;
        let local = match parsed.host() {
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            _ => false,
        };
        if parsed.host().is_none()
            || !(parsed.scheme() == "https" || (parsed.scheme() == "http" && local))
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || parsed.path() != "/"
            || parsed.port() == Some(0)
        {
            return Err(WorkflowError::Configuration);
        }
        let token = token
            .filter(|value| !value.is_empty())
            .ok_or(WorkflowError::Authentication)?;
        if token.len() > 8192
            || !token
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-._~+/=".contains(&byte))
        {
            return Err(WorkflowError::Configuration);
        }
        Ok(Self {
            origin: parsed.as_str().trim_end_matches('/').to_owned(),
            token,
            local_ai: false,
        })
    }

    /// An owned loopback gateway session. It cannot call cloud/workflow routes.
    pub(crate) fn local_ai(origin: &str, cookie: String) -> Result<Self, WorkflowError> {
        let mut client = Self::new(origin, Some(cookie))?;
        if !client.origin.starts_with("http://127.0.0.1:") {
            return Err(WorkflowError::Configuration);
        }
        client.local_ai = true;
        Ok(client)
    }

    /// Read definitions visible to the current principal; never starts a run.
    pub fn definitions(&self) -> Result<Vec<WorkflowDefinition>, WorkflowError> {
        let value = self.get("/api/v1/artcraft/workflows/definitions")?;
        let list = value
            .as_array()
            .filter(|list| list.len() <= 200)
            .ok_or(WorkflowError::InvalidResponse)?;
        list.iter()
            .map(|item| {
                let text = |key: &str, limit: usize| -> Result<String, WorkflowError> {
                    let value = item
                        .get(key)
                        .and_then(|value| value.as_str())
                        .ok_or(WorkflowError::InvalidResponse)?;
                    if value.is_empty()
                        || value.len() > limit
                        || value.chars().any(char::is_control)
                    {
                        return Err(WorkflowError::InvalidResponse);
                    }
                    Ok(value.to_owned())
                };
                let nodes = item
                    .get("nodes")
                    .and_then(|value| value.as_array())
                    .filter(|nodes| nodes.len() <= 1024)
                    .ok_or(WorkflowError::InvalidResponse)?;
                let edges = item
                    .get("edges")
                    .and_then(|value| value.as_array())
                    .filter(|edges| edges.len() <= 4096)
                    .ok_or(WorkflowError::InvalidResponse)?;
                let description = match item.get("description") {
                    None | Some(serde_json::Value::Null) => String::new(),
                    Some(serde_json::Value::String(value)) if value.len() <= 4096 => value.clone(),
                    _ => return Err(WorkflowError::InvalidResponse),
                };
                Ok(WorkflowDefinition {
                    workflow_id: text("workflowId", 256)?,
                    name: text("name", 512)?,
                    version: text("version", 128)?,
                    description,
                    node_count: nodes.len(),
                    edge_count: edges.len(),
                })
            })
            .collect()
    }

    pub(crate) fn get(&self, route: &str) -> Result<serde_json::Value, WorkflowError> {
        self.get_with_timeout(route, Duration::from_secs(8))
    }

    fn get_with_timeout(
        &self,
        route: &str,
        timeout: Duration,
    ) -> Result<serde_json::Value, WorkflowError> {
        if self.local_ai {
            return crate::local_ai::request(
                &self.origin,
                &self.token,
                "GET",
                route,
                &serde_json::Value::Null,
                timeout,
            );
        }
        let agent = ureq::AgentBuilder::new()
            .timeout(timeout)
            .redirects(0)
            .build();
        let response = match agent
            .get(&format!("{}{route}", self.origin))
            .set("Accept", "application/json")
            .set("Authorization", &format!("Bearer {}", self.token))
            .call()
        {
            Ok(response) => response,
            Err(ureq::Error::Status(401, _)) => return Err(WorkflowError::Authentication),
            Err(ureq::Error::Status(403, _)) => return Err(WorkflowError::Forbidden),
            Err(ureq::Error::Status(429, _)) => return Err(WorkflowError::RateLimited),
            Err(_) => return Err(WorkflowError::Unavailable),
        };
        if response.status() != 200 {
            return Err(WorkflowError::Unavailable);
        }
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| WorkflowError::InvalidResponse)?;
        if bytes.len() > 1024 * 1024 {
            return Err(WorkflowError::InvalidResponse);
        }
        let mut envelope: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| WorkflowError::InvalidResponse)?;
        if envelope.get("ok").and_then(|value| value.as_bool()) != Some(true) {
            return Err(WorkflowError::InvalidResponse);
        }
        envelope
            .get_mut("data")
            .map(serde_json::Value::take)
            .ok_or(WorkflowError::InvalidResponse)
    }
}

fn youtube_id(raw: &str) -> Option<String> {
    if raw.len() > 2048 || raw.chars().any(char::is_control) {
        return None;
    }
    let url = url::Url::parse(raw).ok()?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return None;
    }
    let id = match url.host_str()? {
        "youtu.be" => url.path().strip_prefix('/')?.to_owned(),
        "youtube.com" | "www.youtube.com" | "m.youtube.com" => {
            if url.path() == "/watch" {
                url.query_pairs()
                    .find(|(key, _)| key == "v")?
                    .1
                    .into_owned()
            } else {
                url.path()
                    .strip_prefix("/shorts/")
                    .or_else(|| url.path().strip_prefix("/embed/"))?
                    .to_owned()
            }
        }
        _ => return None,
    };
    (id.len() == 11
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'))
    .then_some(id)
}

fn mutation_error(error: WorkflowError) -> RunError {
    match error {
        WorkflowError::Authentication | WorkflowError::Forbidden | WorkflowError::RateLimited => {
            RunError::Remote(error)
        }
        _ => RunError::UnknownOutcome,
    }
}

fn workflow_path(id: &str) -> Result<String, RunError> {
    if id.is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
        return Err(RunError::Input);
    }
    let mut url = url::Url::parse("https://unused.invalid/").map_err(|_| RunError::Input)?;
    // One path segment: a user-controlled ID cannot inject a route or query.
    url.path_segments_mut()
        .map_err(|_| RunError::Input)?
        .push(id);
    if matches!(id, "." | "..") {
        return Err(RunError::Input);
    }
    Ok(url.path().trim_start_matches('/').into())
}

fn validate_inputs(inputs: &serde_json::Value) -> Result<(), RunError> {
    if !inputs.is_object()
        || serde_json::to_vec(inputs)
            .map_err(|_| RunError::Input)?
            .len()
            > 65536
    {
        return Err(RunError::Input);
    }
    Ok(())
}

fn parse_run(value: &serde_json::Value) -> Result<RunSummary, WorkflowError> {
    let text = |key: &str, max| -> Result<String, WorkflowError> {
        let text = value
            .get(key)
            .and_then(|v| v.as_str())
            .ok_or(WorkflowError::InvalidResponse)?;
        if text.is_empty() || text.len() > max || text.chars().any(char::is_control) {
            return Err(WorkflowError::InvalidResponse);
        }
        Ok(text.into())
    };
    let status = text("status", 32)?;
    if ![
        "pending",
        "queued",
        "running",
        "waiting",
        "completed",
        "failed",
        "cancel_requested",
        "cancelled",
    ]
    .contains(&status.as_str())
    {
        return Err(WorkflowError::InvalidResponse);
    }
    let steps = value
        .get("steps")
        .and_then(|v| v.as_array())
        .filter(|v| v.len() <= 4096)
        .ok_or(WorkflowError::InvalidResponse)?;
    let node = match value.get("currentNodeId") {
        None | Some(serde_json::Value::Null) => String::new(),
        Some(serde_json::Value::String(node))
            if node.len() <= 512 && !node.chars().any(char::is_control) =>
        {
            node.clone()
        }
        _ => return Err(WorkflowError::InvalidResponse),
    };
    Ok(RunSummary {
        id: text("runId", 256)?,
        workflow_id: text("workflowId", 256)?,
        status,
        node,
        created_at: text("createdAt", 128)?,
        completed_steps: steps
            .iter()
            .filter(|v| v.get("status").and_then(|s| s.as_str()) == Some("completed"))
            .count(),
        total_steps: steps.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn fixture(status: &str, body: &str) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture");
        let origin = format!("http://{}", listener.local_addr().expect("address"));
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("request");
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .expect("timeout");
            let mut bytes = Vec::new();
            let mut chunk = [0; 1024];
            loop {
                if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|value| value.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        break;
                    }
                }
                assert!(bytes.len() < 16384, "fixture request too large");
                let size = stream.read(&mut chunk).expect("read");
                if size == 0 {
                    break;
                }
                bytes.extend_from_slice(&chunk[..size]);
            }
            let _ = stream.write_all(response.as_bytes());
            String::from_utf8(bytes).expect("request UTF8")
        });
        (origin, worker)
    }

    fn client(origin: &str) -> WorkflowClient {
        WorkflowClient::new(origin, Some("synthetic-access-token".into())).expect("safe origin")
    }

    #[test]
    fn source_metadata_uses_authenticated_promax_route() {
        let body = serde_json::json!({"ok":true,"data":{"id":"abcdefghijk","title":"Nguồn mẫu","url":"https://www.youtube.com/watch?v=abcdefghijk"}}).to_string();
        let (origin, server) = fixture("200 OK", &body);
        assert_eq!(
            client(&origin)
                .source_title("https://youtu.be/abcdefghijk")
                .unwrap(),
            "Nguồn mẫu"
        );
        let request = server.join().unwrap();
        assert!(request.starts_with("POST /api/v1/artcraft/media/source/resolve "));
        assert!(request.contains("synthetic-access-token"));
        assert!(request.contains("https://youtu.be/abcdefghijk"));
    }

    #[test]
    fn source_metadata_rejects_unsafe_urls_without_network_and_malformed_responses() {
        let offline = client("http://127.0.0.1:1");
        for url in [
            "file:///D:/private",
            "http://127.0.0.1",
            "https://youtube.com.attacker.test/watch?v=abcdefghijk",
            "https://user:password@youtube.com/watch?v=abcdefghijk",
        ] {
            assert_eq!(offline.source_title(url), Err(WorkflowError::Configuration));
        }
        let (origin, server) = fixture(
            "200 OK",
            r#"{"ok":true,"data":{"id":"bad","title":"Wrong"}}"#,
        );
        assert_eq!(
            client(&origin).source_title("https://youtu.be/abcdefghijk"),
            Err(WorkflowError::InvalidResponse)
        );
        server.join().unwrap();
    }

    fn run_fixture() -> serde_json::Value {
        serde_json::json!({"runId":"run-1","workflowId":"movie","status":"queued","createdAt":"2026-10-13T00:00:00Z","steps":[{"status":"completed"},{"status":"pending"}]})
    }

    #[test]
    fn reads_run_page_without_inventing_progress() {
        let body = serde_json::json!({"ok":true,"data":{"runs":[run_fixture()],"total":1,"limit":100,"offset":0}}).to_string();
        let (origin, server) = fixture("200 OK", &body);
        let rows = client(&origin).runs().unwrap();
        assert_eq!(rows[0].completed_steps, 1);
        assert_eq!(rows[0].total_steps, 2);
        assert_eq!(rows[0].status, "queued");
        assert!(
            server
                .join()
                .unwrap()
                .starts_with("GET /api/v1/artcraft/workflow-runs?limit=100&offset=0 ")
        );
    }

    #[test]
    fn production_form_is_validated_without_any_engine_or_network() {
        let plan = ProductionPlan::new(
            "movie".into(),
            "A short story",
            "https://example.com/source",
            6.0,
            60.0,
            r#"{"style":"documentary"}"#,
        )
        .unwrap();
        assert_eq!(plan.inputs["scene_count"], 6);
        assert_eq!(plan.inputs["source_urls"][0], "https://example.com/source");
        assert_eq!(plan.inputs["style"], "documentary");
        for extra in [
            r#"{"brief":"hidden override"}"#,
            r#"{"ownerId":"other-user"}"#,
            "[]",
            "invalid",
        ] {
            assert!(ProductionPlan::new("movie".into(), "Story", "", 6.0, 60.0, extra).is_err());
        }
        for source in [
            "file:///private/file",
            "https://user:password@example.com",
            "not a URL",
        ] {
            assert!(ProductionPlan::new("movie".into(), "Story", source, 6.0, 60.0, "{}").is_err());
        }
        assert!(ProductionPlan::new("movie".into(), "Story", "", f32::NAN, 60.0, "{}").is_err());
        assert!(ProductionPlan::new("movie".into(), "Story", "", 6.0, 7201.0, "{}").is_err());
    }

    #[test]
    fn rejects_invalid_run_status_and_inputs_before_network() {
        let mut run = run_fixture();
        run["status"] = "pretend-success".into();
        assert!(parse_run(&run).is_err());
        assert!(validate_inputs(&serde_json::json!([])).is_err());
        assert!(validate_inputs(&serde_json::json!({"huge":"x".repeat(65537)})).is_err());
        assert!(workflow_path("..").is_err());
        let escaped = workflow_path("name/../runs?injected=true").unwrap();
        assert!(!escaped.contains('/'));
        assert!(!escaped.contains('?'));
    }

    #[test]
    fn preflight_reports_missing_worker_without_starting() {
        let (origin, server) = fixture(
            "200 OK",
            r#"{"ok":true,"data":{"supported":true,"canStart":false,"requiresLocalWorker":true,"workerAvailable":false}}"#,
        );
        assert_eq!(
            client(&origin).start("movie", &serde_json::json!({})),
            Err(RunError::NotReady)
        );
        assert!(
            server
                .join()
                .unwrap()
                .starts_with("POST /api/v1/artcraft/workflows/movie/validate ")
        );
    }

    #[test]
    fn ambiguous_mutations_are_never_reported_as_rejected() {
        assert_eq!(
            mutation_error(WorkflowError::Unavailable),
            RunError::UnknownOutcome
        );
        assert_eq!(
            mutation_error(WorkflowError::InvalidResponse),
            RunError::UnknownOutcome
        );
        assert_eq!(
            mutation_error(WorkflowError::Authentication),
            RunError::Remote(WorkflowError::Authentication)
        );
        let (origin, server) = fixture("200 OK", "not-json");
        assert_eq!(
            client(&origin).cancel_run("run-1"),
            Err(RunError::UnknownOutcome)
        );
        assert!(
            server
                .join()
                .unwrap()
                .starts_with("POST /api/v1/artcraft/workflow-runs/run-1/cancel ")
        );
    }

    #[test]
    fn resumes_a_waiting_run_only_after_the_explicit_request() {
        let mut run = run_fixture();
        run["status"] = "running".into();
        let body = serde_json::json!({"ok":true,"data":run}).to_string();
        let (origin, server) = fixture("200 OK", &body);
        let resumed = client(&origin).resume_run("run-1").expect("resume receipt");
        assert_eq!(resumed.status, "running");
        let request = server.join().expect("fixture");
        assert!(request.starts_with("POST /api/v1/artcraft/workflow-runs/run-1/resume HTTP/1.1"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer synthetic-access-token")
        );
    }

    #[test]
    fn resume_rejects_a_receipt_for_a_different_run() {
        let mut run = run_fixture();
        run["runId"] = "run-other".into();
        let body = serde_json::json!({"ok":true,"data":run}).to_string();
        let (origin, server) = fixture("200 OK", &body);
        assert_eq!(
            client(&origin).resume_run("run-1"),
            Err(RunError::UnknownOutcome)
        );
        server.join().expect("fixture");
    }

    #[test]
    fn start_preflights_the_same_inputs_and_submits_exactly_once() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let responses = [
                serde_json::json!({"ok":true,"data":{"supported":true,"canStart":true,"requiresLocalWorker":false,"workerAvailable":true}}),
                serde_json::json!({"ok":true,"data":run_fixture()}),
            ];
            let mut requests = Vec::new();
            for (index, response) in responses.iter().enumerate() {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0u8; 1024];
                let header_end = loop {
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&buffer[..count]);
                    if let Some(end) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                        break end + 4;
                    }
                    assert!(bytes.len() < 16384);
                };
                let header = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
                let length: usize = header
                    .lines()
                    .find_map(|line| {
                        line.to_lowercase()
                            .strip_prefix("content-length:")
                            .map(|n| n.trim().parse().unwrap())
                    })
                    .unwrap();
                while bytes.len() < header_end + length {
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&buffer[..count]);
                }
                requests.push((
                    header,
                    serde_json::from_slice::<serde_json::Value>(
                        &bytes[header_end..header_end + length],
                    )
                    .unwrap(),
                ));
                let body = response.to_string();
                write!(
                    stream,
                    "HTTP/1.1 {} OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    if index == 0 { 200 } else { 201 },
                    body.len(),
                    body
                )
                .unwrap();
            }
            requests
        });
        let inputs = serde_json::json!({"brief":"fixture only"});
        assert_eq!(client(&origin).start("movie", &inputs).unwrap().id, "run-1");
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(
            requests[0]
                .0
                .starts_with("POST /api/v1/artcraft/workflows/movie/validate ")
        );
        assert!(
            requests[1]
                .0
                .starts_with("POST /api/v1/artcraft/workflows/movie/runs ")
        );
        assert_eq!(requests[0].1["inputs"], inputs);
        assert_eq!(requests[1].1["inputs"], inputs);
        assert_eq!(requests[1].1["schedulerAuthority"], "promax");
        assert!(requests[1].1.get("ownerId").is_none());
    }

    #[test]
    fn validates_origin_and_credentials_before_any_network_access() {
        for origin in [
            "http://example.com",
            "https://user:secret@example.com",
            "https://example.com/?key=secret",
            "https://example.com/#secret",
            "file:///tmp/x",
            "https://example.com/path",
        ] {
            assert!(matches!(
                WorkflowClient::new(origin, Some("test".into())),
                Err(WorkflowError::Configuration)
            ));
        }
        assert!(matches!(
            WorkflowClient::new("https://example.com", None),
            Err(WorkflowError::Authentication)
        ));
        assert!(matches!(
            WorkflowClient::new("https://example.com", Some("x\r\nInjected: true".into())),
            Err(WorkflowError::Configuration)
        ));
        assert!(WorkflowClient::new("https://example.com/", Some("test".into())).is_ok());
    }

    #[test]
    fn reads_real_envelope_using_get_and_bearer_header() {
        let (origin, worker) = fixture(
            "200 OK",
            r#"{"ok":true,"data":[{"workflowId":"wf-1","name":"Script","version":"1","nodes":[],"edges":[]}]}"#,
        );
        let definitions = client(&origin).definitions().expect("definitions");
        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].workflow_id, "wf-1");
        let request = worker.join().expect("fixture");
        assert!(request.starts_with("GET /api/v1/artcraft/workflows/definitions HTTP/1.1"));
        assert!(
            request
                .to_lowercase()
                .contains("authorization: bearer synthetic-access-token")
        );
    }

    #[test]
    fn maps_auth_rate_limit_and_service_errors_without_disclosing_body() {
        for (status, expected) in [
            ("401 Unauthorized", WorkflowError::Authentication),
            ("403 Forbidden", WorkflowError::Forbidden),
            ("429 Too Many Requests", WorkflowError::RateLimited),
            ("503 Unavailable", WorkflowError::Unavailable),
            ("302 Found", WorkflowError::Unavailable),
        ] {
            let (origin, worker) = fixture(status, "private upstream detail");
            assert!(matches!(client(&origin).definitions(), Err(error) if error == expected));
            worker.join().expect("fixture");
        }
    }

    #[test]
    fn distinguishes_empty_list_from_malformed_or_unsuccessful_payload() {
        let (origin, worker) = fixture("200 OK", r#"{"ok":true,"data":[]}"#);
        assert!(
            client(&origin)
                .definitions()
                .expect("empty list")
                .is_empty()
        );
        worker.join().expect("fixture");
        for body in [
            "<html>login</html>",
            r#"{"ok":false,"data":[]}"#,
            "[]",
            r#"{"ok":true,"data":[{"name":"incomplete"}]}"#,
        ] {
            let (origin, worker) = fixture("200 OK", body);
            assert!(matches!(
                client(&origin).definitions(),
                Err(WorkflowError::InvalidResponse)
            ));
            worker.join().expect("fixture");
        }
    }

    #[test]
    fn rejects_oversize_response() {
        let (origin, worker) = fixture("200 OK", &"x".repeat(1024 * 1024 + 1));
        assert!(matches!(
            client(&origin).definitions(),
            Err(WorkflowError::InvalidResponse)
        ));
        worker.join().expect("fixture");
    }

    #[test]
    fn times_out_without_hanging_on_a_silent_service() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let origin = format!("http://{}", listener.local_addr().expect("address"));
        let (release, held) = std::sync::mpsc::channel::<()>();
        let worker = std::thread::spawn(move || {
            let (_stream, _) = listener.accept().expect("request");
            let _ = held.recv_timeout(std::time::Duration::from_secs(2));
        });
        assert!(matches!(
            client(&origin).get_with_timeout(
                "/api/v1/artcraft/workflows/definitions",
                Duration::from_millis(40)
            ),
            Err(WorkflowError::Unavailable)
        ));
        let _ = release.send(());
        worker.join().expect("fixture");
    }

    #[test]
    fn never_follows_a_redirect_with_the_access_token() {
        let target = TcpListener::bind("127.0.0.1:0").expect("redirect target");
        target.set_nonblocking(true).expect("nonblocking");
        let status = format!(
            "302 Found\r\nLocation: http://{}/stolen",
            target.local_addr().expect("address")
        );
        let (origin, worker) = fixture(&status, "");
        assert!(matches!(
            client(&origin).definitions(),
            Err(WorkflowError::Unavailable)
        ));
        worker.join().expect("fixture");
        assert!(
            matches!(target.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        );
    }

    #[test]
    fn rejects_unbounded_names_and_graphs() {
        for body in [
            serde_json::json!({"ok": true, "data": [{"workflowId":"wf", "name":"x".repeat(513), "version":"1", "nodes":[], "edges":[]}]}),
            serde_json::json!({"ok": true, "data": [{"workflowId":"wf", "name":"name", "version":"1", "nodes":vec![serde_json::json!({});1025], "edges":[]}]}),
            serde_json::json!({"ok": true, "data": [{"workflowId":"wf", "name":"name", "version":"1", "description":42, "nodes":[], "edges":[]}]}),
        ] {
            let (origin, worker) = fixture("200 OK", &body.to_string());
            assert!(matches!(
                client(&origin).definitions(),
                Err(WorkflowError::InvalidResponse)
            ));
            worker.join().expect("fixture");
        }
    }
}
