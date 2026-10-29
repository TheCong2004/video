// SPDX-License-Identifier: AGPL-3.0-or-later
//! One-owner Kedi live controller shared by the desktop and local operator API.
//! Network/model calls happen outside this controller's lock. STOP and takeover
//! invalidate generations synchronously, even while a provider request is stuck.
#![allow(missing_docs)]

use crate::live_adapters::LiveMetrics;
use crate::live_brain::{
    CommentSimulator, LiveBrain, LiveEvent, LiveFeatureFlags, LivePriority, LiveSession,
    LiveSessionState, LiveTask, PolicyGate, ProductSession, ProductSnapshot,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::Path,
};

/// UTC audit time; scheduling durations are measured separately with Instant.
pub fn utc_now() -> String {
    chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now()).to_rfc3339()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    pub session_id: String,
    pub tenant_id: String,
    pub store_id: String,
    pub dev_mode: bool,
}
impl RuntimeConfig {
    pub fn demo() -> Self {
        Self {
            session_id: format!("local-{:016x}", rand::random::<u64>()),
            tenant_id: "demo-tenant".into(),
            store_id: "demo-store".into(),
            dev_mode: true,
        }
    }
    pub fn validate(&self) -> Result<(), ControlError> {
        for value in [&self.session_id, &self.tenant_id, &self.store_id] {
            if value.is_empty()
                || value.len() > 100
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
            {
                return Err(ControlError::Input);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Viewer,
    Producer,
    Operator,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlRequest {
    pub command_id: String,
    pub expected_revision: u64,
    pub action: ControlAction,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlAction {
    Start,
    Stop,
    Pause,
    Resume,
    Takeover,
    IntroduceProduct,
    Feature { sku: String, revision: u64 },
    Simulate { text: String },
    Speak { text: String },
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ControlError {
    Disabled,
    Unauthorized,
    Conflict,
    Input,
    State,
    Policy,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStatus {
    pub session_id: String,
    pub tenant_id: String,
    pub store_id: String,
    pub state: LiveSessionState,
    pub revision: u64,
    pub generation: u64,
    pub automatic: bool,
    pub queued_tasks: usize,
    pub queued_audio_ms: u32,
    pub featured_sku: Option<String>,
    pub confirmed_pinned_sku: Option<String>,
    pub active_task: Option<String>,
    pub speech_state: String,
    pub scene: String,
    pub last_safe_reply: Option<String>,
    pub fallback_active: bool,
    pub dropped_events: u64,
    pub failed_tasks: u64,
    pub p95_plan_ms: Option<u64>,
    pub analytics_pending: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterestSignal {
    pub trace_id: String,
    pub session_id: String,
    pub tenant_id: String,
    pub store_id: String,
    pub viewer_ref: String,
    pub intent: String,
    pub sku: Option<String>,
}

struct ActiveSpeech {
    task: LiveTask,
    submitted_ms: u64,
    started: bool,
    first_audio_deadline: std::time::Instant,
}

/// Serialized session owner. It produces plans, not neural media frames.
pub struct KediLiveRuntime {
    pub config: RuntimeConfig,
    session: LiveSession,
    flags: LiveFeatureFlags,
    planner: CommentSimulator,
    generation: u64,
    automatic: bool,
    active: Option<ActiveSpeech>,
    resume_script: Option<LiveTask>,
    last_reply: Option<String>,
    scene: String,
    fallback: bool,
    metrics: LiveMetrics,
    receipts: VecDeque<(String, [u8; 32], RuntimeStatus)>,
    viewers: HashMap<String, (u64, u8)>,
    event_deadlines: HashMap<String, u64>,
    analytics: VecDeque<InterestSignal>,
    provider_failures: u8,
    circuit_until_ms: u64,
}
impl KediLiveRuntime {
    pub fn new(config: RuntimeConfig, flags: LiveFeatureFlags) -> Result<Self, ControlError> {
        config.validate()?;
        let session =
            LiveSession::new(config.session_id.clone()).map_err(|_| ControlError::Input)?;
        Ok(Self {
            config,
            session,
            flags,
            planner: CommentSimulator::new(LiveBrain {
                products: ProductSession::default(),
            }),
            generation: 0,
            automatic: false,
            active: None,
            resume_script: None,
            last_reply: None,
            scene: "idle".into(),
            fallback: true,
            metrics: LiveMetrics::new(1024),
            receipts: VecDeque::new(),
            viewers: HashMap::new(),
            event_deadlines: HashMap::new(),
            analytics: VecDeque::new(),
            provider_failures: 0,
            circuit_until_ms: 0,
        })
    }
    pub fn status(&self) -> RuntimeStatus {
        RuntimeStatus {
            session_id: self.config.session_id.clone(),
            tenant_id: self.config.tenant_id.clone(),
            store_id: self.config.store_id.clone(),
            state: self.session.state,
            revision: self.session.revision,
            generation: self.generation,
            automatic: self.automatic,
            queued_tasks: self.planner.scheduler.len(),
            queued_audio_ms: self.planner.scheduler.queued_speech_ms(),
            featured_sku: self.planner.brain.products.featured_sku.clone(),
            confirmed_pinned_sku: self.planner.brain.products.confirmed_pinned_sku.clone(),
            active_task: self.active.as_ref().map(|a| a.task.task_id.clone()),
            speech_state: if self.active.as_ref().is_some_and(|a| a.started) {
                "STARTED"
            } else if self.active.is_some() {
                "SUBMITTED"
            } else {
                "IDLE"
            }
            .into(),
            scene: self.scene.clone(),
            last_safe_reply: self.last_reply.clone(),
            fallback_active: self.fallback,
            dropped_events: self.metrics.dropped_events(),
            failed_tasks: self.metrics.failed_tasks(),
            p95_plan_ms: self.metrics.p95_ingress_to_plan_ms(),
            analytics_pending: self.analytics.len(),
        }
    }
    fn transition(&mut self, next: LiveSessionState) -> Result<(), ControlError> {
        self.session
            .transition(next)
            .map_err(|_| ControlError::State)?;
        Ok(())
    }
    fn cancel(&mut self) {
        self.generation = self.generation.saturating_add(1);
        self.active = None;
        self.resume_script = None;
        self.planner.scheduler.clear();
        self.event_deadlines.clear();
        self.scene = "idle".into();
        self.fallback = true;
    }
    /// Scope validation is enforced before facts are made available to the brain.
    pub fn install_snapshot(&mut self, snapshot: ProductSnapshot) -> Result<(), ControlError> {
        if snapshot.tenant_id != self.config.tenant_id || snapshot.store_id != self.config.store_id
        {
            return Err(ControlError::Unauthorized);
        }
        if !self.config.dev_mode && snapshot.verification_source == "mock" {
            return Err(ControlError::Policy);
        }
        let sku = snapshot.sku.clone();
        let changed = self.planner.brain.products.revision(&sku) != Some(snapshot.revision);
        self.planner
            .brain
            .products
            .upsert(snapshot)
            .map_err(|_| ControlError::Policy)?;
        if changed && self.planner.brain.products.featured_sku.as_deref() == Some(&sku) {
            self.cancel();
        }
        Ok(())
    }
    /// Enforce authenticated command roles, CAS revisions and bounded idempotency receipts.
    pub fn execute(
        &mut self,
        request: ControlRequest,
        role: Role,
        now: &str,
        now_ms: u64,
    ) -> Result<RuntimeStatus, ControlError> {
        if role == Role::Viewer
            || (role == Role::Producer
                && !matches!(
                    request.action,
                    ControlAction::Feature { .. } | ControlAction::IntroduceProduct
                ))
        {
            return Err(ControlError::Unauthorized);
        }
        if request.command_id.is_empty()
            || request.command_id.len() > 128
            || request.command_id.chars().any(char::is_control)
        {
            return Err(ControlError::Input);
        }
        let fingerprint: [u8; 32] =
            Sha256::digest(serde_json::to_vec(&request).map_err(|_| ControlError::Input)?).into();
        if let Some((_, old, result)) = self
            .receipts
            .iter()
            .find(|(id, _, _)| id == &request.command_id)
        {
            return if old == &fingerprint {
                Ok(result.clone())
            } else {
                Err(ControlError::Conflict)
            };
        }
        // Emergency STOP is independent of stale UI revision and the speech worker.
        if !matches!(request.action, ControlAction::Stop)
            && request.expected_revision != self.session.revision
        {
            return Err(ControlError::Conflict);
        }
        match &request.action {
            ControlAction::Start => {
                if !self.flags.live_enabled {
                    return Err(ControlError::Disabled);
                }
                match self.session.state {
                    LiveSessionState::Created => {
                        self.transition(LiveSessionState::Preparing)?;
                        self.transition(LiveSessionState::Ready)?;
                        self.transition(LiveSessionState::Live)?;
                    }
                    LiveSessionState::Recovering => self.transition(LiveSessionState::Live)?,
                    _ => return Err(ControlError::State),
                }
                self.automatic = true;
            }
            ControlAction::Stop => {
                self.automatic = false;
                self.cancel();
                if self.session.state != LiveSessionState::Stopped {
                    if self.session.state != LiveSessionState::Stopping {
                        self.transition(LiveSessionState::Stopping)?;
                    }
                    self.transition(LiveSessionState::Stopped)?;
                }
            }
            ControlAction::Pause => {
                self.transition(LiveSessionState::Paused)?;
                self.cancel();
            }
            ControlAction::Resume => {
                if self.session.state == LiveSessionState::Live && !self.automatic {
                    self.session.revision = self
                        .session
                        .revision
                        .checked_add(1)
                        .ok_or(ControlError::State)?;
                } else {
                    self.transition(LiveSessionState::Live)?;
                }
                self.automatic = true;
            }
            ControlAction::Takeover => {
                if !matches!(
                    self.session.state,
                    LiveSessionState::Live
                        | LiveSessionState::Paused
                        | LiveSessionState::Recovering
                ) {
                    return Err(ControlError::State);
                }
                self.automatic = false;
                self.cancel();
                self.session.revision = self
                    .session
                    .revision
                    .checked_add(1)
                    .ok_or(ControlError::State)?;
            }
            ControlAction::Feature { sku, revision } => {
                let p = self
                    .planner
                    .brain
                    .products
                    .snapshot(&self.config.tenant_id, &self.config.store_id, sku, now)
                    .ok_or(ControlError::Policy)?;
                if p.revision != *revision {
                    return Err(ControlError::Conflict);
                }
                self.planner
                    .brain
                    .products
                    .feature(sku)
                    .map_err(|_| ControlError::Policy)?;
                self.cancel();
                self.session.revision = self
                    .session
                    .revision
                    .checked_add(1)
                    .ok_or(ControlError::State)?;
            }
            ControlAction::IntroduceProduct => {
                if self.session.state != LiveSessionState::Live {
                    return Err(ControlError::State);
                }
                let sku = self
                    .planner
                    .brain
                    .products
                    .featured_sku
                    .clone()
                    .ok_or(ControlError::Policy)?;
                let product = self
                    .planner
                    .brain
                    .products
                    .snapshot(&self.config.tenant_id, &self.config.store_id, &sku, now)
                    .ok_or(ControlError::Policy)?;
                let reference = crate::live_brain::ProductRef {
                    sku: sku.clone(),
                    revision: product.revision,
                };
                let show_price = product.price_vnd.is_some();
                let mut event =
                    crate::live_brain::simulated_comment(&request.command_id, "giới thiệu");
                event.tenant_id = self.config.tenant_id.clone();
                event.store_id = self.config.store_id.clone();
                let speech = self
                    .planner
                    .brain
                    .answer(
                        &event,
                        if show_price {
                            crate::live_brain::LiveIntent::Price
                        } else {
                            crate::live_brain::LiveIntent::Stock
                        },
                        now,
                    )
                    .ok_or(ControlError::Policy)?;
                let task = LiveTask {
                    schema_version: 1,
                    task_id: format!("script-{}", request.command_id),
                    trace_id: request.command_id.clone(),
                    session_id: self.config.session_id.clone(),
                    origin_event_id: None,
                    task_type: "script".into(),
                    priority: LivePriority::P3,
                    generation_id: self.generation,
                    deadline_at: (chrono::DateTime::parse_from_rfc3339(now)
                        .map_err(|_| ControlError::Input)?
                        + chrono::Duration::seconds(15))
                    .to_rfc3339(),
                    product_ref: Some(reference),
                    speech: crate::live_brain::SpeechPayload {
                        text: speech,
                        voice_id: "default".into(),
                        language: "vi-VN".into(),
                    },
                    motion: crate::live_brain::MotionPayload {
                        gesture: "present_product".into(),
                        emotion: "neutral".into(),
                    },
                    scene: crate::live_brain::ScenePayload {
                        scene_id: "product_focus".into(),
                        show_price,
                        show_sku: Some(sku),
                    },
                    platform_actions: Vec::new(),
                    estimated_speech_ms: 6000,
                    expected_session_revision: self.session.revision,
                };
                if !PolicyGate::allows(&task, &self.planner.brain.products, now)
                    || !self.planner.scheduler.offer(task)
                {
                    return Err(ControlError::Policy);
                }
            }
            ControlAction::Simulate { text } => {
                if !self.config.dev_mode {
                    return Err(ControlError::Unauthorized);
                }
                let event = LiveEvent {
                    schema_version: 1,
                    event_id: request.command_id.clone(),
                    trace_id: request.command_id.clone(),
                    session_id: self.config.session_id.clone(),
                    tenant_id: self.config.tenant_id.clone(),
                    store_id: self.config.store_id.clone(),
                    platform: "simulator".into(),
                    event_type: "viewer.comment".into(),
                    received_at: now.into(),
                    viewer_ref: "simulated-viewer".into(),
                    text: Some(text.clone()),
                    platform_product_id: None,
                    metadata: HashMap::new(),
                    source_event_id: None,
                };
                self.ingest(event, now, now_ms)?;
            }
            ControlAction::Speak { text } => {
                if self.session.state != LiveSessionState::Live {
                    return Err(ControlError::State);
                }
                // Manual speech is deliberately free of numeric/financial claims.
                if text.trim().is_empty()
                    || text.chars().count() > 600
                    || text.chars().any(|c| c.is_ascii_digit())
                    || crate::live_brain::classify_intent(text)
                        == crate::live_brain::LiveIntent::Unsafe
                {
                    return Err(ControlError::Policy);
                }
                let lower = text.to_lowercase();
                if [
                    "giá",
                    "còn hàng",
                    "hết hàng",
                    "miễn phí",
                    "giảm giá",
                    "hoàn tiền",
                    "chốt đơn",
                ]
                .iter()
                .any(|s| lower.contains(s))
                {
                    return Err(ControlError::Policy);
                }
                self.cancel();
                let task = LiveTask {
                    schema_version: 1,
                    task_id: format!("manual-{}", request.command_id),
                    trace_id: request.command_id.clone(),
                    session_id: self.config.session_id.clone(),
                    origin_event_id: None,
                    task_type: "operator_control".into(),
                    priority: LivePriority::P0,
                    generation_id: self.generation,
                    deadline_at: (chrono::DateTime::parse_from_rfc3339(now)
                        .map_err(|_| ControlError::Input)?
                        + chrono::Duration::seconds(3))
                    .to_rfc3339(),
                    product_ref: None,
                    speech: crate::live_brain::SpeechPayload {
                        text: text.trim().into(),
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
                    estimated_speech_ms: 6000,
                    expected_session_revision: self.session.revision,
                };
                if !PolicyGate::allows(&task, &self.planner.brain.products, now)
                    || !self.planner.scheduler.offer(task)
                {
                    return Err(ControlError::Policy);
                }
            }
        }
        let result = self.status();
        if self.receipts.len() >= 1024 {
            self.receipts.pop_front();
        }
        self.receipts
            .push_back((request.command_id, fingerprint, result.clone()));
        Ok(result)
    }
    /// Bounded real/simulated comment ingress. Viewer events cannot become operator commands.
    pub fn ingest(
        &mut self,
        event: LiveEvent,
        now: &str,
        now_ms: u64,
    ) -> Result<bool, ControlError> {
        if self.session.state != LiveSessionState::Live || !self.automatic {
            return Err(ControlError::State);
        }
        if event.tenant_id != self.config.tenant_id
            || event.store_id != self.config.store_id
            || event.session_id != self.config.session_id
        {
            return Err(ControlError::Unauthorized);
        }
        if event.event_type != "viewer.comment" {
            return Err(ControlError::Input);
        }
        let event = event.normalize().map_err(|_| ControlError::Input)?;
        let event_id = event.event_id.clone();
        self.viewers
            .retain(|_, (at, _)| now_ms.saturating_sub(*at) < 10_000);
        if self.viewers.len() >= 512 && !self.viewers.contains_key(&event.viewer_ref) {
            self.metrics.record_drop();
            return Ok(false);
        }
        let counter = self
            .viewers
            .entry(event.viewer_ref.clone())
            .or_insert((now_ms, 0));
        if counter.1 >= 2 {
            self.metrics.record_drop();
            return Ok(false);
        }
        counter.1 += 1;
        let started = std::time::Instant::now();
        let signal = InterestSignal {
            trace_id: event.trace_id.clone(),
            session_id: self.config.session_id.clone(),
            tenant_id: self.config.tenant_id.clone(),
            store_id: self.config.store_id.clone(),
            viewer_ref: event.viewer_ref.clone(),
            intent: format!(
                "{:?}",
                crate::live_brain::classify_intent(event.text.as_deref().unwrap_or(""))
            ),
            sku: self.planner.brain.products.featured_sku.clone(),
        };
        let accepted = self
            .planner
            .ingest(event, now_ms, now, self.session.revision)
            .map_err(|_| ControlError::Input)?;
        if accepted
            && self
                .active
                .as_ref()
                .is_some_and(|a| a.task.priority == LivePriority::P3)
            && self
                .planner
                .scheduler
                .next_priority()
                .is_some_and(|p| p <= LivePriority::P1)
        {
            self.resume_script = self.active.take().map(|a| a.task);
            self.generation = self.generation.saturating_add(1);
            self.fallback = true;
        }
        self.metrics
            .record_plan(started.elapsed().as_millis() as u64);
        if !accepted {
            self.metrics.record_drop();
        } else {
            self.event_deadlines
                .retain(|_, deadline| *deadline > now_ms);
            if self.event_deadlines.len() >= 2048 {
                if let Some(key) = self
                    .event_deadlines
                    .iter()
                    .min_by_key(|(_, t)| *t)
                    .map(|(key, _)| key.clone())
                {
                    self.event_deadlines.remove(&key);
                }
            }
            self.event_deadlines
                .insert(event_id, now_ms.saturating_add(3000));
            if self.analytics.len() >= 256 {
                self.analytics.pop_front();
                self.metrics.record_drop();
            }
            self.analytics.push_back(signal);
        }
        Ok(accepted)
    }
    /// Dispatch only one speech writer. Higher-priority buyer work invalidates script generations.
    pub fn dispatch(&mut self, now: &str, now_ms: u64) -> Option<LiveTask> {
        self.tick(now_ms);
        if self.provider_failures >= 3 {
            if self.circuit_until_ms == 0 {
                self.circuit_until_ms = now_ms.saturating_add(30_000);
            }
            if now_ms < self.circuit_until_ms {
                return None;
            }
            self.provider_failures = 2;
            self.circuit_until_ms = 0;
        }
        if self.session.state != LiveSessionState::Live {
            return None;
        }
        if self.active.is_none() && self.planner.scheduler.is_empty() && self.automatic {
            if let Some(mut task) = self.resume_script.take() {
                task.expected_session_revision = self.session.revision;
                task.task_id = format!("{}-resume-{}", task.task_id, self.generation);
                task.deadline_at = (chrono::DateTime::parse_from_rfc3339(now).ok()?
                    + chrono::Duration::seconds(15))
                .to_rfc3339();
                if PolicyGate::allows(&task, &self.planner.brain.products, now) {
                    self.planner.scheduler.offer(task);
                }
            }
        }
        if let Some(active) = &self.active {
            if self
                .planner
                .scheduler
                .next_priority()
                .is_some_and(|p| p < active.task.priority)
            {
                self.generation = self.generation.saturating_add(1);
                self.active = None;
                self.fallback = true;
            } else {
                return None;
            }
        }
        let (mut task, remaining_ms) = loop {
            let task = self
                .planner
                .dispatch_next(now, self.session.revision)
                .ok()??;
            let deadline = match &task.origin_event_id {
                Some(id) => self.event_deadlines.remove(id).unwrap_or(0),
                None => now_ms.saturating_add(if task.priority == LivePriority::P3 {
                    15000
                } else {
                    3000
                }),
            };
            if deadline <= now_ms {
                self.metrics.record_drop();
                continue;
            }
            break (task, deadline.saturating_sub(now_ms));
        };
        self.generation = self.generation.saturating_add(1);
        task.generation_id = self.generation;
        task.task_id = format!("{}-{}", self.config.session_id, task.task_id);
        self.last_reply = Some(task.speech.text.clone());
        self.scene = task.scene.scene_id.clone();
        self.active = Some(ActiveSpeech {
            task: task.clone(),
            submitted_ms: now_ms,
            started: false,
            first_audio_deadline: std::time::Instant::now()
                + std::time::Duration::from_millis(remaining_ms),
        });
        Some(task)
    }
    /// Enforce deadline/revision again at first audible frame, not only at HTTP submission.
    pub fn playback_started(&mut self, generation: u64, now: &str) -> bool {
        let Some(active) = self.active.as_mut() else {
            return false;
        };
        if active.task.generation_id != generation {
            return false;
        }
        let valid = active.task.generation_id == generation
            && std::time::Instant::now() < active.first_audio_deadline
            && self.session.state == LiveSessionState::Live
            && PolicyGate::allows(&active.task, &self.planner.brain.products, now)
            && chrono::DateTime::parse_from_rfc3339(&active.task.deadline_at)
                .ok()
                .zip(chrono::DateTime::parse_from_rfc3339(now).ok())
                .is_some_and(|(deadline, now)| deadline > now);
        if valid {
            active.started = true;
            self.fallback = false;
        } else {
            self.cancel();
            self.metrics.record_drop();
        }
        valid
    }
    pub fn playback_finished(&mut self, generation: u64, success: bool) -> bool {
        if !self
            .active
            .as_ref()
            .is_some_and(|a| a.task.generation_id == generation)
        {
            return false;
        }
        let success = success && self.active.as_ref().is_some_and(|a| a.started);
        if !success {
            self.metrics.record_failure();
            self.provider_failures = self.provider_failures.saturating_add(1);
        } else {
            self.provider_failures = 0;
            self.circuit_until_ms = 0;
        }
        self.active = None;
        self.fallback = true;
        self.scene = "idle".into();
        true
    }
    pub fn tick(&mut self, now_ms: u64) {
        if self.active.as_ref().is_some_and(|a| {
            now_ms.saturating_sub(a.submitted_ms) > if a.started { 30_000 } else { 5_000 }
        }) {
            self.cancel();
            self.metrics.record_failure();
            self.provider_failures = self.provider_failures.saturating_add(1);
        }
    }
    pub fn pop_analytics(&mut self) -> Option<InterestSignal> {
        self.analytics.pop_front()
    }
    pub fn generation_current(&self, generation: u64) -> bool {
        self.session.state == LiveSessionState::Live
            && self.active.as_ref().is_some_and(|a| {
                a.task.generation_id == generation
                    && (a.started || std::time::Instant::now() < a.first_audio_deadline)
            })
    }
    /// STOPPED remains terminal; a later operator start creates a fresh session identity.
    pub fn renew_session(&mut self) -> Result<(), ControlError> {
        if self.session.state != LiveSessionState::Stopped {
            return Err(ControlError::State);
        }
        let mut config = self.config.clone();
        config.session_id = format!("local-{:016x}", rand::random::<u64>());
        let products = self.planner.brain.products.clone();
        let generation = self.generation.saturating_add(1);
        *self = Self::new(config, self.flags)?;
        self.planner.brain.products = products;
        self.generation = generation;
        Ok(())
    }
    /// Persist only metadata. Restoring never republishes or replays speech automatically.
    pub fn checkpoint(&self, path: &Path) -> Result<(), ControlError> {
        Self::save_status(&self.status(), path)
    }
    /// Write a copied status outside the controller lock; no viewer text or secrets are saved.
    pub fn save_status(status: &RuntimeStatus, path: &Path) -> Result<(), ControlError> {
        let mut redacted = status.clone();
        redacted.last_safe_reply = None;
        let data = serde_json::to_vec(&redacted).map_err(|_| ControlError::Unavailable)?;
        let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
        fs::write(&temporary, data).map_err(|_| ControlError::Unavailable)?;
        if let Err(_) = fs::rename(&temporary, path) {
            let _ = fs::remove_file(&temporary);
            return Err(ControlError::Unavailable);
        }
        Ok(())
    }
    pub fn restore(
        config: RuntimeConfig,
        flags: LiveFeatureFlags,
        path: &Path,
    ) -> Result<Self, ControlError> {
        let meta = fs::metadata(path).map_err(|_| ControlError::Unavailable)?;
        if meta.len() > 65536 {
            return Err(ControlError::Input);
        }
        let saved: RuntimeStatus =
            serde_json::from_slice(&fs::read(path).map_err(|_| ControlError::Unavailable)?)
                .map_err(|_| ControlError::Input)?;
        if saved.session_id != config.session_id
            || saved.tenant_id != config.tenant_id
            || saved.store_id != config.store_id
        {
            return Err(ControlError::Unauthorized);
        }
        let mut runtime = Self::new(config, flags)?;
        runtime.generation = saved.generation.saturating_add(1);
        runtime.session.revision = saved.revision.saturating_add(1);
        runtime.session.state = if saved.state == LiveSessionState::Stopped {
            LiveSessionState::Stopped
        } else {
            LiveSessionState::Recovering
        };
        Ok(runtime)
    }
}
