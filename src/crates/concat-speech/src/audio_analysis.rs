// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer and Concat contributors

//! Audio & scene analysis port from OpenChatCut (`audio/silence.ts`, `audio/beats.ts`).
//!
//! Provides:
//! - Windowed RMS silence detection with conservative gating, boundary padding,
//!   and source-revision binding.
//! - Spectral flux onset envelope + autocorrelation beat detection (60-180 BPM,
//!   equidistant beat grid, 4/4 downbeats).
//! - Scene change detection across frame luminance differences.

use serde::{Deserialize, Serialize};

/// Thresholds and parameters for silence detection matching OpenChatCut defaults.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SilenceParams {
    /// Silence threshold relative to high-decile RMS reference (dB, default -26.0).
    pub threshold_db: f64,
    /// Minimum silence duration to consider for deletion (seconds, default 0.6s).
    pub min_silence_sec: f64,
    /// Breathing padding reserved on each side (seconds, default 0.15s).
    pub pad_sec: f64,
    /// Hard ceiling dBFS above which audio is never classified as silence (default -35.0 dBFS).
    pub absolute_silence_db: f64,
    /// Noise floor dBFS below which the recording lacks acoustic evidence (default -45.0 dBFS).
    pub signal_floor_db: f64,
}

impl Default for SilenceParams {
    fn default() -> Self {
        Self {
            threshold_db: -26.0,
            min_silence_sec: 0.6,
            pad_sec: 0.15,
            absolute_silence_db: -35.0,
            signal_floor_db: -45.0,
        }
    }
}

/// A detected silence span in source-media seconds.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SilenceSpan {
    /// Start time in source seconds.
    pub start_sec: f64,
    /// End time in source seconds.
    pub end_sec: f64,
}

/// A revision-bound silence compression plan.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SilenceCompressionPlan {
    /// Source media path.
    pub media_path: String,
    /// Source revision token.
    pub source_revision: String,
    /// Detected silence spans.
    pub silent_spans: Vec<SilenceSpan>,
    /// Kept spans to remain on the timeline.
    pub keep_spans: Vec<SilenceSpan>,
}

fn to_db(rms: f64) -> f64 {
    20.0 * (rms.max(1e-8)).log10()
}

/// Computes windowed RMS envelope with non-overlapping hops.
pub fn rms_envelope(samples: &[f32], sample_rate: u32, window_ms: u32) -> Vec<f64> {
    if samples.is_empty() || sample_rate == 0 || window_ms == 0 {
        return Vec::new();
    }
    let win = (((sample_rate as f64) * (window_ms as f64) / 1000.0).round() as usize).max(1);
    let num_windows = (samples.len() + win - 1) / win;
    let mut out = Vec::with_capacity(num_windows);

    for chunk in samples.chunks(win) {
        let sum_sq: f64 = chunk.iter().map(|&s| (s as f64) * (s as f64)).sum();
        let rms = (sum_sq / (chunk.len() as f64)).sqrt();
        out.push(rms);
    }
    out
}

/// Calculates the p-th percentile dB value from an RMS envelope.
pub fn percentile_db(env: &[f64], p: f64) -> f64 {
    if env.is_empty() {
        return -100.0;
    }
    let mut sorted: Vec<f64> = env.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = (((sorted.len() - 1) as f64 * p).floor() as usize).min(sorted.len() - 1);
    to_db(sorted[idx])
}

/// Detects silent spans from the RMS envelope.
pub fn detect_silent_spans(
    env: &[f64],
    window_ms: u32,
    params: &SilenceParams,
) -> Vec<SilenceSpan> {
    if env.is_empty() || window_ms == 0 {
        return Vec::new();
    }
    let win_sec = (window_ms as f64) / 1000.0;
    let signal_ref_db = percentile_db(env, 0.9);
    if signal_ref_db < params.signal_floor_db {
        return Vec::new();
    }

    let gate_db = (signal_ref_db + params.threshold_db).min(params.absolute_silence_db);
    let mut spans = Vec::new();
    let mut run_start: Option<usize> = None;

    for (w, &rms) in env.iter().enumerate() {
        let is_silent = to_db(rms) < gate_db;
        match (is_silent, run_start) {
            (true, None) => run_start = Some(w),
            (false, Some(start)) => {
                let raw_start = (start as f64) * win_sec;
                let raw_end = (w as f64) * win_sec;
                let padded_start = raw_start + params.pad_sec;
                let padded_end = raw_end - params.pad_sec;
                if padded_end - padded_start >= params.min_silence_sec - 2.0 * params.pad_sec
                    && padded_end > padded_start
                {
                    spans.push(SilenceSpan {
                        start_sec: padded_start,
                        end_sec: padded_end,
                    });
                }
                run_start = None;
            }
            _ => {}
        }
    }

    if let Some(start) = run_start {
        let raw_start = (start as f64) * win_sec;
        let raw_end = (env.len() as f64) * win_sec;
        let padded_start = raw_start + params.pad_sec;
        let padded_end = raw_end - params.pad_sec;
        if padded_end - padded_start >= params.min_silence_sec - 2.0 * params.pad_sec
            && padded_end > padded_start
        {
            spans.push(SilenceSpan {
                start_sec: padded_start,
                end_sec: padded_end,
            });
        }
    }

    spans
}

