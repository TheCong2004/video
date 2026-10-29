// SPDX-License-Identifier: AGPL-3.0-or-later
//! Safe, opt-in adapters and contracts for Kedi Live phases P2-P6.
//!
//! Platform writes and non-loopback OpenTalking access are intentionally not
//! implemented here. These primitives make the local/mock path testable while
//! keeping external side effects behind explicit capabilities and approvals.
#![allow(missing_docs)] // Contract fields are described in docs/kedi-live-audit/P2-P6_IMPLEMENTATION_STATUS.md.

use crate::live::{LiveFrame, LiveFrameKind};
use crate::live_brain::{LiveTask, ProductSnapshot};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, fs, io::Read, net::IpAddr, path::Path, time::Duration};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterError {
    Disabled,
    InvalidConfig,
    InvalidInput,
    Unavailable,
    UnknownOutcome,
    InvalidResponse,
    Unauthorized,
    Stale,
    Capacity,
}

/// Minimal OpenTalking session adapter. Requires explicit opt-in and numeric loopback HTTP.
/// The endpoint is the audited fork's `/sessions/{id}/speak` and `/interrupt` API.
pub struct OpenTalkingLiveAdapter {
    origin: String,
    agent: ureq::Agent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeechReceipt {
    pub session_id: String,
    pub command_id: String,
    pub status: String,
}

/// Correlated playback states. HTTP submission cannot advance beyond SUBMITTED.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeechState {
    Planned,
    Submitted,
    Started,
    Finished,
    Interrupted,
    Failed,
}

/// Rejects late event generations and enforces monotonic speech lifecycle updates.
#[derive(Debug, Clone)]
pub struct SpeechLifecycle {
    pub generation_id: u64,
    pub state: SpeechState,
    pub completion_verified: bool,
}
impl SpeechLifecycle {
    /// Starts a lifecycle for one planned speech generation.
    pub fn planned(generation_id: u64) -> Self {
        Self {
            generation_id,
            state: SpeechState::Planned,
            completion_verified: false,
        }
    }
    /// Applies a trusted event only if it belongs to the active generation.
    pub fn apply(&mut self, generation_id: u64, event: &str) -> bool {
        if generation_id != self.generation_id {
            return false;
        }
        let next = match (self.state, event) {
            (SpeechState::Planned, "queued") => SpeechState::Submitted,
            (SpeechState::Submitted, "started") => SpeechState::Started,
            (SpeechState::Started, "finished") => SpeechState::Finished,
            (SpeechState::Submitted | SpeechState::Started, "interrupted") => {
                SpeechState::Interrupted
            }
            (_, "failed") => SpeechState::Failed,
            _ => return false,
        };
        self.state = next;
        if next == SpeechState::Finished {
            self.completion_verified = true;
        }
        true
    }
}

