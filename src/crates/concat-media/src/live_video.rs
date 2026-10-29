// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded, silent video transport for the native LIVE preview/output.

use crate::{DecodeOptions, Decoder, FrameSource, SeekableSource};
use concat_core::frame::Frame;
use concat_core::speech_clock::SpeechClock;
use concat_core::time::{FrameRate, Rational};
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

enum Command {
    Load(u64, PathBuf, Option<Arc<SpeechClock>>),
    Stop(u64),
}

/// Newest frame only: a slow UI must never accumulate video frames.
pub struct LiveVideo {
    commands: mpsc::Sender<Command>,
    latest: Arc<Mutex<Option<Update>>>,
    generation: AtomicU64,
    clock: Mutex<Option<Arc<SpeechClock>>>,
}

impl LiveVideo {
    /// Start a decoder worker, without touching the editor's timeline.
    pub fn new() -> Self {
        let (commands, rx) = mpsc::channel();
        let latest = Arc::new(Mutex::new(None));
        let mailbox = Arc::clone(&latest);
        let spawned = std::thread::Builder::new()
            .name("concat-live-video".into())
            .spawn(move || run(rx, mailbox));
        if spawned.is_err() {
            log::warn!("could not start LIVE video decoder");
        }
        Self {
            commands,
            latest,
            generation: AtomicU64::new(0),
            clock: Mutex::new(None),
        }
    }
    /// Select a local video; a new selection replaces the old one.
    pub fn load(&self, path: PathBuf) -> u64 {
        self.load_inner(path, None)
    }
    /// Play a complete reply once, driven by the audio device, never looping it.
    pub fn load_synced(&self, path: PathBuf, clock: Arc<SpeechClock>) -> u64 {
        self.load_inner(path, Some(clock))
    }
    fn load_inner(&self, path: PathBuf, clock: Option<Arc<SpeechClock>>) -> u64 {
        if let Ok(mut current) = self.clock.lock() {
            if let Some(old) = current.take() {
                old.finish();
            }
            *current = clock.clone();
        }
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        if self
            .commands
            .send(Command::Load(generation, path, clock))
            .is_err()
        {
            if let Ok(current) = self.clock.lock() {
                if let Some(clock) = current.as_ref() {
                    clock.finish();
                }
            }
            publish(
                &self.latest,
                Update {
                    generation,
                    frame: None,
                    error: Some("LIVE video worker unavailable"),
                },
            );
        }
        generation
    }
    /// Stop and release the decoder.
    pub fn stop(&self) -> u64 {
        if let Ok(mut current) = self.clock.lock() {
            if let Some(clock) = current.take() {
                clock.finish();
            }
        }
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let _ = self.commands.send(Command::Stop(generation));
        publish(
            &self.latest,
            Update {
                generation,
                frame: None,
                error: None,
            },
        );
        generation
    }
    /// Take the newest result, if available.
    pub fn take(&self) -> Option<Update> {
        let update = self
            .latest
            .lock()
            .ok()?
            .take()
            .filter(|u| u.generation == self.generation.load(Ordering::Acquire));
        if let Some(update) = &update {
            if let Ok(current) = self.clock.lock() {
                if let Some(clock) = current.as_ref() {
                    if update.frame.is_some() {
                        clock.acknowledge_picture();
                    } else {
                        clock.finish();
                    }
                }
            }
        }
        update
    }
}

impl Default for LiveVideo {
    fn default() -> Self {
        Self::new()
    }
}
impl Drop for LiveVideo {
    fn drop(&mut self) {
        if let Ok(current) = self.clock.lock() {
            if let Some(clock) = current.as_ref() {
                clock.finish();
            }
        }
    }
}

fn publish(mailbox: &Mutex<Option<Update>>, update: Update) {
    if let Ok(mut slot) = mailbox.lock() {
        *slot = Some(update);
    }
}

fn open(path: &std::path::Path) -> Result<Decoder, &'static str> {
    if !path.is_file() {
        return Err("Video không tồn tại trên máy.");
    }
    let info = crate::probe(path).map_err(|_| "Không đọc được video.")?;
    let video = info.video.ok_or("Tệp không có hình video.")?;
    if video.width == 0 || video.height == 0 {
        return Err("Kích thước video không hợp lệ.");
    }
    // Preserve aspect ratio. Never decode 4K into a small broadcast preview.
    let (width, height) = decode_size(video.width, video.height);
    Decoder::open(
        path,
        &DecodeOptions::default()
            .scaled_to(width, height)
            .at_rate(FrameRate::THIRTY)
            .threaded(2),
    )
    .map_err(|_| "Không mở được bộ giải mã video.")
}

