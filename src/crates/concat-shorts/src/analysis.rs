// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat contributors
//! Bounded native source measurements and installed-only ASR. No network/model download.
//! Inspired by HotClip signals/moments evidence separation (AGPL-3.0).
use crate::{Config, Progress, Source};
use concat_agent::highlight::{Signal, SignalKind, TimedSegment};
use concat_core::time::Rational;
use concat_host::AppDirs;
use concat_media::{
    AudioDecoder, AudioOptions, DecodeOptions, Decoder, FrameSource, SampleFormat, SeekableSource,
};
use concat_speech::transcribe::{TranscribeRequest, Transcriber};
use concat_studio::shorts_jobs::{SourceFingerprint, Stage};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MAX_BYTES: u64 = 4 * 1024 * 1024;
const MAX_SAMPLES: usize = 600;
fn cancelled(cancel: &AtomicBool) -> Result<(), String> {
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
    fn strict_srt_and_vtt_keep_real_times_unicode() {
        let s=parse_subtitles("1\r\n00:00:02,250 --> 00:00:05,750\r\nXin chào Việt Nam\r\n\r\n2\r\n00:00:07,000 --> 00:00:09,000\r\nCảm ơn",10.0).unwrap();
        assert_eq!((s[0].start, s[0].end), (2.25, 5.75));
        assert_eq!(s[0].text, "Xin chào Việt Nam");
        let v = parse_subtitles(
            "WEBVTT\n\ncue-a\n00:02.250 --> 00:05.750 align:start\nXin chào",
            10.0,
        )
        .unwrap();
        assert_eq!(v[0].start, s[0].start);
    }
    #[test]
    fn malformed_or_out_of_source_subtitles_fail_closed() {
        for text in [
            "1\n00:60:00,000 --> 00:61:00,000\nX",
            "1\n00:00:03,000 --> 00:00:02,000\nX",
            "1\n00:00:03,000 --> 00:00:20,000\nX",
            "1\nnot a timestamp\nX",
            "WEBVTT\n\n00:01.000 --> 00:02.000\n",
            "1\n00:00:01,000 --> 00:00:02,000\nX\n\nBAD",
        ] {
            assert!(parse_subtitles(text, 10.0).is_err(), "{text}")
        }
    }
    #[test]
    fn subtitles_overlap_is_explicit_not_silently_shifted() {
        assert!(
            parse_subtitles(
                "1\n00:00:01,000 --> 00:00:04,000\nA\n\n2\n00:00:03,000 --> 00:00:05,000\nB",
                10.0
            )
            .is_err()
        );
    }
    #[test]
    fn no_audio_or_motion_does_not_invent_evidence() {
        assert!(rms_signals(&[(0.0, 1.0, -90.0), (1.0, 2.0, -90.0)]).is_empty());
        assert!(visual_signals(&[(0.0, 0.0), (1.0, 0.0)], 5.0).is_empty());
    }
    #[test]
    fn measured_bursts_and_visual_samples_are_bounded() {
        let mut samples = vec![];
        for n in 0..20 {
            samples.push((n as f64, n as f64 + 1.0, if n == 10 { -8.0 } else { -30.0 }))
        }
        let out = rms_signals(&samples);
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].start, out[0].end), (10.0, 11.0));
        let motion = visual_signals(&[(1.0, 0.01), (2.0, 0.5), (3.0, 0.02)], 4.0);
        assert!(
            motion
                .iter()
                .all(|s| s.start >= 0.0 && s.end <= 4.0 && s.strength.is_finite())
        );
    }
    #[test]
    fn cached_data_must_match_schema_and_source_bounds() {
        let entry = Cache {
            version: 1,
            key: "x".into(),
            segments: vec![TimedSegment {
                id: 1,
                start: 0.0,
                end: 20.0,
                text: "X".into(),
            }],
            signals: vec![],
            warnings: vec![],
        };
        assert!(!cache_valid(&entry, "x", 10.0));
        assert!(!cache_valid(&entry, "other", 30.0));
    }
    #[test]
    fn cancellation_is_checked_without_external_services() {
        assert!(cancelled(&AtomicBool::new(true)).is_err());
    }
}