impl OpenTalkingLiveAdapter {
    /// Audited fork `speak` can run its own LLM. It is not a literal commerce TTS ingress.
    pub fn supports_literal_speech(&self) -> bool {
        false
    }
    pub fn new(origin: &str, enabled: bool) -> Result<Self, AdapterError> {
        if !enabled {
            return Err(AdapterError::Disabled);
        }
        let parsed = url::Url::parse(origin.trim()).map_err(|_| AdapterError::InvalidConfig)?;
        let loopback = parsed
            .host_str()
            .and_then(|h| h.parse::<IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback());
        if parsed.scheme() != "http"
            || !loopback
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || parsed.path() != "/"
        {
            return Err(AdapterError::InvalidConfig);
        }
        Ok(Self {
            origin: parsed.as_str().trim_end_matches('/').to_owned(),
            agent: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(2))
                .timeout_read(Duration::from_secs(5))
                .timeout_write(Duration::from_secs(5))
                .redirects(0)
                .build(),
        })
    }

    pub fn submit(&self, session_id: &str, task: &LiveTask) -> Result<SpeechReceipt, AdapterError> {
        validate_id(session_id, 160)?;
        if !session_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return Err(AdapterError::InvalidInput);
        }
        validate_id(&task.task_id, 123)?;
        if task.task_id.chars().any(char::is_whitespace) {
            return Err(AdapterError::InvalidInput);
        }
        if task.speech.text.trim().is_empty() || task.speech.text.chars().count() > 600 {
            return Err(AdapterError::InvalidInput);
        }
        let command_id = format!("kedi-{}", task.task_id);
        let encoded =
            url::form_urlencoded::byte_serialize(session_id.as_bytes()).collect::<String>();
        let body = json!({"text":task.speech.text,"mode":"replace","command_id":command_id});
        let result = self
            .agent
            .post(&format!("{}/sessions/{}/speak", self.origin, encoded))
            .set("Content-Type", "application/json")
            .set("Idempotency-Key", &command_id)
            .send_string(&body.to_string());
        let response = match result {
            Ok(r) => r,
            Err(ureq::Error::Status(code, _)) if (400..500).contains(&code) => {
                return Err(AdapterError::InvalidInput);
            }
            Err(ureq::Error::Status(_, _)) => return Err(AdapterError::Unavailable),
            Err(_) => return Err(AdapterError::UnknownOutcome),
        };
        let value = read_bounded_json(response)?;
        if value.get("session_id").and_then(Value::as_str) != Some(session_id)
            || value.get("command_id").and_then(Value::as_str) != Some(command_id.as_str())
            || value.get("status").and_then(Value::as_str) != Some("queued")
        {
            return Err(AdapterError::InvalidResponse);
        }
        Ok(SpeechReceipt {
            session_id: session_id.to_owned(),
            command_id,
            status: "SUBMITTED".into(),
        })
    }

    /// Interrupt acceptance is queued work, not proof that old audio has drained.
    pub fn interrupt(&self, session_id: &str) -> Result<(), AdapterError> {
        validate_id(session_id, 160)?;
        if !session_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return Err(AdapterError::InvalidInput);
        }
        let encoded =
            url::form_urlencoded::byte_serialize(session_id.as_bytes()).collect::<String>();
        let result = self
            .agent
            .post(&format!("{}/sessions/{}/interrupt", self.origin, encoded))
            .set("Content-Length", "0")
            .call();
        let response = match result {
            Ok(r) => r,
            Err(ureq::Error::Status(code, _)) if (400..500).contains(&code) => {
                return Err(AdapterError::InvalidInput);
            }
            Err(ureq::Error::Status(_, _)) => return Err(AdapterError::Unavailable),
            Err(_) => return Err(AdapterError::UnknownOutcome),
        };
        let value = read_bounded_json(response)?;
        if value.get("session_id").and_then(Value::as_str) == Some(session_id)
            && value.get("status").and_then(Value::as_str) == Some("interrupted")
        {
            Ok(())
        } else {
            Err(AdapterError::InvalidResponse)
        }
    }
}

fn validate_id(id: &str, max: usize) -> Result<(), AdapterError> {
    if id.trim().is_empty() || id.len() > max || id.chars().any(char::is_control) {
        Err(AdapterError::InvalidInput)
    } else {
        Ok(())
    }
}

