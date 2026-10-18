// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat contributors
//! Complete, bounded native output decode and measured picture/audio diagnostics.
//! Findings are technical observations, never semantic or platform-quality certification.
use crate::output::QaReport;
use concat_media::{AudioDecoder, AudioOptions, DecodeOptions, Decoder, FrameSource, SampleFormat};
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

const MAX_FRAMES: usize = 20_000;
const AUDIO_RATE: u32 = 8000;
const MAX_AUDIO: u64 = 2_000_000;
fn check(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        Err("Cancelled".into())
    } else {
        Ok(())
    }
}

#[derive(Default)]
struct AudioStats {
    samples: u64,
    clipped: u64,
    bin_samples: u32,
    bin_power: f64,
    silent_run: f64,
    longest_silence: f64,
}
impl AudioStats {
    fn push(&mut self, sample: f32) -> Result<(), String> {
        if !sample.is_finite() {
            return Err("QA audio contains nonfinite samples".into());
        }
        self.samples += 1;
        if self.samples > MAX_AUDIO {
            return Err("QA audio decode exceeded bounded sample limit".into());
        }
        self.clipped += u64::from(sample.abs() >= 0.999);
        self.bin_samples += 1;
        self.bin_power += (sample as f64).powi(2);
        if self.bin_samples >= AUDIO_RATE {
            self.finish_bin()
        }
        Ok(())
    }
    fn finish_bin(&mut self) {
        if self.bin_samples == 0 {
            return;
        }
        let span = self.bin_samples as f64 / AUDIO_RATE as f64;
        let db = 20.0
            * (self.bin_power / self.bin_samples as f64)
                .sqrt()
                .max(1e-12)
                .log10();
        if db < -50.0 {
            self.silent_run += span;
            self.longest_silence = self.longest_silence.max(self.silent_run)
        } else {
            self.silent_run = 0.0
        }
        self.bin_power = 0.0;
        self.bin_samples = 0;
    }
    fn fraction(&self) -> f64 {
        if self.samples == 0 {
            0.0
        } else {
            self.clipped as f64 / self.samples as f64
        }
    }
}
fn near_black(pixels: &[u8]) -> bool {
    if pixels.is_empty() || pixels.len() % 4 != 0 {
        return false;
    }
    let dark = pixels
        .chunks_exact(4)
        .filter(|p| p[0] < 8 && p[1] < 8 && p[2] < 8)
        .count();
    dark as f64 / (pixels.len() / 4) as f64 >= 0.98
}
fn warnings(black: f64, audio: &AudioStats) -> Vec<String> {
    let mut result = Vec::new();
    if black >= 1.0 {
        result.push(format!("Measured {black:.2}s of near-black picture (>=98% pixels below8/255); review whether intentional."))
    }
    if audio.longest_silence >= 3.0 {
        result.push(format!(
            "Measured longest mono RMS silence {:.2}s below-50dBFS; review whether intentional.",
            audio.longest_silence
        ))
    }
    if audio.fraction() > 0.01 {
        result.push(format!(
            "Measured {:.2}% of decoded mono samples at|amplitude|>=0.999; possible clipping.",
            audio.fraction() * 100.0
        ))
    }
    result
}
fn scaled(width: u32, height: u32) -> (u32, u32) {
    let ratio = (160.0 / width as f64).min(160.0 / height as f64).min(1.0);
    // Downsample only; at most one pixel rounding error, no square stretch.
    (
        (width as f64 * ratio).round().max(1.0) as u32,
        (height as f64 * ratio).round().max(1.0) as u32,
    )
}