/// Inverts silent spans over total duration to yield keep spans with revision binding.
pub fn build_silence_compression_plan(
    media_path: &str,
    source_revision: &str,
    total_duration_sec: f64,
    silent_spans: &[SilenceSpan],
) -> SilenceCompressionPlan {
    let mut keep_spans = Vec::new();
    let mut cur = 0.0;

    for span in silent_spans {
        if span.start_sec > cur {
            keep_spans.push(SilenceSpan {
                start_sec: cur,
                end_sec: span.start_sec,
            });
        }
        cur = span.end_sec.max(cur);
    }

    if cur < total_duration_sec {
        keep_spans.push(SilenceSpan {
            start_sec: cur,
            end_sec: total_duration_sec,
        });
    }

    SilenceCompressionPlan {
        media_path: media_path.to_string(),
        source_revision: source_revision.to_string(),
        silent_spans: silent_spans.to_vec(),
        keep_spans,
    }
}

// ── Beat Detection ──────────────────────────────────────────────────────────

const FFT_SIZE: usize = 1024;
const HOP_SIZE: usize = 512;
const MIN_BPM: f64 = 60.0;
const MAX_BPM: f64 = 180.0;

/// Beat analysis result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BeatAnalysis {
    /// Detected BPM (beats per minute).
    pub bpm: f64,
    /// Confidence ratio (autocorrelation peak vs mean).
    pub confidence: f64,
    /// Beat timestamps in source seconds.
    pub beats: Vec<f64>,
    /// Measure downbeat timestamps (4/4 accent phase).
    pub downbeats: Vec<f64>,
}

/// In-place Radix-2 Cooley-Tukey FFT.
fn radix2_fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }

    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f64::consts::PI / (len as f64);
        let w_re = ang.cos();
        let w_im = ang.sin();
        let half = len / 2;
        let mut i = 0;
        while i < n {
            let mut cur_re = 1.0;
            let mut cur_im = 0.0;
            for k in 0..half {
                let a = i + k;
                let b = i + k + half;
                let t_re = re[b] * cur_re - im[b] * cur_im;
                let t_im = re[b] * cur_im + im[b] * cur_re;
                re[b] = re[a] - t_re;
                im[b] = im[a] - t_im;
                re[a] += t_re;
                im[a] += t_im;
                let next_re = cur_re * w_re - cur_im * w_im;
                cur_im = cur_re * w_im + cur_im * w_re;
                cur_re = next_re;
            }
            i += len;
        }
        len <<= 1;
    }
}

/// Computes spectral flux onset envelope with Hann windowing and half-wave rectification.
pub fn onset_envelope(samples: &[f32], sample_rate: u32) -> (Vec<f64>, f64) {
    let hop_sec = (HOP_SIZE as f64) / (sample_rate as f64);
    if samples.len() <= FFT_SIZE {
        return (Vec::new(), hop_sec);
    }
    let frames = (samples.len() - FFT_SIZE) / HOP_SIZE;
    if frames <= 2 {
        return (Vec::new(), hop_sec);
    }

    let mut hann = vec![0.0; FFT_SIZE];
    for (i, h) in hann.iter_mut().enumerate() {
        *h = 0.5 * (1.0 - (2.0 * std::f64::consts::PI * (i as f64) / (FFT_SIZE as f64 - 1.0)).cos());
    }

    let bins = FFT_SIZE / 2;
    let mut prev_mag = vec![0.0; bins];
    let mut flux = vec![0.0; frames];
    let mut re = vec![0.0; FFT_SIZE];
    let mut im = vec![0.0; FFT_SIZE];

    for (t, fl) in flux.iter_mut().enumerate().take(frames) {
        let at = t * HOP_SIZE;
        for i in 0..FFT_SIZE {
            re[i] = (samples[at + i] as f64) * hann[i];
            im[i] = 0.0;
        }
        radix2_fft(&mut re, &mut im);

        let mut sum = 0.0;
        for k in 0..bins {
            let mag = (re[k] * re[k] + im[k] * im[k]).sqrt();
            let rise = mag - prev_mag[k];
            if rise > 0.0 {
                sum += rise;
            }
            prev_mag[k] = mag;
        }
        *fl = if t == 0 { 0.0 } else { sum };
    }

    // Detrend by subtracting moving average (0.5s window)
    let half = ((0.25 / hop_sec).round() as usize).max(1);
    let mut env = vec![0.0; frames];
    for t in 0..frames {
        let from = t.saturating_sub(half);
        let to = (t + half).min(frames - 1);
        let mean: f64 = flux[from..=to].iter().sum::<f64>() / ((to - from + 1) as f64);
        env[t] = (flux[t] - mean).max(0.0);
    }

    (env, hop_sec)
}