fn clock(value: &str) -> Result<f64, String> {
    let normalized = value.replace(',', ".");
    let chunks: Vec<_> = normalized.split(':').collect();
    if !(2..=3).contains(&chunks.len()) {
        return Err("Invalid subtitle timestamp".into());
    }
    let sec = chunks.last().unwrap().split('.').collect::<Vec<_>>();
    if sec.len() != 2
        || sec[0].len() != 2
        || sec[1].len() != 3
        || !sec.iter().all(|s| s.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("Invalid subtitle timestamp precision".into());
    }
    let seconds = sec[0]
        .parse::<u32>()
        .map_err(|_| "Invalid subtitle seconds")?;
    let millis = sec[1]
        .parse::<u32>()
        .map_err(|_| "Invalid subtitle milliseconds")?;
    let mins = chunks[chunks.len() - 2];
    if mins.len() != 2 || !mins.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Invalid subtitle minutes".into());
    }
    let minutes = mins
        .parse::<u32>()
        .map_err(|_| "Invalid subtitle minutes")?;
    let hours = if chunks.len() == 3 {
        if chunks[0].len() < 2 || !chunks[0].bytes().all(|b| b.is_ascii_digit()) {
            return Err("Invalid subtitle hours".into());
        }
        chunks[0]
            .parse::<u32>()
            .map_err(|_| "Invalid subtitle hours")?
    } else {
        0
    };
    if seconds >= 60 || minutes >= 60 || hours > 48 {
        return Err("Subtitle timestamp outside supported clock".into());
    }
    Ok(hours as f64 * 3600.0 + minutes as f64 * 60.0 + seconds as f64 + millis as f64 / 1000.0)
}
fn parse_subtitles(text: &str, duration: f64) -> Result<Vec<TimedSegment>, String> {
    if text.len() > MAX_BYTES as usize {
        return Err("Subtitle input exceeds4MB".into());
    }
    let normalized = text
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let vtt = normalized.starts_with("WEBVTT");
    let mut out = Vec::new();
    let mut last_end = 0.0;
    for (n, block) in normalized
        .split("\n\n")
        .filter(|b| !b.trim().is_empty())
        .enumerate()
    {
        let lines = block.lines().map(str::trim).collect::<Vec<_>>();
        if vtt && n == 0 {
            if !lines[0].starts_with("WEBVTT") {
                return Err("Invalid WebVTT header".into());
            }
            continue;
        }
        if vtt
            && (lines[0] == "NOTE"
                || lines[0].starts_with("NOTE ")
                || lines[0] == "STYLE"
                || lines[0] == "REGION")
        {
            continue;
        }
        let timing = if lines[0].contains("-->") {
            0
        } else {
            if !vtt && lines[0].parse::<u32>().is_err() {
                return Err("Invalid SRT cue ID".into());
            }
            1
        };
        let line = lines.get(timing).ok_or("Subtitle cue has no timing")?;
        let (a, b) = line.split_once("-->").ok_or("Subtitle cue has no arrow")?;
        let start = clock(a.trim())?;
        let end_word = b
            .split_whitespace()
            .next()
            .ok_or("Subtitle cue has no end")?;
        let end = clock(end_word)?;
        if !vtt && b.trim() != end_word {
            return Err("Unexpected SRT cue settings".into());
        }
        let words = lines
            .get(timing + 1..)
            .ok_or("Subtitle cue has no text")?
            .join(" ");
        if words.is_empty()
            || !start.is_finite()
            || !end.is_finite()
            || start < last_end - 1e-9
            || end <= start
            || end > duration + 1e-6
        {
            return Err("Subtitle cue overlaps or exceeds source bounds".into());
        }
        if out.len() >= 20000 {
            return Err("Subtitle cue count exceeds limit".into());
        }
        out.push(TimedSegment {
            id: out.len() as u32 + 1,
            start,
            end,
            text: words,
        });
        last_end = end;
    }
    if out.is_empty() {
        return Err("Subtitle file has no valid cues".into());
    }
    Ok(out)
}

