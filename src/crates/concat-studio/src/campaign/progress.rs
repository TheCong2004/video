//! Read-only stage presentation from durable jobs and unexpired worker leases.
use super::*;

/// One stage in the campaign execution path.
#[derive(Clone, Debug)]
pub struct ProgressStage {
    pub title: &'static str,
    pub state: &'static str,
    pub label: &'static str,
    pub detail: String,
    pub error: String,
}

/// A running campaign alone is not proof that a worker is executing a stage.
pub fn stages(record: &Record, now: i64) -> Vec<ProgressStage> {
    let total = record.jobs.len();
    [
        (Stage::Content, "Viết nội dung"),
        (Stage::Media, "Tạo / chọn tư liệu"),
        (Stage::Render, "Dựng video"),
        (Stage::Quality, "Kiểm tra video"),
        (Stage::Publish, "Đăng lên Page"),
    ]
    .into_iter()
    .filter(|(stage, _)| *stage != Stage::Publish || record.plan().publish)
    .map(|(stage, title)| {
        let complete = |job: &&VideoJob| match stage {
            Stage::Content => job.script.is_some(),
            Stage::Media => !job.media.is_empty(),
            Stage::Render => job.rendered.is_some(),
            Stage::Quality => job.quality.is_some(),
            Stage::Publish => job.state == JobState::Published && job.post_id.is_some() && job.post_url.is_some(),
            Stage::Done => false,
        };
        let done = record.jobs.iter().filter(complete).count();
        let current: Vec<_> = record.jobs.iter().filter(|job| job.stage == stage && !complete(job)).collect();
        let running = current.iter().filter(|job| job.state == JobState::Running && job.lease.as_ref().is_some_and(|lease| lease.expires_at > now)).count();
        let blocked: Vec<_> = current.iter().filter(|job| matches!(job.state, JobState::Unknown | JobState::Failed | JobState::NeedsReview | JobState::NeedsApproval) || job.hold).collect();
        let (state, label) = if running > 0 {
            ("running", "Đang xử lý")
        } else if record.state == CampaignState::Cancelled {
            ("cancelled", "Đã hủy")
        } else if !blocked.is_empty() {
            ("blocked", "Cần xử lý")
        } else if total > 0 && done == total {
            ("done", "Hoàn tất")
        } else if record.state == CampaignState::Paused {
            ("paused", "Tạm dừng")
        } else {
            ("waiting", "Đang chờ")
        };
        let error = blocked.iter().find_map(|job| job.error.as_deref()).unwrap_or_else(|| if blocked.is_empty() { "" } else { "Có video đang chờ duyệt hoặc đang được giữ để sửa." }).chars().take(320).collect();
        let detail = if running > 0 {
            format!("{running} video đang xử lý · {done}/{total} hoàn tất")
        } else if !blocked.is_empty() {
            format!("{} video cần xử lý · {done}/{total} hoàn tất", blocked.len())
        } else if current.iter().any(|job| job.state == JobState::Running) {
            format!("Chưa có tín hiệu worker còn hiệu lực · {done}/{total} hoàn tất")
        } else {
            format!("{done}/{total} video hoàn tất")
        };
        ProgressStage { title, state, label, detail, error }
    })
    .collect()
}