/// Detects BPM and beat timestamps using autocorrelation of the onset envelope.
pub fn detect_beats(samples: &[f32], sample_rate: u32) -> BeatAnalysis {
    let (env, hop_sec) = onset_envelope(samples, sample_rate);
    if env.len() < 10 || hop_sec <= 0.0 {
        return BeatAnalysis {
            bpm: 0.0,
            confidence: 0.0,
            beats: Vec::new(),
            downbeats: Vec::new(),
        };
    }

    let min_lag = (2).max(((60.0 / MAX_BPM) / hop_sec).floor() as usize);
    let max_lag = ((60.0 / MIN_BPM) / hop_sec).ceil() as usize;

    if env.len() < max_lag * 2 {
        return BeatAnalysis {
            bpm: 0.0,
            confidence: 0.0,
            beats: Vec::new(),
            downbeats: Vec::new(),
        };
    }

    let mut ac = vec![0.0; max_lag + 1];
    for lag in min_lag..=max_lag {
        let mut sum = 0.0;
        let n = env.len() - lag;
        for t in 0..n {
            sum += env[t] * env[t + lag];
        }
        ac[lag] = sum / (n as f64);
    }

    let mut ac_mean = 0.0;
    for lag in min_lag..=max_lag {
        ac_mean += ac[lag];
    }
    ac_mean /= (max_lag - min_lag + 1) as f64;

    if ac_mean <= 0.0 {
        return BeatAnalysis {
            bpm: 0.0,
            confidence: 0.0,
            beats: Vec::new(),
            downbeats: Vec::new(),
        };
    }

    let mut best_lag = 0;
    let mut best_score = f64::NEG_INFINITY;
    for lag in min_lag..=max_lag {
        let bpm = 60.0 / ((lag as f64) * hop_sec);
        let octaves = (bpm / 120.0).log2();
        let weight = (-0.5 * (octaves / 0.6).powi(2)).exp();
        let score = ac[lag] * weight;
        if score > best_score {
            best_score = score;
            best_lag = lag;
        }
    }

    let confidence = if best_lag > 0 { ac[best_lag] / ac_mean } else { 0.0 };
    if confidence < 1.2 || best_lag == 0 {
        return BeatAnalysis {
            bpm: 0.0,
            confidence,
            beats: Vec::new(),
            downbeats: Vec::new(),
        };
    }

    let mut period = best_lag as f64;
    if best_lag > min_lag && best_lag < max_lag {
        let y0 = ac[best_lag - 1];
        let y1 = ac[best_lag];
        let y2 = ac[best_lag + 1];
        let denom = y0 - 2.0 * y1 + y2;
        if denom.abs() > 1e-12 {
            period = (best_lag as f64) + 0.5 * ((y0 - y2) / denom);
        }
    }

    let bpm = (60.0 / (period * hop_sec)).round();
    let beat_period = period * hop_sec;

    // Fit grid phase
    let mut best_phase = 0.0;
    let mut max_phase_energy = 0.0;
    let num_steps = 10;
    for step in 0..num_steps {
        let phase = ((step as f64) / (num_steps as f64)) * beat_period;
        let mut energy = 0.0;
        let mut t = phase;
        while t < (env.len() as f64) * hop_sec {
            let idx = (t / hop_sec).floor() as usize;
            if idx < env.len() {
                energy += env[idx];
            }
            t += beat_period;
        }
        if energy > max_phase_energy {
            max_phase_energy = energy;
            best_phase = phase;
        }
    }

    let mut beats = Vec::new();
    let mut downbeats = Vec::new();
    let mut t = best_phase;
    let mut beat_idx = 0;
    let total_sec = (samples.len() as f64) / (sample_rate as f64);

    while t < total_sec {
        beats.push(t);
        if beat_idx % 4 == 0 {
            downbeats.push(t);
        }
        t += beat_period;
        beat_idx += 1;
    }

    BeatAnalysis {
        bpm,
        confidence,
        beats,
        downbeats,
    }
}