#[derive(Serialize, Deserialize)]
struct Cache {
    version: u32,
    key: String,
    segments: Vec<TimedSegment>,
    signals: Vec<Signal>,
    warnings: Vec<String>,
}
fn cache_valid(cache: &Cache, key: &str, duration: f64) -> bool {
    if cache.version != 1
        || cache.key != key
        || cache.segments.len() > 20000
        || cache.signals.len() > 1800
        || cache.warnings.len() > 32
    {
        return false;
    }
    let mut ids = std::collections::HashSet::new();
    let mut previous = 0.0;
    for s in &cache.segments {
        if !s.start.is_finite()
            || !s.end.is_finite()
            || s.start < previous
            || s.end <= s.start
            || s.end > duration
            || s.text.len() > 65536
            || !ids.insert(s.id)
        {
            return false;
        }
        previous = s.start;
    }
    cache.signals.iter().all(|s| {
        s.start.is_finite()
            && s.end.is_finite()
            && s.strength.is_finite()
            && s.start >= 0.0
            && s.end > s.start
            && s.end <= duration
            && (0.0..=1.0).contains(&s.strength)
    })
}
fn limited_text(path: &Path) -> Result<String, String> {
    let meta = fs::symlink_metadata(path).map_err(|_| "Cannot read subtitle/cache metadata")?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > MAX_BYTES {
        return Err("Subtitle/cache file is invalid or oversized".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| "Cannot open subtitle/cache")?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read subtitle/cache")?;
    if bytes.len() > MAX_BYTES as usize {
        return Err("Subtitle/cache grew beyond limit".into());
    }
    String::from_utf8(bytes).map_err(|_| "Subtitle/cache must be UTF-8".into())
}
fn cache_key(
    source: &Source,
    config: &Config,
    model: Option<&str>,
    dirs: &AppDirs,
) -> Result<String, String> {
    let fp = SourceFingerprint::capture(&source.path, &serde_json::json!({}))
        .map_err(|_| "Cannot fingerprint shorts source")?;
    let sidecar = if let Some(path) = &config.transcript_path {
        Some(
            SourceFingerprint::capture(path, &serde_json::json!({}))
                .map_err(|_| "Cannot fingerprint subtitle source")?,
        )
    } else {
        None
    };
    let model_identity = model
        .and_then(|m| {
            fs::metadata(
                dirs.data
                    .join("whisper-models")
                    .join(format!("ggml-{m}.bin")),
            )
            .ok()
        })
        .map(|m| {
            (
                m.len(),
                m.modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos()),
            )
        });
    let encoded=serde_json::to_vec(&serde_json::json!({"version":1,"source":fp,"config":config,"subtitle":sidecar,"asr":model,"modelIdentity":model_identity,"duration":source.duration,"size":[source.width,source.height]})).map_err(|_|"Cannot serialize analysis identity")?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}
fn store_cache(dir: &Path, entry: &Cache) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|_| "Cannot create analysis cache")?;
    if fs::symlink_metadata(dir)
        .map_err(|_| "Cannot verify analysis cache")?
        .file_type()
        .is_symlink()
    {
        return Err("Analysis cache cannot be a symlink".into());
    }
    let bytes = serde_json::to_vec(entry).map_err(|_| "Cannot encode analysis cache")?;
    if bytes.len() > MAX_BYTES as usize {
        return Err("Analysis cache exceeds4MB".into());
    }
    let path = dir.join(format!("analysis-{}.json", entry.key));
    if !path.exists() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "Clock unavailable")?
            .as_nanos();
        let temp = dir.join(format!("analysis-{}-{nonce}.part", entry.key));
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .map_err(|_| "Cannot create cache artifact")?;
        let write = (|| {
            file.write_all(&bytes)
                .map_err(|_| "Cannot write cache artifact")?;
            file.sync_all().map_err(|_| "Cannot sync cache artifact")?;
            drop(file);
            fs::rename(&temp, &path).map_err(|_| "Cannot commit cache artifact")
        })();
        if write.is_err() {
            let _ = fs::remove_file(&temp);
        }
        write?;
    }
    // Only our own non-symlink JSON cache artifacts; never media/models/projects.
    let mut entries = fs::read_dir(dir)
        .map_err(|_| "Cannot list analysis cache")?
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.starts_with("analysis-") || !name.ends_with(".json") {
                return None;
            }
            let m = fs::symlink_metadata(e.path()).ok()?;
            (!m.file_type().is_symlink() && m.is_file()).then_some((
                e.path(),
                m.len(),
                m.modified().unwrap_or(UNIX_EPOCH),
            ))
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|e| e.2);
    let mut size = entries.iter().map(|e| e.1).sum::<u64>();
    let mut count = entries.len();
    for (path, bytes, _) in entries {
        if count <= 12 && size <= 32 * 1024 * 1024 {
            break;
        }
        if fs::remove_file(path).is_ok() {
            size = size.saturating_sub(bytes);
            count -= 1;
        }
    }
    Ok(())
}

