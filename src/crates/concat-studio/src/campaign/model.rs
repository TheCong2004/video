// SPDX-License-Identifier: AGPL-3.0-or-later
//! Explicit, reviewable campaign policy and durable per-video records.
use crate::ai_control::digest;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Automation {
    Manual,
    SemiAuto,
    FullAuto,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PageRule {
    pub page_id: String,
    pub name: String,
    pub topics: Vec<String>,
    pub daily_limit: u16,
    pub slots: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Providers {
    pub text_model: String,
    pub grok_profile: String,
    pub anchor: Option<PathBuf>,
    pub assets: Vec<PathBuf>,
    pub allowed_fallbacks: Vec<String>,
    pub text_cost_micros: u64,
    pub media_cost_micros: u64,
    pub render_reserve_seconds: u64,
}
impl Providers {
    /// An absent upload selects prompt-based generation, not an empty render.
    pub fn uses_grok(&self) -> bool {
        self.anchor.is_some() || self.assets.is_empty()
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub objective: String,
    pub video_count: u32,
    pub video_seconds: f64,
    pub topic: String,
    pub language: String,
    pub style: String,
    pub unique_content: bool,
    pub facts: Vec<String>,
    pub pages: Vec<PageRule>,
    pub publish: bool,
    pub timezone: String,
    pub start_date: String,
    pub end_date: String,
    pub completion_deadline_utc: Option<i64>,
    pub allow_cross_page: bool,
    pub api_budget_micros: u64,
    pub render_budget_seconds: u64,
    pub max_workers: u8,
    pub mode: Automation,
    pub approve_content: bool,
    pub approve_publish: bool,
    pub max_attempts: u8,
    pub retry_delay_seconds: u32,
    pub providers: Providers,
    pub locked_fields: Vec<String>,
}
impl Plan {
    pub fn validate(&self) -> Result<(), String> {
        use chrono::NaiveDate;
        if self.objective.trim().is_empty()
            || self.objective.len() > 16000
            || self.topic.trim().is_empty()
            || self.language.trim().is_empty()
            || self.style.len() > 4000
            || !(1..=1000).contains(&self.video_count)
            || !self.video_seconds.is_finite()
            || !(1.0..=600.0).contains(&self.video_seconds)
            || !(1..=8).contains(&self.max_workers)
            || !(1..=5).contains(&self.max_attempts)
            || !(5..=86400).contains(&self.retry_delay_seconds)
            || self.providers.text_model.trim().is_empty()
            || self.providers.assets.len() > 100
            || self.facts.len() > 100
            || self.providers.allowed_fallbacks.len() > 20
        {
            return Err(
                "Kế hoạch thiếu thông tin hoặc vượt giới hạn 1–1.000 video, 1–8 worker".into(),
            );
        }
        self.timezone
            .parse::<chrono_tz::Tz>()
            .map_err(|_| "Múi giờ IANA không hợp lệ")?;
        let start = NaiveDate::parse_from_str(&self.start_date, "%Y-%m-%d")
            .map_err(|_| "Ngày bắt đầu không hợp lệ")?;
        let end = NaiveDate::parse_from_str(&self.end_date, "%Y-%m-%d")
            .map_err(|_| "Ngày kết thúc không hợp lệ")?;
        if end < start || (end - start).num_days() > 3660 {
            return Err("Khoảng ngày chiến dịch không hợp lệ".into());
        }
        if self.publish && self.pages.is_empty() {
            return Err("Chọn Facebook Page đã được cấp quyền trước khi bật đăng bài".into());
        }
        if self.pages.len() > 100 {
            return Err("Tối đa 100 Page trong một chiến dịch".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        for page in &self.pages {
            if !decimal_id(&page.page_id)
                || !ids.insert(&page.page_id)
                || page.name.len() > 500
                || !(1..=100).contains(&page.daily_limit)
                || page.slots.is_empty()
                || page.slots.len() > 100
                || page.topics.len() > 100
            {
                return Err("Page, hạn mức hoặc khung giờ không hợp lệ".into());
            }
            let mut slots = std::collections::BTreeSet::new();
            for slot in &page.slots {
                if slot.len() != 5
                    || chrono::NaiveTime::parse_from_str(slot, "%H:%M").is_err()
                    || !slots.insert(slot)
                {
                    return Err("Giờ đăng phải là HH:MM và không trùng".into());
                }
            }
        }
        if self.providers.uses_grok() && self.providers.grok_profile.trim().is_empty() {
            return Err("Không cần tải ảnh lên. Chọn hồ sơ Nexora có Grok để AI tự tạo ảnh và video".into());
        }
        for path in self
            .providers
            .anchor
            .iter()
            .chain(self.providers.assets.iter())
        {
            if !path.is_absolute() {
                return Err("Tư liệu phải có đường dẫn tuyệt đối đã được người dùng chọn".into());
            }
        }
        if self.providers.render_reserve_seconds == 0
            || self.render_budget_seconds
                < self
                    .providers
                    .render_reserve_seconds
                    .saturating_mul(self.video_count as u64)
        {
            return Err("Ngân sách render không đủ phần dự trù đã khai báo".into());
        }
        let estimate = self
            .providers
            .text_cost_micros
            .saturating_add(if self.providers.uses_grok() {
                self.providers.media_cost_micros
            } else {
                0
            })
            .saturating_mul(self.video_count as u64);
        if estimate > self.api_budget_micros {
            return Err("Ngân sách API thấp hơn chi phí dự trù".into());
        }
        if self
            .locked_fields
            .iter()
            .any(|field| !Self::policy_fields().contains(&field.as_str()))
        {
            return Err("Tên trường khóa không hợp lệ".into());
        }
        Ok(())
    }
    pub fn hash(&self) -> String {
        digest(&serde_json::to_value(self).expect("Plan serialization"))
    }
    pub fn policy_fields() -> &'static [&'static str] {
        &[
            "objective",
            "video_count",
            "video_seconds",
            "topic",
            "language",
            "style",
            "unique_content",
            "facts",
            "pages",
            "publish",
            "timezone",
            "start_date",
            "end_date",
            "completion_deadline_utc",
            "allow_cross_page",
            "api_budget_micros",
            "render_budget_seconds",
            "max_workers",
            "mode",
            "approve_content",
            "approve_publish",
            "max_attempts",
            "retry_delay_seconds",
            "providers",
            "locked_fields",
        ]
    }
}
pub fn decimal_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_digit())
}
/// Verify explicit numeric targets without interpreting a campaign deadline as
/// the length of a clip. Other natural-language choices remain reviewable.
pub fn check_request_targets(request: &str, plan: &Plan) -> Result<(), String> {
    let lower = request.to_lowercase();
    for (offset, _) in lower.match_indices("video") {
        let before = lower[..offset].trim_end();
        let number = before
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit() || matches!(c, '.' | ','))
            .collect::<String>()
            .chars()
            .rev()
            .filter(|c| c.is_ascii_digit())
            .collect::<String>();
        if let Ok(count) = number.parse::<u32>() {
            if count != plan.video_count {
                return Err(format!(
                    "AI trả {} video nhưng yêu cầu là {count}. Chưa chấp nhận bản nháp.",
                    plan.video_count
                ));
            }
        }
    }
    if let Some((_, after)) = lower.split_once("mỗi video") {
        let chars: Vec<char> = after.chars().take(120).collect();
        let mut i = 0;
        while i < chars.len() && !chars[i].is_ascii_digit() {
            i += 1;
        }
        let mut seconds = 0.0;
        let mut found = false;
        loop {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || matches!(chars[i], '.' | ',')) {
                i += 1;
            }
            let n = chars[start..i]
                .iter()
                .collect::<String>()
                .replace(',', ".")
                .parse::<f64>()
                .ok();
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            let start = i;
            while i < chars.len() && (chars[i].is_alphabetic()) {
                i += 1;
            }
            let unit = chars[start..i].iter().collect::<String>();
            let factor = if matches!(unit.as_str(), "giây" | "s" | "sec" | "seconds") {
                Some(1.0)
            } else if matches!(unit.as_str(), "phút" | "min" | "minutes") {
                Some(60.0)
            } else {
                None
            };
            if let (Some(n), Some(factor)) = (n, factor) {
                seconds += n * factor;
                found = true;
            } else {
                break;
            }
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            if i == chars.len() || !chars[i].is_ascii_digit() {
                break;
            }
        }
        if found && (!seconds.is_finite() || (seconds - plan.video_seconds).abs() > 0.01) {
            return Err("AI đã thay thời lượng video. Thời hạn chiến dịch không được cộng vào độ dài mỗi video.".into());
        }
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CampaignState {
    Draft,
    Approved,
    Running,
    Paused,
    Cancelled,
    Completed,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Content,
    Media,
    Render,
    Quality,
    Publish,
    Done,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Pending,
    Running,
    NeedsApproval,
    NeedsReview,
    RetryWait,
    ReadyToPublish,
    Published,
    Completed,
    Failed,
    Unknown,
    Cancelled,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlanVersion {
    pub revision: u64,
    pub plan: Plan,
    pub hash: String,
    pub created_at: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Script {
    pub idea: String,
    pub topic: String,
    pub audience: String,
    pub hook: String,
    pub narration: String,
    pub title: String,
    pub caption: String,
    pub hashtags: Vec<String>,
    pub image_prompt: String,
    pub motion_prompt: String,
    pub originality: Option<Originality>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Originality {
    pub model: String,
    pub compared_video_ids: Vec<String>,
    pub is_distinct: bool,
    pub reason: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Asset {
    pub path: PathBuf,
    pub sha256: String,
    pub rights: String,
    pub provider: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QualityReport {
    pub sha256: String,
    pub seconds: f64,
    pub width: u32,
    pub height: u32,
    pub audio: bool,
    pub audible: bool,
    pub decoded: bool,
    pub subtitles_in_frame: bool,
    pub checked_at: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Lease {
    pub token: String,
    pub worker: String,
    pub expires_at: i64,
    pub plan_revision: u64,
    pub job_revision: u64,
    pub reserved_micros: u64,
    pub reserved_seconds: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VideoJob {
    pub id: String,
    pub ordinal: u32,
    pub revision: u64,
    pub stage: Stage,
    pub state: JobState,
    pub attempt: u8,
    pub retry_at: i64,
    pub dispatch_id: Option<String>,
    pub script: Option<Script>,
    #[serde(default)]
    pub script_locked: bool,
    #[serde(default)]
    pub script_versions: Vec<Script>,
    #[serde(default)]
    pub repair_attempts: u8,
    #[serde(default)]
    pub timing_repairs: u8,
    pub media: Vec<Asset>,
    pub rendered: Option<Asset>,
    pub quality: Option<QualityReport>,
    pub hold: bool,
    pub approval_revision: Option<u64>,
    pub lease: Option<Lease>,
    pub page_id: Option<String>,
    pub slot_utc: Option<i64>,
    pub upload_id: Option<String>,
    pub upload_url: Option<String>,
    pub publish_dispatched: bool,
    pub post_id: Option<String>,
    pub post_url: Option<String>,
    pub published_at: Option<i64>,
    pub error: Option<String>,
    pub actual_micros: u64,
    pub render_seconds: u64,
}
impl VideoJob {
    pub fn new(index: u32) -> Self {
        Self {
            id: format!("video-{:05}", index + 1),
            ordinal: index + 1,
            revision: 0,
            stage: Stage::Content,
            state: JobState::Pending,
            attempt: 0,
            retry_at: 0,
            dispatch_id: None,
            script: None,
            script_locked: false,
            script_versions: vec![],
            repair_attempts: 0,
            timing_repairs: 0,
            media: vec![],
            rendered: None,
            quality: None,
            hold: false,
            approval_revision: None,
            lease: None,
            page_id: None,
            slot_utc: None,
            upload_id: None,
            upload_url: None,
            publish_dispatched: false,
            post_id: None,
            post_url: None,
            published_at: None,
            error: None,
            actual_micros: 0,
            render_seconds: 0,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublishingSlot {
    pub page_id: String,
    pub utc: i64,
    pub local_date: String,
    pub video_id: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuditEvent {
    pub at: i64,
    pub actor: String,
    pub action: String,
    pub video_id: Option<String>,
    pub detail: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UsageRecord {
    pub at: i64,
    pub video_id: String,
    pub stage: Stage,
    pub micros: u64,
    pub render_seconds: u64,
    pub estimated: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub schema: u32,
    pub id: String,
    pub revision: u64,
    pub state: CampaignState,
    pub versions: Vec<PlanVersion>,
    pub approved_hash: Option<String>,
    pub jobs: Vec<VideoJob>,
    pub slots: Vec<PublishingSlot>,
    pub audit: Vec<AuditEvent>,
    pub usage: Vec<UsageRecord>,
    pub created_at: i64,
}
impl Record {
    pub fn plan(&self) -> &Plan {
        &self
            .versions
            .last()
            .expect("Campaign requires a version")
            .plan
    }
    pub fn plan_revision(&self) -> u64 {
        self.versions.last().expect("Campaign version").revision
    }
    pub fn audit(
        &mut self,
        actor: &str,
        action: &str,
        video: Option<&str>,
        detail: impl Into<String>,
        at: i64,
    ) {
        self.audit.push(AuditEvent {
            at,
            actor: actor.into(),
            action: action.into(),
            video_id: video.map(Into::into),
            detail: detail.into(),
        });
        self.revision += 1;
    }
    pub fn counts(&self) -> std::collections::BTreeMap<String, usize> {
        let mut c = std::collections::BTreeMap::new();
        c.insert("target".into(), self.plan().video_count as usize);
        for job in &self.jobs {
            if job.script.is_some() {
                *c.entry("scripted".into()).or_default() += 1;
            }
            if !job.media.is_empty() {
                *c.entry("media".into()).or_default() += 1;
            }
            if job.rendered.is_some() {
                *c.entry("rendered".into()).or_default() += 1;
            }
            if job.quality.is_some() {
                *c.entry("quality_checked".into()).or_default() += 1;
            }
            if job.slot_utc.is_some() {
                *c.entry("scheduled".into()).or_default() += 1;
            }
            if job.state == JobState::Published && job.post_id.is_some() && job.post_url.is_some() {
                *c.entry("published".into()).or_default() += 1;
            }
            if job.state == JobState::Failed {
                *c.entry("failed".into()).or_default() += 1;
            }
            if matches!(
                job.state,
                JobState::Unknown | JobState::NeedsReview | JobState::NeedsApproval
            ) {
                *c.entry("needs_action".into()).or_default() += 1;
            }
        }
        c
    }
}
/// Full prior content is partitioned, never shortened into word substitutions.
/// Both the budget reservation and semantic reviewer use these exact groups.
pub fn novelty_groups(record: &Record, video_id: &str) -> Vec<Vec<serde_json::Value>> {
    let mut groups = vec![];
    let mut group = vec![];
    let mut bytes = 0;
    for job in record.jobs.iter().filter(|j| j.id != video_id) {
        if let Some(script) = &job.script {
            let value = serde_json::json!({"video_id":job.id,"idea":script.idea,"narration":script.narration});
            let size = value.to_string().len();
            if bytes + size > 60000 && !group.is_empty() {
                groups.push(group);
                group = vec![];
                bytes = 0;
            }
            bytes += size;
            group.push(value);
        }
    }
    if !group.is_empty() {
        groups.push(group);
    }
    groups
}