fn decode_size(width: u32, height: u32) -> (u32, u32) {
    let (limit_w, limit_h) = if width >= height {
        (1280.0, 720.0)
    } else {
        (720.0, 1280.0)
    };
    let scale = (limit_w / f64::from(width))
        .min(limit_h / f64::from(height))
        .min(1.0);
    (
        ((f64::from(width) * scale) as u32).max(2) & !1,
        ((f64::from(height) * scale) as u32).max(2) & !1,
    )
}

fn run(rx: mpsc::Receiver<Command>, mailbox: Arc<Mutex<Option<Update>>>) {
    let interval = Duration::from_nanos(1_000_000_000 / 30);
    let mut decoder: Option<Decoder> = None;
    let mut generation = 0;
    let mut due = Instant::now();
    let mut clock: Option<Arc<SpeechClock>> = None;
    let mut picture_index = 0u64;
    loop {
        let command = if decoder.is_none() {
            match rx.recv() {
                Ok(c) => Some(c),
                Err(_) => break,
            }
        } else {
            match rx.recv_timeout(due.saturating_duration_since(Instant::now())) {
                Ok(c) => Some(c),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        };
        if let Some(mut command) = command {
            // Coalesce rapid selection changes before opening another decoder.
            while let Ok(newer) = rx.try_recv() {
                command = newer;
            }
            decoder = None;
            match command {
                Command::Stop(id) => {
                    generation = id;
                    publish(
                        &mailbox,
                        Update {
                            generation,
                            frame: None,
                            error: None,
                        },
                    );
                }
                Command::Load(id, path, next_clock) => {
                    generation = id;
                    clock = next_clock;
                    picture_index = 0;
                    match open(&path) {
                        Ok(d) => decoder = Some(d),
                        Err(error) => publish(
                            &mailbox,
                            Update {
                                generation,
                                frame: None,
                                error: Some(error),
                            },
                        ),
                    }
                }
            }
            due = Instant::now();
            continue;
        }
        let Some(active) = decoder.as_mut() else {
            continue;
        };
        if let Some(clock) = &clock {
            if clock.finished() {
                decoder = None;
                continue;
            }
            if picture_index > 0 && clock.seconds() * 30.0 < picture_index as f64 {
                due = Instant::now() + Duration::from_millis(3);
                continue;
            }
        }
        let frame = match active.next_frame() {
            Ok(Some(frame)) => Ok(Some(frame)),
            Ok(None) if clock.is_none() => active
                .seek(Rational::ZERO)
                .and_then(|()| active.next_frame()),
            Ok(None) => {
                decoder = None;
                continue;
            }
            Err(error) => Err(error),
        };
        picture_index += 1;
        match frame {
            Ok(Some(frame)) => publish(
                &mailbox,
                Update {
                    generation,
                    frame: Some(frame),
                    error: None,
                },
            ),
            _ => {
                publish(
                    &mailbox,
                    Update {
                        generation,
                        frame: None,
                        error: Some("Video trống hoặc không giải mã được."),
                    },
                );
                decoder = None;
            }
        }
        // Do not burst old frames to catch up after slow decoding.
        due = if clock.is_some() {
            Instant::now()
        } else {
            (due + interval).max(Instant::now())
        };
    }
}

/// Decoder result tagged with its selection generation.
pub struct Update {
    /// Selection that produced this result.
    pub generation: u64,
    /// Decoded picture, or none for a stopped/error state.
    pub frame: Option<Frame>,
    /// Safe diagnostic; no paths or upstream response bodies.
    pub error: Option<&'static str>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hd_landscape_and_portrait_are_not_unnecessarily_downscaled() {
        assert_eq!(decode_size(1280, 720), (1280, 720));
        assert_eq!(decode_size(720, 1280), (720, 1280));
        assert_eq!(decode_size(1024, 576), (1024, 576));
        assert_eq!(decode_size(1920, 1080), (1280, 720));
    }
    fn wait(player: &LiveVideo) -> Update {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(update) = player.take() {
                return update;
            }
            assert!(
                Instant::now() < deadline,
                "decoder must produce a bounded result"
            );
            std::thread::yield_now();
        }
    }

    #[test]
    fn real_video_changes_pixels_loops_and_switches_without_stale_frames() {
        use crate::{EncodeOptions, Encoder, FrameSink};
        let path =
            std::env::temp_dir().join(format!("concat-live-motion-{}.mp4", std::process::id()));
        let mut encoder = Encoder::create(
            &path,
            64,
            96,
            FrameRate::THIRTY,
            &EncodeOptions {
                preset: "ultrafast".into(),
                threads: 1,
                hardware: false,
                ..Default::default()
            },
        )
        .unwrap();
        for i in 0..6 {
            let mut frame = Frame::black(64, 96);
            for pixel in frame.pixels_mut().chunks_exact_mut(4) {
                pixel[0] = i * 40;
            }
            encoder.write_frame(&frame).unwrap();
        }
        encoder.finish().unwrap();
        drop(encoder);
        let player = LiveVideo::new();
        let generation = player.load(path.clone());
        let mut first = None;
        let mut changed = false;
        let mut wrapped = false;
        for _ in 0..16 {
            let update = wait(&player);
            assert_eq!(update.generation, generation);
            assert!(update.error.is_none(), "{:?}", update.error);
            let frame = update.frame.unwrap();
            assert_eq!((frame.width(), frame.height()), (64, 96));
            let value = frame.pixels()[0];
            let initial = *first.get_or_insert(value);
            if value.abs_diff(initial) > 15 {
                changed = true;
            }
            if changed && value.abs_diff(initial) < 5 {
                wrapped = true;
            }
        }
        assert!(changed, "not a static avatar");
        assert!(wrapped, "EOF must rewind, not freeze the last frame");
        let next = player.load(PathBuf::from("missing-switch.mp4"));
        let update = wait(&player);
        assert_eq!(update.generation, next);
        assert!(update.error.is_some());
        player.stop();
        drop(player);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn missing_clip_reports_error_without_a_frame() {
        let player = LiveVideo::new();
        let generation = player.load(PathBuf::from("missing-live-clip-does-not-exist.mp4"));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            if let Some(update) = player.take() {
                assert_eq!(update.generation, generation);
                assert!(update.frame.is_none());
                assert!(update.error.is_some());
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
    #[test]
    fn stop_invalidates_old_frames_and_returns_stopped() {
        let player = LiveVideo::new();
        let first = player.load(PathBuf::from("missing.mp4"));
        let stopped = player.stop();
        assert!(stopped > first);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            if let Some(update) = player.take() {
                assert_eq!(update.generation, stopped);
                assert!(update.frame.is_none());
                assert!(update.error.is_none());
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
    #[test]
    fn reply_video_waits_for_device_time_and_stop_cancels_audio() {
        use crate::{EncodeOptions, Encoder, FrameSink};
        let path =
            std::env::temp_dir().join(format!("concat-synced-motion-{}.mp4", std::process::id()));
        let mut encoder = Encoder::create(
            &path,
            64,
            96,
            FrameRate::THIRTY,
            &EncodeOptions {
                preset: "ultrafast".into(),
                threads: 1,
                hardware: false,
                ..Default::default()
            },
        )
        .unwrap();
        for i in 0..6 {
            let mut frame = Frame::black(64, 96);
            for pixel in frame.pixels_mut().chunks_exact_mut(4) {
                pixel[0] = i * 40;
            }
            encoder.write_frame(&frame).unwrap();
        }
        encoder.finish().unwrap();
        drop(encoder);
        let player = LiveVideo::new();
        let clock = Arc::new(SpeechClock::default());
        player.load_synced(path.clone(), clock.clone());
        let first = wait(&player).frame.unwrap();
        assert!(
            clock.audio_ready(),
            "first consumed picture releases the audio barrier"
        );
        let deadline = Instant::now() + Duration::from_millis(80);
        while Instant::now() < deadline {
            assert!(
                player.take().is_none(),
                "video must not run ahead of the paused audio device"
            );
            std::thread::yield_now();
        }
        for index in 1..6 {
            clock.advance(index as f64 / 30.0 + 0.0001);
            let next = wait(&player).frame.unwrap();
            assert!(next.pixels()[0].abs_diff(first.pixels()[0]) > 15);
        }
        clock.advance(0.3);
        let deadline = Instant::now() + Duration::from_millis(80);
        while Instant::now() < deadline {
            assert!(
                player.take().is_none(),
                "a speech reply must never wrap back to its first frame"
            );
            std::thread::yield_now();
        }
        player.stop();
        assert!(clock.finished());
        assert!(!clock.completed(), "a stop is not a successful utterance");
        drop(player);
        // Decoder closes asynchronously. Use a bounded retry for Windows locks.
        let deadline = Instant::now() + Duration::from_secs(2);
        while std::fs::remove_file(&path).is_err() {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
}
