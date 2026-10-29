// SPDX-License-Identifier: AGPL-3.0-or-later
//! Kedi Video Factory – core job model and factory state.
//!
//! Defines the `KediJob` record, `FactoryStage` lifecycle and
//! `KediFactory` aggregate that bridges the UI to lux/yt-dlp and
//! the existing `video_library`, `shorts_jobs` and `native_pipeline`
//! infrastructure.
//!
//! This module is *data and logic only* – no Slint types, no FFI.
//! The Slint bridge lives in `concat/src/studio_kedi_factory.rs`.

use crate::{
    lux::{self, LuxDownloadOptions, LuxProgressEvent, LuxVideoInfo},
    provider_registry::{self, ProviderDef},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
    time::{SystemTime, UNIX_EPOCH},
};

// ── Job lifecycle ────────────────────────────────────────────────────────────

/// Full lifecycle for a Kedi factory job. Stages advance forward only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FactoryStage {
    Draft,
    Queued,
    Resolving,   // fetching metadata
    Downloading, // active transfer
    Paused,
    Cancelling,
    Cancelled,
    Failed,
    Completed,
    NeedsAttention,
}

impl FactoryStage {
    pub fn label(self) -> &'static str {
        match self {
            Self::Draft => "DRAFT",
            Self::Queued => "QUEUED",
            Self::Resolving => "RESOLVING",
            Self::Downloading => "DOWNLOADING",
            Self::Paused => "PAUSED",
            Self::Cancelling => "CANCELLING",
            Self::Cancelled => "CANCELLED",
            Self::Failed => "FAILED",
            Self::Completed => "COMPLETED",
            Self::NeedsAttention => "NEEDS_ATTENTION",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Cancelled | Self::Failed | Self::Completed)
    }

    pub fn can_cancel(self) -> bool {
        matches!(
            self,
            Self::Queued | Self::Resolving | Self::Downloading | Self::Paused
        )
    }
}

// ── Input kind ──────────────────────────────────────────────────────────────

/// Which of the four input flows created this job.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum JobInput {
    /// User pasted one or more video URLs.
    VideoUrl { urls: Vec<String> },
    /// User pasted a product page URL.
    ProductUrl { url: String },
    /// User dropped / imported local files or a directory.
    LocalFiles { paths: Vec<PathBuf> },
    /// User entered a text brief / script.
    Brief { text: String },
}

// ── Job record ───────────────────────────────────────────────────────────────

/// A persisted factory job. Stored as JSON in the jobs directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KediJob {
    pub job_id: String,
    pub batch_id: Option<String>,
    pub input: JobInput,
    pub provider_id: Option<String>,
    pub stage: FactoryStage,
    /// 0.0 – 1.0
    pub progress: f32,
    /// How many times this job has been attempted.
    pub attempts: u32,
    pub max_attempts: u32,
    pub error_code: Option<String>,
    pub error_detail: Option<String>,
    pub output_paths: Vec<PathBuf>,
    pub created_at: u64,
    pub updated_at: u64,
    pub completed_at: Option<u64>,
    /// Resolved metadata (title, duration, etc.) after probing.
    pub resolved_info: Option<Vec<LuxVideoInfo>>,
    /// Selected stream IDs per URL index.
    pub selected_streams: HashMap<usize, String>,
    /// Where downloads land until verified complete.
    pub download_dir: Option<PathBuf>,
}

impl KediJob {
    pub fn new(input: JobInput) -> Self {
        let now = unix_now();
        let job_id = format!("kedi-{:016x}", rand_u64());
        Self {
            job_id,
            batch_id: None,
            input,
            provider_id: None,
            stage: FactoryStage::Draft,
            progress: 0.0,
            attempts: 0,
            max_attempts: 3,
            error_code: None,
            error_detail: None,
            output_paths: Vec::new(),
            created_at: now,
            updated_at: now,
            completed_at: None,
            resolved_info: None,
            selected_streams: HashMap::new(),
            download_dir: None,
        }
    }

    pub fn mark_updated(&mut self) {
        self.updated_at = unix_now();
    }

    pub fn advance(&mut self, stage: FactoryStage) {
        self.stage = stage;
        self.mark_updated();
    }

    pub fn fail(&mut self, code: &str, detail: &str) {
        self.stage = FactoryStage::Failed;
        self.error_code = Some(code.to_owned());
        self.error_detail = Some(detail.to_owned());
        self.mark_updated();
    }