/// Read-only adapter for the existing validated TikTok connector event protocol.
#[derive(Debug, Clone)]
pub struct TikTokReadOnlyAdapter {
    tenant_id: String,
    store_id: String,
}
impl TikTokReadOnlyAdapter {
    /// Binds normalized comments to one configured tenant and store.
    pub fn new(tenant_id: &str, store_id: &str) -> Result<Self, AdapterError> {
        validate_id(tenant_id, 160)?;
        validate_id(store_id, 160)?;
        Ok(Self {
            tenant_id: tenant_id.into(),
            store_id: store_id.into(),
        })
    }
    /// Returns truthful read-only capability metadata; authorization remains false until separately verified.
    pub fn capabilities() -> crate::live_brain::PlatformCapabilities {
        crate::live_brain::PlatformCapabilities{schema_version:1,platform:"tiktok".into(),account_scope:"operator-selected-session".into(),read_comments:true,request_pin_product:false,send_comment:false,pin_confirmation:false,authorized:false,verified_in_staging:false,policy_notes:Some("Existing connector supports read-only event flow. No pin/message writes are exposed by this adapter.".into()),rate_limit_per_minute:None}
    }
    /// Maps only validated TikTok chat frames. Other event types do not enter the comment brain.
    pub fn normalize_comment(
        &self,
        frame: &LiveFrame,
    ) -> Result<Option<crate::live_brain::LiveEvent>, AdapterError> {
        if frame.kind != LiveFrameKind::Event {
            return Ok(None);
        }
        let v = &frame.value;
        if v.get("source").and_then(Value::as_str) != Some("tiktok")
            || v.get("eventKind").and_then(Value::as_str) != Some("chat")
        {
            return Ok(None);
        }
        let event_id = v
            .get("eventId")
            .and_then(Value::as_str)
            .ok_or(AdapterError::InvalidResponse)?;
        let session_id = v
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or(AdapterError::InvalidResponse)?;
        let received_at = v
            .get("receivedAt")
            .and_then(Value::as_str)
            .ok_or(AdapterError::InvalidResponse)?;
        let payload = v
            .get("payload")
            .and_then(Value::as_object)
            .ok_or(AdapterError::InvalidResponse)?;
        let text = payload
            .get("content")
            .and_then(Value::as_str)
            .ok_or(AdapterError::InvalidResponse)?;
        let actor = v
            .get("actor")
            .and_then(Value::as_object)
            .ok_or(AdapterError::InvalidResponse)?;
        let identity = actor
            .get("userId")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or(event_id);
        let digest =
            Sha256::digest(format!("{}:{}:{identity}", self.tenant_id, self.store_id).as_bytes());
        let viewer_ref = format!("tt-{}", hex_lower(&digest[..8]));
        let mut metadata = HashMap::new();
        metadata.insert(
            "room_event".into(),
            Value::String(event_id.chars().take(200).collect()),
        );
        Ok(Some(
            crate::live_brain::LiveEvent {
                schema_version: 1,
                event_id: event_id.into(),
                trace_id: format!(
                    "trace-{}",
                    hex_lower(&Sha256::digest(event_id.as_bytes())[..8])
                ),
                session_id: session_id.into(),
                tenant_id: self.tenant_id.clone(),
                store_id: self.store_id.clone(),
                platform: "tiktok".into(),
                event_type: "viewer.comment".into(),
                received_at: received_at.into(),
                viewer_ref,
                text: Some(text.into()),
                platform_product_id: None,
                metadata,
                source_event_id: Some(event_id.into()),
            }
            .normalize()
            .map_err(|_| AdapterError::InvalidResponse)?,
        ))
    }
}
fn read_bounded_json(response: ureq::Response) -> Result<Value, AdapterError> {
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| AdapterError::InvalidResponse)?;
    if bytes.len() > 64 * 1024 {
        return Err(AdapterError::InvalidResponse);
    }
    serde_json::from_slice(&bytes).map_err(|_| AdapterError::InvalidResponse)
}