/// Fully decodes a <=180-second native shorts export, preserving picture aspect in diagnostics.
/// Stream absence/truncation/corruption, invalid numbers and cancellation fail; real visual/audio
/// findings are warnings because deliberate black/silence/clipping cannot be inferred from pixels.
pub(crate) fn inspect(
    path: &Path,
    expected: f64,
    width: u32,
    height: u32,
    want_audio: bool,
    cancel: &AtomicBool,
) -> Result<QaReport, String> {
    check(cancel)?;
    if !expected.is_finite() || expected <= 0.0 || expected > 180.0 || width == 0 || height == 0 {
        return Err("QA expected output bounds are invalid".into());
    }
    let meta = std::fs::symlink_metadata(path).map_err(|_| "QA output file is missing")?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() == 0 {
        return Err("QA output is not a nonempty ordinary file".into());
    }
    let probe = concat_host::media::probe(&path.to_string_lossy())
        .map_err(|_| "QA cannot probe rendered output")?;
    let video = probe.video.ok_or("QA output has no video stream")?;
    let duration = probe.duration.ok_or("QA output has no duration")?;
    if !duration.is_finite()
        || duration <= 0.0
        || duration > 180.2
        || video.width != width
        || video.height != height
        || (duration - expected).abs() > 0.15
    {
        return Err("QA output dimensions/duration do not match approved edit".into());
    }
    let fps = video.frame_rate;
    if !fps.is_finite() || !(1.0..=240.0).contains(&fps) || expected * fps > MAX_FRAMES as f64 {
        return Err("QA output frame rate or frame count exceeds safe bounds".into());
    }
    if want_audio && probe.audio.is_none() {
        return Err("QA output lost the source audio stream".into());
    }
    let (small_width, small_height) = scaled(width, height);
    let mut decoder = Decoder::open(
        path,
        &DecodeOptions::default()
            .scaled_to(small_width, small_height)
            .threaded(2)
            .limited_to(MAX_FRAMES as u64 + 1),
    )
    .map_err(|_| "QA video decoder failed to open")?;
    let mut frames = 0usize;
    let mut black_frames = 0usize;
    let mut previous_pts: Option<f64> = None;
    let tolerance = (2.0 / fps).max(0.15);
    loop {
        check(cancel)?;
        let Some(frame) = decoder
            .next_frame()
            .map_err(|_| "QA video contains undecodable frames")?
        else {
            break;
        };
        frames += 1;
        if frames > MAX_FRAMES {
            return Err("QA video exceeded bounded frame decode limit".into());
        }
        let pts = decoder
            .position()
            .ok_or("QA video frame has no real timestamp")?
            .as_f64();
        if !pts.is_finite()
            || pts < 0.0
            || pts > duration + tolerance
            || previous_pts.is_some_and(|p| pts < p - 1e-9)
            || previous_pts.is_none() && pts > tolerance
        {
            return Err("QA video contains invalid or missing source-clock coverage".into());
        }
        if previous_pts.is_some_and(|p| pts - p > tolerance) {
            return Err("QA video timestamp gap exceeds bounded frame coverage".into());
        }
        previous_pts = Some(pts);
        if near_black(frame.pixels()) {
            black_frames += 1
        }
    }
    let decoded_duration = frames as f64 / fps;
    if frames == 0
        || (decoded_duration - expected).abs() > tolerance
        || previous_pts.is_none_or(|p| expected - (p + 1.0 / fps) > tolerance)
    {
        return Err("QA video ended before approved duration or contains extra frames".into());
    }
    let mut audio = AudioStats::default();
    if probe.audio.is_some() {
        let mut reader = AudioDecoder::open(
            path,
            &AudioOptions {
                rate: AUDIO_RATE,
                channels: 1,
                format: SampleFormat::F32,
                ..Default::default()
            },
        )
        .map_err(|_| "QA audio decoder failed to open")?;
        while let Some(frame) = reader
            .next_f32()
            .map_err(|_| "QA audio contains undecodable samples")?
        {
            check(cancel)?;
            for sample in frame {
                audio.push(sample)?
            }
        }
        audio.finish_bin();
        let seconds = audio.samples as f64 / AUDIO_RATE as f64;
        if audio.samples == 0 || (seconds - expected).abs() > 0.25 {
            return Err("QA audio is empty, truncated or exceeds approved duration".into());
        }
    }
    check(cancel)?;
    let near_black_seconds = (black_frames as f64 / fps).min(duration);
    let warnings = warnings(near_black_seconds, &audio);
    Ok(QaReport {
        width,
        height,
        duration,
        decoded_samples: frames,
        has_audio: probe.audio.is_some(),
        warnings,
        full_decode: true,
        audio_samples: audio.samples,
        near_black_seconds,
        longest_silence_seconds: audio.longest_silence,
        clipped_fraction: audio.fraction(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn silence_is_measured_in_contiguous_bins_and_resets() {
        let mut s = AudioStats::default();
        for _ in 0..AUDIO_RATE * 4 {
            s.push(0.0).unwrap()
        }
        for _ in 0..AUDIO_RATE {
            s.push(0.5).unwrap()
        }
        for _ in 0..AUDIO_RATE * 2 {
            s.push(0.0).unwrap()
        }
        s.finish_bin();
        assert_eq!(s.longest_silence, 4.0);
        assert_eq!(s.samples, AUDIO_RATE as u64 * 7);
        assert!(warnings(0.0, &s).iter().any(|w| w.contains("4.00s")));
    }
    #[test]
    fn partial_silence_bin_uses_real_sample_duration() {
        let mut s = AudioStats::default();
        for _ in 0..AUDIO_RATE / 2 {
            s.push(0.0).unwrap()
        }
        s.finish_bin();
        assert_eq!(s.longest_silence, 0.5);
        assert!(warnings(0.0, &s).is_empty());
    }
    #[test]
    fn clipping_fraction_is_actual_amplitude_observation() {
        let mut s = AudioStats::default();
        for _ in 0..100 {
            s.push(1.0).unwrap()
        }
        for _ in 0..900 {
            s.push(0.25).unwrap()
        }
        assert!((s.fraction() - 0.1).abs() < 1e-9);
        assert!(warnings(0.0, &s).iter().any(|w| w.contains("10.00%")));
        assert!(s.push(f32::NAN).is_err());
    }
    #[test]
    fn black_detection_is_pixel_measured() {
        assert!(near_black(&[0, 0, 0, 255].repeat(100)));
        assert!(!near_black(&[128, 128, 128, 255].repeat(100)));
        assert!(!near_black(&[]));
        assert!(!warnings(2.0, &AudioStats::default()).is_empty());
    }
    #[test]
    fn diagnostic_resize_preserves_portrait_aspect() {
        assert_eq!(scaled(1080, 1920), (90, 160));
        assert_eq!(scaled(1920, 1080), (160, 90));
    }
    #[test]
    fn cancellation_precedes_any_file_access() {
        assert!(
            inspect(
                Path::new("missing"),
                1.0,
                1080,
                1920,
                false,
                &AtomicBool::new(true)
            )
            .unwrap_err()
            .contains("Cancelled")
        );
    }
}