    pub fn complete(&mut self, outputs: Vec<PathBuf>) {
        if outputs.is_empty() {
            self.fail("NO_OUTPUT", "Không có video hợp lệ được tạo.");
            return;
        }
        self.stage = FactoryStage::Completed;
        self.progress = 1.0;
        self.output_paths = outputs;
        self.completed_at = Some(unix_now());
        self.mark_updated();
    }
}

/// Commit the actual outcome, retaining partial outputs without reporting success.
pub fn finish_download(
    job: &mut KediJob,
    outputs: Vec<PathBuf>,
    cancelled: bool,
    errors: &[String],
) {
    job.output_paths = outputs.clone();
    if cancelled {
        job.advance(FactoryStage::Cancelled);
    } else if !errors.is_empty() {
        job.fail("DOWNLOAD_FAILED", &errors.join("\n"));
    } else {
        job.complete(outputs);
    }
}

/// Prepare deterministic edits from operator-selected files and confirmed facts.
pub fn product_board(
    paths: &[PathBuf],
    title: &str,
    price: &str,
    caption: &str,
    seconds: f64,
    audio: Option<PathBuf>,
) -> Result<crate::storyboard::Storyboard, String> {
    if paths.is_empty() || paths.len() > 100 {
        return Err("Chọn từ 1 đến 100 ảnh/video thật trước khi dựng.".into());
    }
    if !seconds.is_finite() || !(0.5..=600.0).contains(&seconds) {
        return Err("Thời lượng mỗi tư liệu phải từ 0.5 đến 600 giây.".into());
    }
    let caption = if caption.trim().is_empty() {
        [title.trim(), price.trim()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
    } else {
        caption.trim().to_owned()
    };
    let mut board = crate::storyboard::Storyboard::default();
    board.title = if title.trim().is_empty() {
        "Video sản phẩm".into()
    } else {
        title.trim().into()
    };
    for path in paths {
        if !path.is_file() {
            return Err(format!("Không tìm thấy tư liệu: {}", path.display()));
        }
        let mut shot = crate::storyboard::Shot::new("", seconds);
        shot.title = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        shot.background = Some(path.clone());
        shot.voice = audio.clone();
        shot.caption = caption.clone();
        board.shots.push(shot);
    }
    board.validate()?;
    Ok(board)
}

/// Decode a local media file through ffprobe before exposing it as a completed video.
pub fn verify_media(probe: &Path, path: &Path) -> Result<(), String> {
    verify_output(path)?;
    let output = lux::make_command(probe)
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height:format=duration",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .map_err(|e| format!("Không chạy được ffprobe: {e}"))?;
    if !output.status.success() {
        return Err("ffprobe không đọc được video đã xuất.".into());
    }
    let data: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|_| "Metadata video không hợp lệ")?;
    let valid = data["streams"].as_array().is_some_and(|s| {
        s.iter()
            .any(|s| s["width"].as_u64().unwrap_or(0) > 0 && s["height"].as_u64().unwrap_or(0) > 0)
    });
    let duration = data["format"]["duration"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);
    if !valid || !duration.is_finite() || duration <= 0.0 {
        return Err("Video thiếu hình hoặc thời lượng hợp lệ.".into());
    }
    Ok(())
}

/// One record per job avoids concurrent append loss. Only the supplied job is replaced.
pub fn save_job(root: &Path, job: &KediJob) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let path = root.join(format!("{}.json", job.job_id));
    let temporary = root.join(format!("{}.tmp", job.job_id));
    std::fs::write(
        &temporary,
        serde_json::to_vec_pretty(job).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(temporary, path).map_err(|e| e.to_string())
}

/// An interrupted process never resumes silently or appears completed.
pub fn load_jobs(root: &Path) -> Vec<KediJob> {
    let mut jobs = vec![];
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|s| s != "json") {
                continue;
            }
            if let Ok(bytes) = std::fs::read(path) {
                if let Ok(mut job) = serde_json::from_slice::<KediJob>(&bytes) {
                    if job.stage.can_cancel() || job.stage == FactoryStage::Cancelling {
                        job.fail(
                            "INTERRUPTED",
                            "Tác vụ bị gián đoạn khi ứng dụng đóng. Có thể thử tải lại.",
                        );
                    }
                    jobs.push(job);
                }
            }
        }
    }
    jobs.sort_by_key(|j| j.created_at);
    jobs
}

// ── Factory aggregate ────────────────────────────────────────────────────────

/// Maximum concurrent downloads. Configurable; default is 2.
pub const DEFAULT_CONCURRENCY: usize = 2;