/// Authenticated, tenant-scoped Fufuni/Pancake gateway client.
/// Remote origins require HTTPS; plain HTTP is accepted only on numeric loopback for tests.
pub struct CommerceGatewayClient {
    origin: String,
    token: String,
    agent: ureq::Agent,
}
impl CommerceGatewayClient {
    /// Best-effort analytics write; callers run this outside the media/controller thread.
    pub fn publish_interest(
        &self,
        signal: &crate::kedi_live::InterestSignal,
    ) -> Result<(), AdapterError> {
        validate_id(&signal.tenant_id, 160)?;
        validate_id(&signal.store_id, 160)?;
        let body = json!({"type":"viewer_intent","tenant_id":signal.tenant_id,"store_id":signal.store_id,"session_id":signal.session_id,"trace_id":signal.trace_id,"viewer_ref":signal.viewer_ref,"intent":signal.intent,"sku":signal.sku});
        if body.to_string().len() > 8192 {
            return Err(AdapterError::InvalidInput);
        }
        match self
            .agent
            .post(&format!("{}/v1/live/events", self.origin))
            .set("Authorization", &format!("Bearer {}", self.token))
            .set("Content-Type", "application/json")
            .send_string(&body.to_string())
        {
            Ok(_) => Ok(()),
            Err(ureq::Error::Status(code, _)) if (400..500).contains(&code) => {
                Err(AdapterError::Unauthorized)
            }
            Err(_) => Err(AdapterError::Unavailable),
        }
    }
    /// Creates a disabled-by-default client from an explicit origin and scoped token.
    pub fn new(origin: &str, token: &str, enabled: bool) -> Result<Self, AdapterError> {
        if !enabled {
            return Err(AdapterError::Disabled);
        }
        if token.trim().is_empty() || token.len() > 4096 || token.chars().any(char::is_control) {
            return Err(AdapterError::InvalidConfig);
        }
        let parsed = url::Url::parse(origin.trim()).map_err(|_| AdapterError::InvalidConfig)?;
        let loopback = parsed
            .host_str()
            .and_then(|h| h.parse::<IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback());
        if (parsed.scheme() != "https" && !(parsed.scheme() == "http" && loopback))
            || parsed.username() != ""
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || parsed.path() != "/"
        {
            return Err(AdapterError::InvalidConfig);
        }
        Ok(Self {
            origin: parsed.as_str().trim_end_matches('/').into(),
            token: token.into(),
            agent: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(2))
                .timeout_read(Duration::from_secs(4))
                .timeout_write(Duration::from_secs(4))
                .redirects(0)
                .build(),
        })
    }

    /// Fetches one authoritative snapshot; the response scope must exactly match the request.
    pub fn snapshot(
        &self,
        tenant: &str,
        store: &str,
        sku: &str,
    ) -> Result<ProductSnapshot, AdapterError> {
        validate_id(tenant, 160)?;
        validate_id(store, 160)?;
        validate_id(sku, 160)?;
        let mut url = url::Url::parse(&format!(
            "{}/v1/live/products/{}/snapshot",
            self.origin,
            url::form_urlencoded::byte_serialize(sku.as_bytes()).collect::<String>()
        ))
        .map_err(|_| AdapterError::InvalidConfig)?;
        url.query_pairs_mut()
            .append_pair("tenant_id", tenant)
            .append_pair("store_id", store);
        let response = match self
            .agent
            .get(url.as_str())
            .set("Authorization", &format!("Bearer {}", self.token))
            .set("Accept", "application/json")
            .call()
        {
            Ok(r) => r,
            Err(ureq::Error::Status(404, _)) => return Err(AdapterError::InvalidInput),
            Err(ureq::Error::Status(code, _)) if (400..500).contains(&code) => {
                return Err(AdapterError::Unauthorized);
            }
            Err(ureq::Error::Status(_, _)) => return Err(AdapterError::Unavailable),
            Err(_) => return Err(AdapterError::Unavailable),
        };
        let value = read_bounded_json(response)?;
        let snapshot: ProductSnapshot =
            serde_json::from_value(value).map_err(|_| AdapterError::InvalidResponse)?;
        let mut validator = crate::live_brain::ProductSession::default();
        validator
            .upsert(snapshot.clone())
            .map_err(|_| AdapterError::InvalidResponse)?;
        if snapshot.tenant_id != tenant
            || snapshot.store_id != store
            || snapshot.sku != sku
            || snapshot.verification_source == "mock"
        {
            return Err(AdapterError::Unauthorized);
        }
        if !matches!(
            snapshot.verification_source.as_str(),
            "fufuni" | "pancake_gateway"
        ) || snapshot.currency != "VND"
            || snapshot.title.trim().is_empty()
            || !matches!(
                snapshot.stock_state.as_str(),
                "available" | "low" | "out_of_stock" | "unknown"
            )
            || snapshot.price_vnd.is_some_and(|p| p > i64::MAX as u64)
            || chrono::DateTime::parse_from_rfc3339(&snapshot.observed_at).is_err()
            || chrono::DateTime::parse_from_rfc3339(&snapshot.valid_until).is_err()
        {
            return Err(AdapterError::InvalidResponse);
        }
        Ok(snapshot)
    }
}

