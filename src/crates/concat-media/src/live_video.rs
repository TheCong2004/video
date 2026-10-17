// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded, silent video transport for the native LIVE preview/output.

use crate::{DecodeOptions, Decoder, FrameSource, SeekableSource};
use concat_core::frame::Frame;
use concat_core::time::{FrameRate, Rational};
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

enum Command {
    Load(u64, PathBuf),
    Stop(u64),
}

/// Newest frame only: a slow UI must never accumulate video frames.
pub struct LiveVideo {
    commands: mpsc::Sender<Command>,
    latest: Arc<Mutex<Option<Update>>>,
    generation: AtomicU64,
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
        }
    }
    /// Select a local video; a new selection replaces the old one.
    pub fn load(&self, path: PathBuf) -> u64 {
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        if self.commands.send(Command::Load(generation, path)).is_err() {
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
        self.latest
            .lock()
            .ok()?
            .take()
            .filter(|u| u.generation == self.generation.load(Ordering::Acquire))
    }
}

impl Default for LiveVideo {
    fn default() -> Self {
        Self::new()
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
    let scale = (720.0 / f64::from(video.width))
        .min(1280.0 / f64::from(video.height))
        .min(1.0);
    let width = ((f64::from(video.width) * scale) as u32).max(2) & !1;
    let height = ((f64::from(video.height) * scale) as u32).max(2) & !1;
    Decoder::open(
        path,
        &DecodeOptions::default()
            .scaled_to(width, height)
            .at_rate(FrameRate::THIRTY)
            .threaded(2),
    )
    .map_err(|_| "Không mở được bộ giải mã video.")
}

fn run(rx: mpsc::Receiver<Command>, mailbox: Arc<Mutex<Option<Update>>>) {
    let interval = Duration::from_nanos(1_000_000_000 / 30);
    let mut decoder: Option<Decoder> = None;
    let mut generation = 0;
    let mut due = Instant::now();
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
                Command::Load(id, path) => {
                    generation = id;
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
        let frame = match active.next_frame() {
            Ok(Some(frame)) => Ok(Some(frame)),
            Ok(None) => active
                .seek(Rational::ZERO)
                .and_then(|()| active.next_frame()),
            Err(error) => Err(error),
        };
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
        due = (due + interval).max(Instant::now());
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
}