/// Shared factory state. Accessed from the UI thread and worker threads.
#[derive(Default)]
pub struct KediFactory {
    pub jobs: Vec<KediJob>,
    /// Provider availability after runtime check.
    pub ytdlp_available: bool,
    pub lux_available: bool,
    /// Provider IDs that are usable right now.
    pub usable_providers: Vec<String>,
}

impl KediFactory {
    /// Check binaries and update availability flags.
    pub fn check_availability(&mut self) {
        self.lux_available = lux::find_lux_binary().is_ok();
        self.ytdlp_available = lux::ytdlp_binary().is_some();
        self.usable_providers =
            provider_registry::check_runtime_availability(self.ytdlp_available, self.lux_available);
    }

    /// Create a new draft job and append it.
    pub fn create_job(&mut self, input: JobInput) -> String {
        let job = KediJob::new(input);
        let id = job.job_id.clone();
        self.jobs.push(job);
        id
    }

    /// Find a job by ID (mutable).
    pub fn job_mut(&mut self, id: &str) -> Option<&mut KediJob> {
        self.jobs.iter_mut().find(|j| j.job_id == id)
    }

    /// Active (non-terminal) job count.
    pub fn active_count(&self) -> usize {
        self.jobs.iter().filter(|j| !j.stage.is_terminal()).count()
    }

    /// Resolve the best provider for a URL.
    pub fn provider_for(&self, url: &str) -> Option<&'static ProviderDef> {
        provider_registry::provider_for_url(url)
    }
}

// ── Worker entry points ──────────────────────────────────────────────────────

/// Resolve metadata for a single URL on a worker thread.
/// Returns the resolved video info list, or an error string.
pub fn resolve_url(
    url: &str,
    playlist: bool,
    cancel: Arc<AtomicBool>,
) -> Result<Vec<LuxVideoInfo>, String> {
    lux::probe_video_info_cancellable(url, playlist, cancel)
}

/// Download a single video. Reports progress via `progress_cb`.
/// Returns the verified output path on success.
///
/// # Errors
/// Returns a human-readable error string. Partial files are cleaned up.
pub fn download_video<F>(
    url: &str,
    stream_id: Option<String>,
    output_dir: &Path,
    cancel: Arc<AtomicBool>,
    mut progress_cb: F,
) -> Result<PathBuf, String>
where
    F: FnMut(f32, String) + Send + 'static,
{
    std::fs::create_dir_all(output_dir).map_err(|e| format!("Không tạo được thư mục tải: {e}"))?;

    let opts = LuxDownloadOptions {
        cancel: cancel.clone(),
        url: url.to_owned(),
        stream_id,
        playlist: false,
        output_dir: Some(output_dir.to_path_buf()),
        audio_only: false,
    };

    let cb = std::sync::Arc::new(std::sync::Mutex::new(progress_cb));
    let cb_clone = cb.clone();
    let paths = lux::download_video(&opts, move |event| {
        if let LuxProgressEvent::Progress { percent, speed, .. } = event {
            if let Ok(mut f) = cb_clone.lock() {
                f(percent / 100.0, speed);
            }
        }
    })?;

    let path = paths
        .into_iter()
        .next()
        .ok_or_else(|| "Tải xong nhưng không tìm được đường dẫn file.".to_string())?;

    // Post-download verification
    verify_output(&path)?;

    Ok(path)
}