/// Per-store product cache. Keys include tenant and store; older revisions are rejected.
#[derive(Debug, Clone, Default)]
pub struct ProductCatalog {
    snapshots: HashMap<(String, String, String), ProductSnapshot>,
}
impl ProductCatalog {
    pub fn upsert(&mut self, snapshot: ProductSnapshot) -> Result<(), AdapterError> {
        let mut validator = crate::live_brain::ProductSession::default();
        validator
            .upsert(snapshot.clone())
            .map_err(|_| AdapterError::InvalidResponse)?;
        validate_id(&snapshot.tenant_id, 160)?;
        validate_id(&snapshot.store_id, 160)?;
        validate_id(&snapshot.sku, 160)?;
        let key = (
            snapshot.tenant_id.clone(),
            snapshot.store_id.clone(),
            snapshot.sku.clone(),
        );
        if self.snapshots.len() >= 512 && !self.snapshots.contains_key(&key) {
            return Err(AdapterError::Capacity);
        }
        if let Some(previous) = self.snapshots.get(&key) {
            if snapshot.revision < previous.revision {
                return Err(AdapterError::Stale);
            }
            if snapshot.revision == previous.revision && snapshot != *previous {
                return Err(AdapterError::Stale);
            }
        }
        self.snapshots.insert(key, snapshot);
        Ok(())
    }
    pub fn get(&self, tenant: &str, store: &str, sku: &str, now: &str) -> Option<&ProductSnapshot> {
        let now = chrono::DateTime::parse_from_rfc3339(now).ok()?;
        self.snapshots
            .get(&(tenant.into(), store.into(), sku.into()))
            .filter(|s| {
                chrono::DateTime::parse_from_rfc3339(&s.valid_until)
                    .is_ok_and(|expiry| expiry > now)
            })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ShippingQuote {
    pub quote_id: String,
    pub store_id: String,
    pub sku: String,
    pub total_vnd: u64,
    pub expires_at: String,
    pub destination_scope: String,
}
impl ShippingQuote {
    pub fn validate(&self, now: &str, required_scope: &str) -> Result<(), AdapterError> {
        validate_id(&self.quote_id, 160)?;
        if self.store_id.trim().is_empty()
            || self.sku.trim().is_empty()
            || self.total_vnd > i64::MAX as u64
            || self.destination_scope != required_scope
        {
            return Err(AdapterError::InvalidResponse);
        }
        let now =
            chrono::DateTime::parse_from_rfc3339(now).map_err(|_| AdapterError::InvalidInput)?;
        if !chrono::DateTime::parse_from_rfc3339(&self.expires_at).is_ok_and(|expiry| expiry > now)
        {
            return Err(AdapterError::Stale);
        }
        Ok(())
    }
}

/// Stable asset manifest for prepared Concat scenes and motion clips.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MotionClip {
    pub clip_id: String,
    pub path: String,
    pub sha256: String,
    pub duration_ms: u32,
    pub gesture: String,
    pub emotion: String,
    pub loopable: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SceneManifest {
    pub schema_version: u8,
    pub manifest_id: String,
    pub revision: u64,
    pub clips: Vec<MotionClip>,
}
impl SceneManifest {
    pub fn validate_files(&self, root: &Path) -> Result<(), AdapterError> {
        if self.schema_version != 1
            || self.manifest_id.trim().is_empty()
            || self.clips.is_empty()
            || self.clips.len() > 256
        {
            return Err(AdapterError::InvalidInput);
        }
        let root = root
            .canonicalize()
            .map_err(|_| AdapterError::InvalidConfig)?;
        let mut ids = std::collections::HashSet::new();
        for clip in &self.clips {
            if clip.clip_id.trim().is_empty()
                || !ids.insert(&clip.clip_id)
                || clip.duration_ms == 0
                || clip.duration_ms > 120_000
                || !valid_sha256(&clip.sha256)
                || !matches!(
                    clip.gesture.as_str(),
                    "idle"
                        | "attentive"
                        | "smile"
                        | "point_left"
                        | "point_right"
                        | "present_product"
                        | "hand_detail"
                        | "look_down"
                )
                || !matches!(
                    clip.emotion.as_str(),
                    "neutral" | "friendly" | "happy" | "confident" | "attentive"
                )
            {
                return Err(AdapterError::InvalidInput);
            }
            let rel = Path::new(&clip.path);
            if rel.is_absolute()
                || rel
                    .components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
            {
                return Err(AdapterError::InvalidInput);
            }
            let file = root
                .join(rel)
                .canonicalize()
                .map_err(|_| AdapterError::InvalidInput)?;
            if !file.starts_with(&root) || !file.is_file() {
                return Err(AdapterError::InvalidInput);
            }
            let mut input = fs::File::open(&file).map_err(|_| AdapterError::Unavailable)?;
            if input
                .metadata()
                .map_err(|_| AdapterError::Unavailable)?
                .len()
                > 512 * 1024 * 1024
            {
                return Err(AdapterError::InvalidInput);
            }
            let mut hasher = Sha256::new();
            let mut buffer = [0u8; 65536];
            let mut total = 0u64;
            loop {
                let n = input
                    .read(&mut buffer)
                    .map_err(|_| AdapterError::Unavailable)?;
                if n == 0 {
                    break;
                }
                total += n as u64;
                if total > 512 * 1024 * 1024 {
                    return Err(AdapterError::InvalidInput);
                }
                hasher.update(&buffer[..n]);
            }
            if hex_lower(&hasher.finalize()) != clip.sha256.to_ascii_lowercase() {
                return Err(AdapterError::InvalidResponse);
            }
        }
        Ok(())
    }
}
fn valid_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}
fn hex_lower(bytes: &[u8]) -> String {
    const H: &[u8] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(H[(b >> 4) as usize] as char);
        out.push(H[(b & 15) as usize] as char);
    }
    out
}