fn rms_signals(samples: &[(f64, f64, f64)]) -> Vec<Signal> {
    if samples.is_empty() {
        return vec![];
    }
    let mut values = samples.iter().map(|s| s.2).collect::<Vec<_>>();
    values.sort_by(f64::total_cmp);
    let median = values[values.len() / 2];
    let threshold = (median + 6.0).max(-50.0);
    samples
        .iter()
        .filter(|s| s.2 >= threshold && s.1 > s.0)
        .map(|s| Signal {
            kind: SignalKind::Loudness,
            start: s.0,
            end: s.1,
            strength: ((s.2 - threshold + 6.0) / 24.0).clamp(0.05, 1.0),
        })
        .collect()
}
fn visual_signals(samples: &[(f64, f64)], duration: f64) -> Vec<Signal> {
    let mut values = samples
        .iter()
        .map(|s| s.1)
        .filter(|s| *s > 0.0)
        .collect::<Vec<_>>();
    if values.is_empty() {
        return vec![];
    }
    values.sort_by(f64::total_cmp);
    let threshold = values[(values.len() * 85 / 100).min(values.len() - 1)].max(0.025);
    let mut out = Vec::new();
    for (i, (t, diff)) in samples.iter().enumerate() {
        if *diff < threshold {
            continue;
        }
        let start = if i > 0 { samples[i - 1].0 } else { *t };
        let end = (*t).min(duration);
        if end <= start {
            continue;
        }
        out.push(Signal {
            kind: SignalKind::Motion,
            start,
            end,
            strength: (diff / 0.4).clamp(0.05, 1.0),
        });
        if *diff >= 0.4 {
            out.push(Signal {
                kind: SignalKind::Cut,
                start,
                end,
                strength: (diff / 0.8).min(1.0),
            })
        }
    }
    out
}
fn audio(source: &Source, cancel: &AtomicBool) -> Result<Vec<Signal>, String> {
    let rate = 8000u32;
    let window = (source.duration / MAX_SAMPLES as f64).max(1.0);
    let per_window = (window * rate as f64).ceil() as u64;
    let mut decoder = AudioDecoder::open(
        &source.path,
        &AudioOptions {
            duration: Some(source.duration),
            rate,
            channels: 1,
            format: SampleFormat::F32,
            ..Default::default()
        },
    )
    .map_err(|_| "Audio evidence decode unavailable")?;
    let mut count = 0u64;
    let mut used = 0u64;
    let mut sum = 0.0;
    let mut samples = Vec::new();
    let mut start = 0.0;
    while let Some(frame) = decoder
        .next_f32()
        .map_err(|_| "Audio evidence decoding failed")?
    {
        cancelled(cancel)?;
        for value in frame {
            if !value.is_finite() {
                return Err("Audio samples contain invalid amplitudes".into());
            }
            sum += (value as f64).powi(2);
            count += 1;
            used += 1;
            if count >= per_window {
                let end = (used as f64 / rate as f64).min(source.duration);
                if end > start {
                    samples.push((
                        start,
                        end,
                        20.0 * (sum / count as f64).sqrt().max(1e-9).log10(),
                    ));
                }
                start = end;
                count = 0;
                sum = 0.0;
                if samples.len() >= MAX_SAMPLES {
                    return Ok(rms_signals(&samples));
                }
            }
        }
    }
    if count > 0 && samples.len() < MAX_SAMPLES {
        let end = (used as f64 / rate as f64).min(source.duration);
        if end > start {
            samples.push((
                start,
                end,
                20.0 * (sum / count as f64).sqrt().max(1e-9).log10(),
            ))
        }
    }
    Ok(rms_signals(&samples))
}
fn visual(
    source: &Source,
    cancel: &AtomicBool,
    progress: &Progress,
) -> Result<Vec<Signal>, String> {
    let factor = (160.0 / source.width as f64)
        .min(90.0 / source.height as f64)
        .min(1.0);
    let width = ((source.width as f64 * factor) as u32).max(2) & !1;
    let height = ((source.height as f64 * factor) as u32).max(2) & !1;
    let mut decoder = Decoder::open(
        &source.path,
        &DecodeOptions::default()
            .scaled_to(width, height)
            .threaded(2),
    )
    .map_err(|_| "Visual evidence decoder unavailable")?;
    let count = (source.duration * 2.0)
        .ceil()
        .max(2.0)
        .min(MAX_SAMPLES as f64) as usize;
    let mut previous: Option<Vec<u8>> = None;
    let mut samples = Vec::new();
    for i in 0..count {
        cancelled(cancel)?;
        let requested = source.duration * i as f64 / count as f64;
        decoder
            .seek(Rational::new(
                (requested * 1_000_000.0).round() as i64,
                1_000_000,
            ))
            .map_err(|_| "Visual evidence seek failed")?;
        let Some(frame) = decoder
            .next_frame()
            .map_err(|_| "Visual evidence decoding failed")?
        else {
            break;
        };
        let Some(pts) = decoder
            .position()
            .map(|p| p.as_f64())
            .filter(|p| p.is_finite() && *p >= 0.0 && *p < source.duration)
        else {
            continue;
        };
        if samples.last().is_some_and(|s: &(f64, f64)| pts <= s.0) {
            continue;
        }
        let pixels = frame.pixels();
        let diff = previous
            .as_ref()
            .filter(|p| p.len() == pixels.len())
            .map(|before| {
                before
                    .iter()
                    .zip(pixels)
                    .enumerate()
                    .filter(|(n, _)| n % 4 != 3)
                    .map(|(_, (&a, &b))| a.abs_diff(b) as f64 / 255.0)
                    .sum::<f64>()
                    / (pixels.len() / 4 * 3) as f64
            })
            .unwrap_or(0.0);
        previous = Some(pixels.to_vec());
        samples.push((pts, diff));
        progress(Stage::Analyzing, ((i + 1) as f32 / count as f32).min(1.0));
    }
    if samples.len() < 2 {
        return Err("Insufficient actual video timestamps for visual evidence".into());
    }
    Ok(visual_signals(&samples, source.duration))
}