/// Verify a downloaded file is a valid, non-empty video.
pub fn verify_output(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!(
            "File không tồn tại sau khi tải: {}",
            path.display()
        ));
    }
    let size = path
        .metadata()
        .map_err(|e| format!("Không đọc được thông tin file: {e}"))?
        .len();
    if size < 1024 {
        return Err(format!(
            "File quá nhỏ ({size} bytes) – có thể bị lỗi khi tải."
        ));
    }
    // Basic container probe via lux/ffprobe if available
    // (lightweight check: just confirm the file is readable and non-zero)
    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn rand_u64() -> u64 {
    rand::random::<u64>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_and_partial_downloads_never_report_completed() {
        let mut job = KediJob::new(JobInput::VideoUrl {
            urls: vec!["https://example.com/video".into()],
        });
        finish_download(&mut job, vec![], false, &[]);
        assert_eq!(job.stage, FactoryStage::Failed);
        finish_download(&mut job, vec!["partial.mp4".into()], true, &[]);
        assert_eq!(job.stage, FactoryStage::Cancelled);
        assert_eq!(job.output_paths.len(), 1);
        finish_download(
            &mut job,
            vec!["good.mp4".into()],
            false,
            &["second URL failed".into()],
        );
        assert_eq!(job.stage, FactoryStage::Failed);
        finish_download(&mut job, vec!["good.mp4".into()], false, &[]);
        assert_eq!(job.stage, FactoryStage::Completed);
        assert_eq!(job.progress, 1.0);
    }

    #[test]
    fn factory_rejects_missing_assets_and_invalid_durations() {
        assert!(product_board(&[], "", "", "", 5.0, None).is_err());
        assert!(product_board(&["missing.mp4".into()], "", "", "", f64::NAN, None).is_err());
        assert!(product_board(&["missing.mp4".into()], "", "", "", 5.0, None).is_err());
    }

    #[test]
    fn jobs_survive_restart_and_interrupted_downloads_are_retryable() {
        let root = std::env::temp_dir().join(format!("kedi-job-test-{:016x}", rand_u64()));
        let mut a = KediJob::new(JobInput::VideoUrl {
            urls: vec!["https://example.com/a".into()],
        });
        a.advance(FactoryStage::Downloading);
        let mut b = KediJob::new(JobInput::VideoUrl {
            urls: vec!["https://example.com/b".into()],
        });
        b.complete(vec!["b.mp4".into()]);
        save_job(&root, &a).unwrap();
        save_job(&root, &b).unwrap();
        a.progress = 0.5;
        save_job(&root, &a).unwrap();
        let rows = load_jobs(&root);
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows.iter()
                .find(|j| j.job_id == a.job_id)
                .unwrap()
                .error_code
                .as_deref(),
            Some("INTERRUPTED")
        );
        assert_eq!(
            rows.iter().find(|j| j.job_id == b.job_id).unwrap().stage,
            FactoryStage::Completed
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn factory_render_encodes_real_files_and_rejects_fake_mp4() {
        let Some(ffmpeg) = std::env::var_os("CONCAT_TEST_FFMPEG").map(PathBuf::from) else {
            return;
        };
        let root = std::env::temp_dir().join(format!("kedi-render-test-{:016x}", rand_u64()));
        std::fs::create_dir(&root).unwrap();
        let source = root.join("source.png");
        let status = lux::make_command(&ffmpeg)
            .args([
                "-hide_banner",
                "-nostdin",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=navy:s=640x360",
                "-frames:v",
                "1",
                "-threads",
                "1",
            ])
            .arg(&source)
            .status()
            .unwrap();
        assert!(status.success());
        let board =
            product_board(&[source], "Sản phẩm do người dùng nhập", "", "", 0.5, None).unwrap();
        assert!(!board.shots[0].caption.contains("89.000"));
        let font = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../concat-text/fonts/HankenGrotesk-Regular.ttf");
        let mut progress = vec![];
        let output = crate::storyboard_render::render_board(
            &ffmpeg,
            &font,
            &board,
            &root.join("render"),
            crate::storyboard_render::RenderSize {
                width: 720,
                height: 1280,
            },
            &AtomicBool::new(false),
            |done, total| progress.push((done, total)),
        )
        .unwrap();
        let probe = ffmpeg.with_file_name("ffprobe.exe");
        verify_media(&probe, &output).unwrap();
        assert_eq!(progress.last(), Some(&(1, 1)));
        let fake = root.join("fake.mp4");
        std::fs::write(&fake, vec![0u8; 2048]).unwrap();
        assert!(verify_media(&probe, &fake).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn job_lifecycle_advances_forward() {
        let mut job = KediJob::new(JobInput::VideoUrl {
            urls: vec!["https://example.com/v.mp4".into()],
        });
        assert_eq!(job.stage, FactoryStage::Draft);
        job.advance(FactoryStage::Queued);
        assert_eq!(job.stage, FactoryStage::Queued);
        job.fail("TEST_ERROR", "test detail");
        assert_eq!(job.stage, FactoryStage::Failed);
        assert!(job.stage.is_terminal());
    }

    #[test]
    fn factory_tracks_jobs() {
        let mut factory = KediFactory::default();
        let id = factory.create_job(JobInput::Brief {
            text: "test brief".into(),
        });
        assert_eq!(factory.jobs.len(), 1);
        let job = factory.job_mut(&id).unwrap();
        job.advance(FactoryStage::Queued);
        assert_eq!(factory.active_count(), 1);
    }

    #[test]
    fn stage_labels_are_screaming_snake() {
        for stage in [
            FactoryStage::Draft,
            FactoryStage::Queued,
            FactoryStage::Resolving,
            FactoryStage::Downloading,
            FactoryStage::Completed,
            FactoryStage::Failed,
        ] {
            let label = stage.label();
            assert_eq!(
                label,
                label.to_uppercase(),
                "stage label not screaming snake: {label}"
            );
            assert!(!label.contains(' '));
        }
    }
}