/// Capability and approval gate for platform write actions. No platform client is called here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionState {
    Pending,
    Submitted,
    Acked,
    Rejected,
    Unknown,
}
#[derive(Debug, Clone)]
pub struct PinApproval {
    pub action_id: String,
    pub sku: String,
    pub approved_by: String,
    pub state: ActionState,
}
impl PinApproval {
    pub fn request(
        action_id: &str,
        sku: &str,
        write_enabled: bool,
        authorized: bool,
        capability: bool,
        approved_by: Option<&str>,
    ) -> Result<Self, AdapterError> {
        if !write_enabled || !authorized || !capability {
            return Err(AdapterError::Unauthorized);
        }
        validate_id(action_id, 160)?;
        validate_id(sku, 160)?;
        let by = approved_by
            .filter(|s| !s.trim().is_empty())
            .ok_or(AdapterError::Unauthorized)?;
        Ok(Self {
            action_id: action_id.into(),
            sku: sku.into(),
            approved_by: by.into(),
            state: ActionState::Pending,
        })
    }
    pub fn submitted(&mut self) {
        if self.state == ActionState::Pending {
            self.state = ActionState::Submitted;
        }
    }
    pub fn reconcile(&mut self, confirmed_sku: Option<&str>, timed_out: bool) {
        if self.state == ActionState::Submitted {
            self.state = if confirmed_sku == Some(self.sku.as_str()) {
                ActionState::Acked
            } else if timed_out {
                ActionState::Unknown
            } else {
                ActionState::Submitted
            };
        }
    }
}

/// Bounded latency/error counters suitable for local status UI.
#[derive(Debug, Clone)]
pub struct LiveMetrics {
    capacity: usize,
    ingress_to_plan_ms: Vec<u64>,
    failed_tasks: u64,
    dropped_events: u64,
}
impl LiveMetrics {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.clamp(1, 4096),
            ingress_to_plan_ms: Vec::new(),
            failed_tasks: 0,
            dropped_events: 0,
        }
    }
    pub fn record_plan(&mut self, ms: u64) {
        if self.ingress_to_plan_ms.len() == self.capacity {
            self.ingress_to_plan_ms.remove(0);
        }
        self.ingress_to_plan_ms.push(ms);
    }
    pub fn record_failure(&mut self) {
        self.failed_tasks = self.failed_tasks.saturating_add(1);
    }
    pub fn record_drop(&mut self) {
        self.dropped_events = self.dropped_events.saturating_add(1);
    }
    pub fn p95_ingress_to_plan_ms(&self) -> Option<u64> {
        if self.ingress_to_plan_ms.is_empty() {
            None
        } else {
            let mut v = self.ingress_to_plan_ms.clone();
            v.sort_unstable();
            Some(v[((v.len() * 95).div_ceil(100) - 1).min(v.len() - 1)])
        }
    }
    pub fn failed_tasks(&self) -> u64 {
        self.failed_tasks
    }
    pub fn dropped_events(&self) -> u64 {
        self.dropped_events
    }
    pub fn samples(&self) -> usize {
        self.ingress_to_plan_ms.len()
    }
}

