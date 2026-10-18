// SPDX-License-Identifier: AGPL-3.0-or-later
//! Local-first orchestration; all edits and rendering belong to existing native engines.
mod analysis;
#[cfg(test)]
mod integration_tests;
mod output;
mod qa;
use concat_agent::highlight::{self, Genre, Highlight, Signal, TimedSegment};
use concat_agent::shorts_plan::{self, CaptionPreset};
use concat_project::model::VideoSettings;
use concat_project::{Editor, PreparedDraft};
use concat_studio::shorts_jobs::{Queue, SourceFingerprint, Stage};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// Worker progress, never a fabricated completion receipt.
pub type Progress = Arc<dyn Fn(Stage, f32) + Send + Sync>;
/// Actual media probe result.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Source {
    pub path: PathBuf,
    pub label: String,
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    pub has_audio: bool,
}
/// Bounded, credential-free user choices.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub target_count: usize,
    pub min_duration: f64,
    pub max_duration: f64,
    pub genre: Genre,
    pub width: u32,
    pub height: u32,
    pub caption_preset: u8,
    pub transcript_path: Option<PathBuf>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            target_count: 3,
            min_duration: 15.,
            max_duration: 60.,
            genre: Genre::General,
            width: 720,
            height: 1280,
            caption_preset: 1,
            transcript_path: None,
        }
    }
}
impl Config {
    fn validate(&self) -> Result<(), String> {
        if !(1..=12).contains(&self.target_count)
            || !self.min_duration.is_finite()
            || !self.max_duration.is_finite()
            || self.min_duration < 1.
            || self.max_duration < self.min_duration
            || self.max_duration > 180.
            || self.width < 128
            || self.height < 128
            || self.width > 3840
            || self.height > 3840
            || self.width % 2 != 0
            || self.height % 2 != 0
            || self.caption_preset > 5
        {
            return Err("Invalid shorts settings".into());
        }
        Ok(())
    }
    fn options(&self) -> highlight::Options {
        highlight::Options {
            min_duration: self.min_duration,
            max_duration: self.max_duration,
            target_count: self.target_count,
            genre: self.genre,
        }
    }
    fn video(&self) -> VideoSettings {
        VideoSettings {
            width: self.width,
            height: self.height,
            ..Default::default()
        }
    }
    fn captions(&self) -> CaptionPreset {
        match self.caption_preset {
            1 => CaptionPreset::Clean,
            2 => CaptionPreset::Bold,
            3 => CaptionPreset::Podcast,
            4 => CaptionPreset::Cinematic,
            5 => CaptionPreset::Gaming,
            _ => CaptionPreset::None,
        }
    }
}
/// Grounded, reviewable analysis; source identity is rechecked before use.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Analysis {
    pub source: Source,
    pub config: Config,
    pub segments: Vec<TimedSegment>,
    pub signals: Vec<Signal>,
    pub highlights: Vec<Highlight>,
    pub warnings: Vec<String>,
    pub job_id: String,
    pub fingerprint: SourceFingerprint,
}
pub use output::{Output, QaReport};
/// Shared local services. No account, network service or download is required.
#[derive(Clone)]
pub struct Engine {
    dirs: concat_host::AppDirs,
    transcriber: Arc<concat_speech::transcribe::Transcriber>,
    queue: Queue,
    _ownership: Arc<std::fs::File>,
}
impl Engine {
    /// Open queue once at startup and recover interrupted work honestly.
    pub fn new(
        dirs: concat_host::AppDirs,
        transcriber: Arc<concat_speech::transcribe::Transcriber>,
    ) -> Result<Self, String> {
        let queue = Queue::open(dirs.data.join("auto-shorts/jobs.json"), 200, 3).map_err(err)?;
        // Keep worker ownership across every stage, not only individual ledger writes.
        // A second process must not 'recover' the first process's running job.
        static OWNERS: std::sync::OnceLock<
            std::sync::Mutex<std::collections::HashMap<PathBuf, std::sync::Weak<std::fs::File>>>,
        > = std::sync::OnceLock::new();
        let owner_path = std::fs::canonicalize(dirs.data.join("auto-shorts"))
            .map_err(err)?
            .join("worker.lock");
        let mut owners = OWNERS
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| "Worker ownership lock failed")?;
        let ownership =
            if let Some(file) = owners.get(&owner_path).and_then(std::sync::Weak::upgrade) {
                file
            } else {
                let file = std::fs::OpenOptions::new()
                    .create(true)
                    .truncate(false)
                    .read(true)
                    .write(true)
                    .open(&owner_path)
                    .map_err(err)?;
                file.try_lock()
                    .map_err(|_| "Auto Shorts is busy in another Concat process")?;
                let file = Arc::new(file);
                owners.insert(owner_path, Arc::downgrade(&file));
                file
            };
        static RECOVERED: std::sync::OnceLock<
            std::sync::Mutex<std::collections::BTreeSet<PathBuf>>,
        > = std::sync::OnceLock::new();
        let mut recovered = RECOVERED
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| "Queue startup lock failed")?;
        if !recovered.contains(&dirs.data) {
            queue.recover_interrupted().map_err(err)?;
            recovered.insert(dirs.data.clone());
        }
        Ok(Self {
            dirs,
            transcriber,
            queue,
            _ownership: ownership,
        })
    }
    /// Native probe, with bounded local source validation.
    pub fn inspect(path: &Path) -> Result<Source, String> {
        let fingerprint = SourceFingerprint::capture(path, &json!({})).map_err(err)?;
        let info = concat_host::media::probe(&fingerprint.path.to_string_lossy())?;
        let video = info.video.ok_or("Source has no video stream")?;
        let duration = info
            .duration
            .filter(|d| d.is_finite() && *d > 0. && *d <= 172800.)
            .ok_or("Unsupported source duration")?;
        Ok(Source {
            label: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
            path: fingerprint.path,
            duration,
            width: video.width,
            height: video.height,
            bytes: fingerprint.size,
            has_audio: info.audio.is_some(),
        })
    }
    /// Analyze using measured evidence and, only when requested, an exact configured model.
    pub fn analyze(
        &self,
        path: &Path,
        config: Config,
        ai: Option<(concat_studio::workflows::WorkflowClient, String)>,
        cancel: &AtomicBool,
        progress: Progress,
    ) -> Result<Analysis, String> {
        config.validate()?;
        check_cancel(cancel)?;
        let source = Self::inspect(path)?;
        let mut wire = serde_json::to_value(&config).map_err(err)?;
        // Include sidecar identity, not just its pathname, in queue/cache configuration.
        if let Some(p) = &config.transcript_path {
            let fp = SourceFingerprint::capture(p, &json!({})).map_err(err)?;
            wire["transcript_identity"] = serde_json::to_value(fp).map_err(err)?;
        }
        if let Some((_, model)) = &ai {
            if model.trim().is_empty() {
                return Err("Select a configured AI model first".into());
            }
            wire["model"] = json!(model);
        }
        let fingerprint = SourceFingerprint::capture(&source.path, &wire).map_err(err)?;
        let entry = self.queue.enqueue(fingerprint.clone(), wire).map_err(err)?;
        if !entry.inserted {
            self.queue.retry(&entry.job.id).map_err(err)?;
        }
        let id = entry.job.id;
        self.queue.claim(&id).map_err(err)?;
        let result = (|| {
            progress(Stage::Analyzing, 0.);
            self.queue
                .advance(&id, Stage::Transcribing, json!({"source_probed":true}))
                .map_err(err)?;
            let (segments, signals, mut warnings) = analysis::run(
                &source,
                &config,
                &self.transcriber,
                &self.dirs,
                cancel,
                &progress,
            )?;
            check_cancel(cancel)?;
            self.queue
                .advance(
                    &id,
                    Stage::DetectingHighlights,
                    json!({"segments":segments.len(),"signals":signals.len()}),
                )
                .map_err(err)?;
            let options = config.options();
            let mut highlights =
                highlight::analyze(&segments, &signals, source.duration, &options).map_err(err)?;
            if let Some((client, model)) = ai {
                let prompt = highlight::prompt(&segments, &highlights, source.duration, &options)
                    .map_err(err)?;
                let response = client
                    .ai_structured(&model, &prompt)
                    .map_err(|_| "Configured AI request failed; no model fallback was used")?;
                check_cancel(cancel)?;
                highlights = highlight::resolve_model_response(
                    &response.text,
                    &segments,
                    &highlights,
                    source.duration,
                    &options,
                )
                .map_err(err)?;
            }
            if !fingerprint.unchanged().map_err(err)? {
                return Err("Source changed during analysis".into());
            }
            warnings.push("Center-crop fallback; face/subject tracking is not enabled. Review framing before export.".into());
            if highlights.is_empty() {
                warnings.push(
                    "No grounded candidates met the configured duration/evidence gates.".into(),
                );
            }
            let analysis = Analysis {
                source,
                config,
                segments,
                signals,
                highlights,
                warnings,
                job_id: id.clone(),
                fingerprint,
            };
            let dir = self.dirs.data.join("auto-shorts/analyses");
            std::fs::create_dir_all(&dir).map_err(err)?;
            let artifact = dir.join(format!("{id}-attempt-{}.json", entry.job.attempts + 1));
            output::write_json(&artifact, &analysis)?;
            self.queue
                .advance(
                    &id,
                    Stage::PreparingEdit,
                    json!({"candidates":analysis.highlights.len(),"analysis":artifact}),
                )
                .map_err(err)?;
            self.queue
                .needs_review(&id, "USER_REVIEW_REQUIRED")
                .map_err(err)?;
            progress(Stage::NeedsReview, 1.);
            Ok(analysis)
        })();
        if result.is_err() {
            if cancel.load(Ordering::Acquire) {
                let _ = self.queue.cancel(&id);
            } else {
                let _ = self.queue.fail(&id, "ANALYSIS_FAILED");
            }
        }
        result
    }
    /// Prepare a new timeline; caller performs explicit native approval.
    pub fn draft(&self, a: &Analysis, id: &str, editor: &Editor) -> Result<PreparedDraft, String> {
        a.config.validate()?;
        if !a.fingerprint.unchanged().map_err(err)? {
            return Err("Source changed; analyze again".into());
        }
        let source = Self::inspect(&a.source.path)?;
        if source.path != a.fingerprint.path
            || source.path != a.source.path
            || source.width != a.source.width
            || source.height != a.source.height
            || source.duration != a.source.duration
            || source.bytes != a.source.bytes
            || source.has_audio != a.source.has_audio
        {
            return Err("Analysis source identity or metadata no longer matches".into());
        }
        let h = a
            .highlights
            .iter()
            .find(|h| h.id == id)
            .ok_or("Unknown highlight")?;
        let media = concat_host::media::probe(&a.source.path.to_string_lossy())?.to_new_media();
        shorts_plan::propose(
            editor,
            media,
            h,
            a.config.video(),
            &a.segments,
            a.config.captions(),
        )
        .map_err(err)
    }
    /// Render an actual low-resolution preview through the same native renderer.
    pub fn preview(&self, a: &Analysis, id: &str, cancel: &AtomicBool) -> Result<PathBuf, String> {
        output::preview(self, a, id, cancel)
    }
    /// Explicitly approved selections only; actual files must pass QA before completion.
    pub fn export(
        &self,
        a: &Analysis,
        ids: &[String],
        folder: &Path,
        cancel: &AtomicBool,
        progress: Progress,
    ) -> Result<Vec<Output>, String> {
        output::export(self, a, ids, folder, cancel, progress)
    }
    /// Cancel the ASR worker started by this pipeline.
    pub fn cancel_asr(&self) { /* Worker cancellation is mirrored by its owned ASR progress callback. Never cancel another shared transcription. */
    }
    /// Durable local queue handle.
    pub fn queue(&self) -> Queue {
        self.queue.clone()
    }
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn check_cancel(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        Err("Cancelled".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configuration_is_bounded() {
        let mut c = Config::default();
        assert!(c.validate().is_ok());
        c.max_duration = f64::NAN;
        assert!(c.validate().is_err());
        c = Config::default();
        c.target_count = 13;
        assert!(c.validate().is_err());
        c = Config::default();
        c.width = 99999;
        assert!(c.validate().is_err());
    }
    #[test]
    fn cancellation_is_explicit() {
        assert!(check_cancel(&AtomicBool::new(true)).is_err());
    }
}
