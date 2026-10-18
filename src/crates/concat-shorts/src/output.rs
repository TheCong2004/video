// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native renderer, atomic deliverables, measured output QA and local publish packs.
use super::*;
use concat_core::time::Rational;
use concat_media::{DecodeOptions, Decoder, FrameSource, SeekableSource};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    time::{SystemTime, UNIX_EPOCH},
};

/// File-level checks, not a claim of semantic or platform quality.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QaReport {
    pub width: u32,
    pub height: u32,
    pub duration: f64,
    pub decoded_samples: usize,
    pub has_audio: bool,
    pub warnings: Vec<String>,
    pub full_decode: bool,
    pub audio_samples: u64,
    pub near_black_seconds: f64,
    pub longest_silence_seconds: f64,
    pub clipped_fraction: f64,
}
/// Deliverables from one actually rendered selection.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Output {
    pub video_path: PathBuf,
    pub cover_path: Option<PathBuf>,
    pub metadata_path: PathBuf,
    pub qa: QaReport,
    pub highlight_id: String,
}
fn unique() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    )
}
pub(crate) fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(err)?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("Artifact metadata exceeds size limit".into());
    }
    write_new(path, &bytes)
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if path.exists() {
        return Err("Refusing to overwrite an existing deliverable".into());
    }
    let tmp = path.with_extension(format!("{}.partial", unique()));
    let result = (|| {
        let mut f = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp)
            .map_err(err)?;
        f.write_all(bytes).map_err(err)?;
        f.sync_all().map_err(err)?;
        // Exclusive staging directory / Engine worker ownership; supports FAT/exFAT.
        if path.exists() {
            return Err("Refusing to overwrite an existing deliverable".into());
        }
        fs::rename(&tmp, path).map_err(err)?;
        Ok(())
    })();
    let _ = fs::remove_file(&tmp);
    result
}
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn scratch(engine: &Engine) -> Result<Scratch, String> {
    let parent = engine.dirs.data.join("auto-shorts/scratch");
    fs::create_dir_all(&parent).map_err(err)?;
    let p = parent.join(unique());
    fs::create_dir(&p).map_err(err)?;
    Ok(Scratch(p))
}
fn render(
    engine: &Engine,
    a: &Analysis,
    id: &str,
    path: &Path,
    cancel: &AtomicBool,
    progress: &Progress,
) -> Result<QaReport, String> {
    check_cancel(cancel)?;
    let owned = scratch(engine)?;
    let mut session = concat_host::Session::open(
        &owned.0.to_string_lossy(),
        concat_project::DocumentSettings {
            name: "Auto Shorts render".into(),
            width: a.config.width,
            height: a.config.height,
            rate_num: 30,
            rate_den: 1,
        },
    )?;
    let draft = engine.draft(a, id, &session.planning_snapshot())?;
    session.commit_draft(draft).map_err(err)?;
    // Caption rasters use an owned per-render cache, removed after the render joins.
    let dirs = concat_host::AppDirs {
        config: owned.0.join("config"),
        data: owned.0.join("data"),
    };
    let titles = concat_host::Titles::new(&dirs);
    let art = titles.clips(session.project(), a.config.width, a.config.height);
    let text_count = session
        .project()
        .active()
        .clips
        .iter()
        .filter(|c| c.kind == concat_project::model::ClipKind::Text)
        .count();
    if art.len() != text_count {
        return Err("Caption rasterization failed; output was not accepted".into());
    }
    let spec = concat_host::export::ExportSpec {
        output: path.to_string_lossy().into(),
        crf: 21,
        preset: "veryfast".into(),
        ..Default::default()
    };
    let request =
        concat_host::export::request(&session, &spec, art.into_iter().map(|t| t.clip).collect());
    concat_host::export::run(&request, cancel, |p| {
        let fraction = if p.total > 0 {
            p.frame as f32 / p.total as f32
        } else {
            0.
        };
        progress(Stage::Rendering, fraction.clamp(0., 1.));
    })?;
    check_cancel(cancel)?;
    let h = a
        .highlights
        .iter()
        .find(|h| h.id == id)
        .ok_or("Unknown highlight")?;
    crate::qa::inspect(
        path,
        h.duration(),
        a.config.width,
        a.config.height,
        a.source.has_audio,
        cancel,
    )
}
pub(crate) fn preview(
    engine: &Engine,
    a: &Analysis,
    id: &str,
    cancel: &AtomicBool,
) -> Result<PathBuf, String> {
    let mut low = a.clone();
    (low.config.width, low.config.height) = preview_dimensions(a.config.width, a.config.height);
    let dir = engine.dirs.data.join("auto-shorts/previews");
    fs::create_dir_all(&dir).map_err(err)?;
    // Retain at most eight owned previews. Never touch user output directories.
    let mut old: Vec<_> = fs::read_dir(&dir)
        .map_err(err)?
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name().to_string_lossy().starts_with("preview-")
                && e.path().extension().is_some_and(|x| x == "mp4")
        })
        .collect();
    old.sort_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
    let count = old.len().saturating_sub(7);
    for e in old.into_iter().take(count) {
        let _ = fs::remove_file(e.path());
    }
    let path = dir.join(format!("preview-{}.mp4", unique()));
    let p: Progress = Arc::new(|_, _| {});
    match render(engine, &low, id, &path, cancel, &p) {
        Ok(_) => Ok(path),
        Err(e) => {
            let _ = fs::remove_file(&path);
            Err(e)
        }
    }
}
pub(crate) fn export(
    engine: &Engine,
    a: &Analysis,
    ids: &[String],
    folder: &Path,
    cancel: &AtomicBool,
    progress: Progress,
) -> Result<Vec<Output>, String> {
    if ids.is_empty() || ids.len() > 12 {
        return Err("Select between one and twelve reviewed candidates".into());
    }
    let mut unique_ids = std::collections::BTreeSet::new();
    for id in ids {
        if !unique_ids.insert(id) || !a.highlights.iter().any(|h| &h.id == id) {
            return Err("Invalid highlight selection".into());
        }
    }
    check_cancel(cancel)?;
    if !folder.is_dir() {
        return Err("Choose an existing output directory".into());
    }
    let selected: Vec<_> = ids
        .iter()
        .filter_map(|id| a.highlights.iter().find(|h| &h.id == id))
        .collect();
    use sha2::Digest;
    let identity = sha2::Sha256::digest(
        serde_json::to_vec(&json!({"selected":selected,"captions":a.segments})).map_err(err)?,
    );
    let wire = json!({"operation":"native_export","analysis":a.job_id,"selection":ids,"evidence_digest":format!("{identity:x}"),"output":folder,"config":a.config});
    let fp = SourceFingerprint::capture(&a.source.path, &wire).map_err(err)?;
    if !a.fingerprint.unchanged().map_err(err)? {
        return Err("Source changed; analyze again".into());
    }
    let entry = engine.queue.enqueue(fp, wire).map_err(err)?;
    let job = entry.job.id;
    if !entry.inserted {
        engine.queue.retry(&job).map_err(err)?;
    }
    engine.queue.claim(&job).map_err(err)?;
    let result = (|| {
        engine
            .queue
            .advance(&job, Stage::Rendering, json!({"approved_selections":ids}))
            .map_err(err)?;
        let pending = folder.join(format!(".concat-shorts-{}.partial", unique()));
        fs::create_dir(&pending).map_err(err)?;
        let guard = Scratch(pending.clone());
        let mut outputs = Vec::new();
        for (index, id) in ids.iter().enumerate() {
            check_cancel(cancel)?;
            let h = a
                .highlights
                .iter()
                .find(|h| &h.id == id)
                .ok_or("Unknown highlight")?;
            let basename = format!("clip-{:02}", index + 1);
            let video = pending.join(format!("{basename}.mp4"));
            let qa = render(engine, a, id, &video, cancel, &progress)?;
            let cover_ratio = 720.0 / a.config.width.max(a.config.height) as f64;
            let cover_size = (
                ((a.config.width as f64 * cover_ratio).round() as u32 / 2 * 2).max(2),
                ((a.config.height as f64 * cover_ratio).round() as u32 / 2 * 2).max(2),
            );
            let mut decoder = Decoder::open(
                &video,
                &DecodeOptions {
                    size: Some(cover_size),
                    threads: 2,
                    ..Default::default()
                },
            )
            .map_err(err)?;
            decoder
                .seek(Rational::new((h.duration() * 0.25 * 1000.) as i64, 1000))
                .map_err(err)?;
            let frame = decoder
                .next_frame()
                .map_err(err)?
                .ok_or("Cover frame could not be decoded")?;
            let cover = pending.join(format!("{basename}-cover.jpg"));
            write_new(&cover, &concat_media::jpeg(&frame, 4).map_err(err)?)?;
            let meta = pending.join(format!("{basename}.json"));
            write_json(
                &meta,
                &json!({"source":a.fingerprint,"highlight":h,"qa":qa,"caption_timing":"source segment intervals; not word alignment","crop":"static center-fill fallback","publisher":"not configured","uploaded":false}),
            )?;
            let text = format!("{}\n\n{}\n\n{}\n", h.title, h.hook, h.reason);
            write_new(
                &pending.join(format!("{basename}-post.txt")),
                text.as_bytes(),
            )?;
            let cues = remap_cues(&a.segments, &h.parts);
            write_new(&pending.join(format!("{basename}.srt")), cues.as_bytes())?;
            outputs.push(Output {
                video_path: video,
                cover_path: Some(cover),
                metadata_path: meta,
                qa,
                highlight_id: id.clone(),
            });
        }
        engine
            .queue
            .advance(
                &job,
                Stage::QualityChecking,
                json!({"rendered":outputs.len()}),
            )
            .map_err(err)?;
        progress(Stage::QualityChecking, 1.);
        if !a.fingerprint.unchanged().map_err(err)? {
            return Err("Source changed during export".into());
        }
        let manifest:Vec<_>=outputs.iter().map(|o|json!({"video":o.video_path.file_name().unwrap(),"cover":o.cover_path.as_ref().and_then(|p|p.file_name()),"metadata":o.metadata_path.file_name().unwrap(),"highlight_id":o.highlight_id,"qa":o.qa})).collect();
        write_json(&pending.join("clips.json"), &manifest)?;
        check_cancel(cancel)?;
        let final_dir = folder.join(format!("Concat-Shorts-{}", unique()));
        engine
            .queue
            .advance(
                &job,
                Stage::QualityChecking,
                json!({"qa_passed":true,"recoverable_output":final_dir,"staged_output":pending}),
            )
            .map_err(err)?;
        fs::rename(&pending, &final_dir).map_err(err)?;
        drop(guard);
        for out in &mut outputs {
            out.video_path = final_dir.join(out.video_path.file_name().unwrap());
            out.cover_path = out
                .cover_path
                .as_ref()
                .map(|p| final_dir.join(p.file_name().unwrap()));
            out.metadata_path = final_dir.join(out.metadata_path.file_name().unwrap());
        }
        // Manifest paths are relative so relocation does not invalidate them.
        let warnings: Vec<_> = outputs.iter().flat_map(|o| o.qa.warnings.clone()).collect();
        let receipt = json!({"files":outputs.len(),"directory":final_dir,"full_decode":true,"warnings":warnings,"publisher":"not configured"});
        if warnings.is_empty() {
            engine.queue.finish(&job,outputs.iter().map(|o|o.video_path.clone()).collect(),receipt).map_err(|_|format!("Video files passed QA and were saved to {}. The queue receipt could not be saved; do not re-export blindly. The prior QA receipt records this directory.",final_dir.display()))?;
            progress(Stage::Completed, 1.);
        } else {
            engine
                .queue
                .advance(&job, Stage::QualityChecking, receipt)
                .map_err(err)?;
            engine
                .queue
                .needs_review(&job, "OUTPUT_QA_WARNINGS")
                .map_err(err)?;
            progress(Stage::NeedsReview, 1.);
        }
        Ok(outputs)
    })();
    if result.is_err() {
        if cancel.load(Ordering::Acquire) {
            let _ = engine.queue.cancel(&job);
        } else {
            let _ = engine.queue.fail(&job, "EXPORT_FAILED");
        }
    }
    result
}
fn stamp(t: f64) -> String {
    let ms = (t * 1000.).round().max(0.) as u64;
    format!(
        "{:02}:{:02}:{:02},{:03}",
        ms / 3600000,
        (ms / 60000) % 60,
        (ms / 1000) % 60,
        ms % 1000
    )
}
fn preview_dimensions(w: u32, h: u32) -> (u32, u32) {
    for divisor in 1..=w.min(h) / 128 {
        if w % divisor == 0
            && h % divisor == 0
            && (w / divisor) % 2 == 0
            && (h / divisor) % 2 == 0
            && w.max(h) / divisor <= 640
        {
            return (w / divisor, h / divisor);
        }
    }
    (w, h) // Extreme ratios remain exact rather than silently changing the crop.
}
fn remap_cues(cues: &[TimedSegment], parts: &[highlight::SourceRange]) -> String {
    let mut text = String::new();
    let mut offset = 0.;
    let mut index = 1;
    for p in parts {
        for c in cues {
            let start = c.start.max(p.start);
            let end = c.end.min(p.end);
            if end > start {
                text.push_str(&format!(
                    "{index}\n{} --> {}\n{}\n\n",
                    stamp(offset + start - p.start),
                    stamp(offset + end - p.start),
                    c.text
                ));
                index += 1;
            }
        }
        offset += p.end - p.start;
    }
    text
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stitched_captions_keep_occupied_source_time() {
        let text = remap_cues(
            &[
                TimedSegment {
                    id: 1,
                    start: 10.,
                    end: 12.,
                    text: "Xin chào Việt Nam".into(),
                },
                TimedSegment {
                    id: 2,
                    start: 30.,
                    end: 32.,
                    text: "Tiếp theo".into(),
                },
            ],
            &[
                highlight::SourceRange {
                    start: 10.,
                    end: 12.,
                },
                highlight::SourceRange {
                    start: 30.,
                    end: 32.,
                },
            ],
        );
        assert!(text.contains("00:00:02,000 --> 00:00:04,000"));
        assert!(!text.contains("00:00:30"));
    }
    #[test]
    fn timestamp_carries_minutes() {
        assert_eq!(stamp(60.001), "00:01:00,001");
    }
    #[test]
    fn preview_preserves_exact_aspect_and_native_bounds() {
        for (w, h) in [(720, 1280), (1920, 1080), (128, 3840), (3840, 2160)] {
            let (x, y) = preview_dimensions(w, h);
            assert_eq!(x * h, y * w);
            assert!(x >= 128 && y >= 128 && x % 2 == 0 && y % 2 == 0);
        }
    }
}