/// Offline deployment preflight; does not start engines or publish output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preflight {
    pub live_flag_off: bool,
    pub platform_write_off: bool,
    pub loopback_only: bool,
    pub assets_validated: bool,
    pub runtime_ready: bool,
}
impl Preflight {
    pub fn can_start_local_preview(&self) -> bool {
        self.live_flag_off
            && self.platform_write_off
            && self.loopback_only
            && self.assets_validated
            && self.runtime_ready
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opentalking_adapter_requires_flag_and_numeric_loopback() {
        assert!(matches!(
            OpenTalkingLiveAdapter::new("http://127.0.0.1:8000/", false),
            Err(AdapterError::Disabled)
        ));
        assert!(OpenTalkingLiveAdapter::new("http://localhost:8000/", true).is_err());
        assert!(OpenTalkingLiveAdapter::new("http://0.0.0.0:8000/", true).is_err());
        assert!(OpenTalkingLiveAdapter::new("http://127.0.0.1:8000/", true).is_ok());
    }
    #[test]
    fn shipping_quote_must_be_fresh_and_scope_matched() {
        let q = ShippingQuote {
            quote_id: "q1".into(),
            store_id: "store".into(),
            sku: "sku".into(),
            total_vnd: 25000,
            expires_at: "2099-01-01T00:00:00Z".into(),
            destination_scope: "HCM".into(),
        };
        assert!(q.validate("2026-01-01T00:00:00Z", "HCM").is_ok());
        assert_eq!(
            q.validate("2026-01-01T00:00:00Z", "HN"),
            Err(AdapterError::InvalidResponse)
        );
        assert_eq!(
            q.validate("2100-01-01T00:00:00Z", "HCM"),
            Err(AdapterError::Stale)
        );
    }
    #[test]
    fn pin_write_requires_enabled_authorized_capability_and_human_approval() {
        assert_eq!(
            PinApproval::request("a", "sku", true, true, false, Some("operator")).err(),
            Some(AdapterError::Unauthorized)
        );
        let mut a = PinApproval::request("a", "sku", true, true, true, Some("operator")).unwrap();
        a.submitted();
        a.reconcile(None, true);
        assert_eq!(a.state, ActionState::Unknown);
        let mut b = PinApproval::request("b", "sku", true, true, true, Some("operator")).unwrap();
        b.submitted();
        b.reconcile(Some("sku"), false);
        assert_eq!(b.state, ActionState::Acked);
    }
    #[test]
    fn metrics_are_bounded_and_p95_is_deterministic() {
        let mut m = LiveMetrics::new(3);
        for n in [10, 30, 20, 40] {
            m.record_plan(n);
        }
        assert_eq!(m.samples(), 3);
        assert_eq!(m.p95_ingress_to_plan_ms(), Some(40));
        m.record_drop();
        assert_eq!(m.dropped_events(), 1);
    }
    #[test]
    fn preflight_never_confuses_live_write_with_safe_preview() {
        let p = Preflight {
            live_flag_off: true,
            platform_write_off: true,
            loopback_only: true,
            assets_validated: true,
            runtime_ready: true,
        };
        assert!(p.can_start_local_preview());
        let mut bad = p.clone();
        bad.platform_write_off = false;
        assert!(!bad.can_start_local_preview());
    }
    #[test]
    fn speech_http_ack_and_stale_events_do_not_claim_playback() {
        let mut lifecycle = SpeechLifecycle::planned(8);
        assert!(lifecycle.apply(8, "queued"));
        assert_eq!(lifecycle.state, SpeechState::Submitted);
        assert!(!lifecycle.completion_verified);
        assert!(!lifecycle.apply(7, "started"));
        assert!(lifecycle.apply(8, "started"));
        assert!(lifecycle.apply(8, "finished"));
        assert!(lifecycle.completion_verified);
    }
    #[test]
    fn opentalking_mock_http_receipt_is_only_submitted() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = stream.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..n]);
                if request.windows(4).any(|w| w == b"\r\n\r\n") {
                    let header_end = request.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
                    let headers =
                        String::from_utf8_lossy(&request[..header_end]).to_ascii_lowercase();
                    let content_len = headers
                        .lines()
                        .find_map(|l| {
                            l.strip_prefix("content-length: ")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if request.len() >= header_end + content_len {
                        break;
                    }
                }
            }
            let text = String::from_utf8_lossy(&request);
            assert!(text.starts_with("POST /sessions/session-1/speak HTTP/1.1"));
            assert!(
                text.to_ascii_lowercase()
                    .contains("idempotency-key: kedi-kedi-task-9")
            );
            assert!(text.contains("\"mode\":\"replace\""));
            let body =
                r#"{"session_id":"session-1","command_id":"kedi-kedi-task-9","status":"queued"}"#;
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
        });
        let adapter = OpenTalkingLiveAdapter::new(&format!("http://{address}/"), true).unwrap();
        let task = LiveTask {
            schema_version: 1,
            task_id: "kedi-task-9".into(),
            trace_id: "trace-9".into(),
            session_id: "session-1".into(),
            origin_event_id: None,
            task_type: "speech_and_scene".into(),
            priority: crate::live_brain::LivePriority::P1,
            generation_id: 9,
            deadline_at: "2099-01-01T00:00:00Z".into(),
            product_ref: None,
            speech: crate::live_brain::SpeechPayload {
                text: "Xin chào".into(),
                voice_id: "default".into(),
                language: "vi-VN".into(),
            },
            motion: crate::live_brain::MotionPayload {
                gesture: "idle".into(),
                emotion: "neutral".into(),
            },
            scene: crate::live_brain::ScenePayload {
                scene_id: "default".into(),
                show_price: false,
                show_sku: None,
            },
            platform_actions: Vec::new(),
            estimated_speech_ms: 1000,
            expected_session_revision: 1,
        };
        let receipt = adapter.submit("session-1", &task).unwrap();
        assert_eq!(receipt.status, "SUBMITTED");
        server.join().unwrap();
    }
    #[test]
    fn commerce_gateway_uses_store_scope_and_bearer_token_in_mock() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut req = [0u8; 4096];
            let n = stream.read(&mut req).unwrap();
            let request = String::from_utf8_lossy(&req[..n]);
            assert!(request.starts_with("GET /v1/live/products/SKU-1/snapshot?"));
            assert!(request.contains("store_id=store-1"));
            assert!(request.contains("tenant_id=tenant-1"));
            assert!(
                request
                    .to_ascii_lowercase()
                    .contains("authorization: bearer test-token")
            );
            let body = r#"{"schema_version":1,"tenant_id":"tenant-1","store_id":"store-1","sku":"SKU-1","revision":3,"currency":"VND","title":"Demo","price_vnd":15000,"stock_state":"available","observed_at":"2026-10-27T00:00:00Z","valid_until":"2099-01-01T00:00:00Z","verification_source":"fufuni"}"#;
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
        });
        let client =
            CommerceGatewayClient::new(&format!("http://{address}/"), "test-token", true).unwrap();
        let snapshot = client.snapshot("tenant-1", "store-1", "SKU-1").unwrap();
        assert_eq!(snapshot.revision, 3);
        server.join().unwrap();
        assert!(CommerceGatewayClient::new("http://example.invalid/", "token", true).is_err());
    }
    #[test]
    fn tiktok_adapter_normalizes_only_chat_and_hashes_viewer_reference() {
        let adapter = TikTokReadOnlyAdapter::new("tenant", "store").unwrap();
        let frame = LiveFrame {
            kind: LiveFrameKind::Event,
            value: json!({"type":"live.event","source":"tiktok","eventKind":"chat","eventId":"event-1","sessionId":"room-1","receivedAt":"2026-10-27T00:00:00Z","actor":{"userId":"sensitive-account-id","userName":"Viewer Name","displayId":"handle"},"payload":{"content":"  giá   bao nhiêu? "}}),
        };
        let event = adapter.normalize_comment(&frame).unwrap().unwrap();
        assert_eq!(event.text.as_deref(), Some("giá bao nhiêu?"));
        assert!(event.viewer_ref.starts_with("tt-"));
        assert!(!event.viewer_ref.contains("sensitive-account-id"));
        assert_eq!(event.tenant_id, "tenant");
        assert_eq!(event.store_id, "store");
        let gift = LiveFrame {
            kind: LiveFrameKind::Event,
            value: json!({"source":"tiktok","eventKind":"gift"}),
        };
        assert!(adapter.normalize_comment(&gift).unwrap().is_none());
        let caps = TikTokReadOnlyAdapter::capabilities();
        assert!(caps.read_comments);
        assert!(!caps.authorized);
        assert!(!caps.request_pin_product);
        assert!(!caps.send_comment);
    }
    #[test]
    fn product_catalog_isolated_by_tenant_and_rejects_old_revisions() {
        let p = ProductSnapshot {
            schema_version: 1,
            tenant_id: "t1".into(),
            store_id: "s1".into(),
            sku: "sku".into(),
            revision: 2,
            currency: "VND".into(),
            title: "x".into(),
            price_vnd: Some(10),
            stock_state: "available".into(),
            observed_at: "2026-01-01T00:00:00Z".into(),
            valid_until: "2099-01-01T00:00:00Z".into(),
            verification_source: "mock".into(),
            ..Default::default()
        };
        let mut c = ProductCatalog::default();
        c.upsert(p.clone()).unwrap();
        assert!(c.get("t2", "s1", "sku", "2026-01-02T00:00:00Z").is_none());
        let mut stale = p;
        stale.revision = 1;
        assert_eq!(c.upsert(stale), Err(AdapterError::Stale));
    }
    #[test]
    fn scene_manifest_rejects_traversal_and_verifies_hash() {
        let dir = std::env::temp_dir().join(format!("kedi-live-manifest-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let bytes = b"test clip";
        fs::write(dir.join("clip.mp4"), bytes).unwrap();
        let hash = hex_lower(&Sha256::digest(bytes));
        let mk = |path: &str| SceneManifest {
            schema_version: 1,
            manifest_id: "test".into(),
            revision: 1,
            clips: vec![MotionClip {
                clip_id: "idle".into(),
                path: path.into(),
                sha256: hash.clone(),
                duration_ms: 1500,
                gesture: "idle".into(),
                emotion: "neutral".into(),
                loopable: true,
            }],
        };
        assert!(mk("clip.mp4").validate_files(&dir).is_ok());
        assert!(mk("..\\outside.mp4").validate_files(&dir).is_err());
        assert!(mk("missing.mp4").validate_files(&dir).is_err());
        let _ = fs::remove_dir_all(dir);
    }
}