/// Real measurements; missing ASR remains a valid visual-only path and is never downloaded.
pub(crate) fn run(
    source: &Source,
    config: &Config,
    transcriber: &Arc<Transcriber>,
    dirs: &AppDirs,
    cancel: &AtomicBool,
    progress: &Progress,
) -> Result<(Vec<TimedSegment>, Vec<Signal>, Vec<String>), String> {
    cancelled(cancel)?;
    if !source.duration.is_finite()
        || source.duration <= 0.0
        || source.duration > 172800.0
        || source.width == 0
        || source.height == 0
    {
        return Err("Invalid source analysis bounds".into());
    }
    let status = if source.has_audio && config.transcript_path.is_none() {
        Transcriber::status(dirs).ok()
    } else {
        None
    };
    let model = status.as_ref().and_then(|status| {
        status
            .models
            .iter()
            .filter(|m| m.downloaded && !m.english_only)
            .min_by_key(|m| m.size_bytes)
            .map(|m| m.id.clone())
            .or_else(|| {
                status
                    .models
                    .iter()
                    .filter(|m| m.downloaded)
                    .min_by_key(|m| m.size_bytes)
                    .map(|m| m.id.clone())
            })
    });
    let key = cache_key(source, config, model.as_deref(), dirs)?;
    let cache_dir = dirs.data.join("auto-shorts/cache");
    if let Ok(raw) = limited_text(&cache_dir.join(format!("analysis-{key}.json"))) {
        if let Ok(entry) = serde_json::from_str::<Cache>(&raw) {
            if cache_valid(&entry, &key, source.duration) {
                cancelled(cancel)?;
                progress(Stage::Analyzing, 1.0);
                return Ok((entry.segments, entry.signals, entry.warnings));
            }
        }
    }
    let mut warnings=vec!["Visual signals use at most600 real decoder PTS samples; frame differences are activity/cut hints, not face/emotion/semantic detections.".into(),"Loudness evidence is streaming mono RMS amplitude, not LUFS or speech emotion.".into()];
    let mut cacheable = true;
    let mut signals = match visual(source, cancel, progress) {
        Ok(s) => s,
        Err(e) => {
            cancelled(cancel)?;
            cacheable = false;
            warnings.push(e);
            vec![]
        }
    };
    if source.has_audio {
        match audio(source, cancel) {
            Ok(s) => signals.extend(s),
            Err(e) => {
                cancelled(cancel)?;
                cacheable = false;
                warnings.push(e)
            }
        }
    }
    let mut segments = Vec::new();
    if let Some(path) = &config.transcript_path {
        segments = parse_subtitles(&limited_text(path)?, source.duration)?;
        warnings.push("Subtitle timings are imported source evidence; text accuracy was not independently verified.".into());
    } else if source.has_audio {
        if let Some(model) = model {
            if transcriber.is_busy() {
                cacheable = false;
                warnings.push("Installed ASR is busy; visual/audio-only analysis retained. Import subtitles or retry later.".into());
            } else {
                let mirrored = Arc::new(AtomicBool::new(false));
                let done = AtomicBool::new(false);
                let result = std::thread::scope(|scope| {
                    let mirrored_watch = mirrored.clone();
                    let done_ref = &done;
                    scope.spawn(move || {
                        while !done_ref.load(Ordering::Acquire) {
                            mirrored_watch.store(cancel.load(Ordering::Acquire), Ordering::Release);
                            std::thread::park_timeout(Duration::from_millis(20));
                        }
                    });
                    let result = (|| {
                        let mut start = 0.0;
                        while start < source.duration {
                            cancelled(cancel)?;
                            let window = (source.duration - start).min(120.0);
                            let request = TranscribeRequest {
                                path: source.path.to_string_lossy().into_owned(),
                                audio_stream: None,
                                source_start: start,
                                window,
                                model_id: model.clone(),
                            };
                            let notify = progress.clone();
                            let owned = transcriber.clone();
                            let mirror = mirrored.clone();
                            let duration = source.duration;
                            let cues = transcriber.transcribe(dirs, &request, move |p| {
                                if mirror.load(Ordering::Acquire) {
                                    owned.cancel()
                                }
                                notify(
                                    Stage::Transcribing,
                                    ((start + window * (p as f64 / 100.0).clamp(0.0, 1.0))
                                        / duration) as f32,
                                )
                            })?;
                            cancelled(cancel)?;
                            for c in cues {
                                if !c.start.is_finite()
                                    || !c.end.is_finite()
                                    || c.start < 0.0
                                    || c.end <= c.start
                                    || c.end > window + 1e-6
                                {
                                    return Err(
                                        "ASR returned timestamps outside its real source window"
                                            .into(),
                                    );
                                }
                                if segments.len() >= 20000 {
                                    return Err("ASR segments exceed limit".into());
                                }
                                segments.push(TimedSegment {
                                    id: segments.len() as u32 + 1,
                                    start: start + c.start,
                                    end: start + c.end,
                                    text: c.text,
                                });
                            }
                            start += window;
                        }
                        Ok::<(), String>(())
                    })();
                    done.store(true, Ordering::Release);
                    result
                });
                if result.is_err() {
                    cancelled(cancel)?;
                    cacheable = false;
                    warnings.push("Installed ASR did not complete; partial transcript and real visual/audio evidence retained. Review before use.".into())
                }
                warnings.push("ASR segment times are measured within<=120s native Whisper windows; no word alignment was inferred.".into());
            }
        } else {
            if status.is_none() {
                cacheable = false;
            }
            warnings.push("No installed Whisper model: visual/audio-only analysis; no model was downloaded. Import SRT/VTT for speech-based selection.".into());
        }
    }
    cancelled(cancel)?;
    let entry = Cache {
        version: 1,
        key,
        segments,
        signals,
        warnings,
    };
    if !cache_valid(&entry, &entry.key, source.duration) {
        return Err("Measured analysis failed evidence bounds validation".into());
    }
    let mut warnings = entry.warnings.clone();
    if cacheable && store_cache(&cache_dir, &entry).is_err() {
        warnings.push("Analysis succeeded but bounded cache could not be written.".into())
    }
    Ok((entry.segments, entry.signals, warnings))
}