// ── Scene Detection ─────────────────────────────────────────────────────────

/// A detected scene change.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneCut {
    /// Timestamp of the cut in seconds.
    pub time_sec: f64,
    /// Scene change difference score.
    pub score: f64,
}

/// Detects scene cut transitions from luminance differences between frames.
///
/// `frame_luminance`: mean or histogram difference between consecutive frames sampled at `fps`.
/// `threshold`: difference score threshold (typically 0.3..0.5).
/// `min_scene_sec`: minimum duration between scene cuts.
pub fn detect_scene_cuts(
    frame_diffs: &[f64],
    fps: f64,
    threshold: f64,
    min_scene_sec: f64,
) -> Vec<SceneCut> {
    if frame_diffs.is_empty() || fps <= 0.0 {
        return Vec::new();
    }
    let mut cuts = Vec::new();
    let mut last_cut_time = -min_scene_sec;

    for (idx, &score) in frame_diffs.iter().enumerate() {
        let t = (idx as f64) / fps;
        if score >= threshold && (t - last_cut_time) >= min_scene_sec {
            cuts.push(SceneCut {
                time_sec: t,
                score,
            });
            last_cut_time = t;
        }
    }
    cuts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rms_envelope_and_silence_detection() {
        let sample_rate = 16000;
        let mut samples = Vec::new();

        // 1 sec tone (loud speech)
        for i in 0..sample_rate {
            let t = (i as f32) / (sample_rate as f32);
            samples.push((t * 440.0 * 2.0 * std::f32::consts::PI).sin() * 0.5);
        }
        // 1 sec silence
        samples.extend(vec![0.0001f32; sample_rate as usize]);
        // 1 sec tone
        for i in 0..sample_rate {
            let t = (i as f32) / (sample_rate as f32);
            samples.push((t * 440.0 * 2.0 * std::f32::consts::PI).sin() * 0.5);
        }

        let env = rms_envelope(&samples, sample_rate, 50);
        assert!(!env.is_empty());

        let params = SilenceParams::default();
        let spans = detect_silent_spans(&env, 50, &params);
        assert_eq!(spans.len(), 1);
        assert!(spans[0].start_sec >= 1.0);
        assert!(spans[0].end_sec <= 2.0);

        let plan = build_silence_compression_plan("clip.mp4", "rev-1", 3.0, &spans);
        assert_eq!(plan.source_revision, "rev-1");
        assert_eq!(plan.keep_spans.len(), 2);
        assert!(plan.keep_spans[0].start_sec == 0.0);
        assert!(plan.keep_spans[1].end_sec == 3.0);
    }

    #[test]
    fn test_beat_detection_synthetic_rhythm() {
        let sample_rate = 16000;
        let total_sec = 4.0;
        let total_samples = (total_sec * (sample_rate as f64)) as usize;
        let mut samples = vec![0.0f32; total_samples];

        // Place clicks every 0.5s (corresponds to 120 BPM)
        let interval_samples = (0.5 * (sample_rate as f64)) as usize;
        let mut idx = 0;
        while idx < total_samples {
            for i in 0..100.min(total_samples - idx) {
                samples[idx + i] = 0.8;
            }
            idx += interval_samples;
        }

        let beats = detect_beats(&samples, sample_rate);
        assert!(beats.confidence >= 1.2);
        assert!((beats.bpm - 120.0).abs() <= 1.0);
        assert!(!beats.beats.is_empty());
        assert!(!beats.downbeats.is_empty());
    }

    #[test]
    fn test_scene_cut_detection() {
        let frame_diffs = vec![0.05, 0.08, 0.65, 0.02, 0.04, 0.80, 0.01];
        let cuts = detect_scene_cuts(&frame_diffs, 10.0, 0.5, 0.2);
        assert_eq!(cuts.len(), 2);
        assert!((cuts[0].time_sec - 0.2).abs() < 1e-4);
        assert!((cuts[1].time_sec - 0.5).abs() < 1e-4);
    }
}
