// SPDX-License-Identifier: AGPL-3.0-or-later
//! Explicit operator-run test of the real output device, not an AI/provider call.
use concat_host::playback::{Playback, PlaybackEvents};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

struct Events;
impl PlaybackEvents for Events {
    fn position(&self, _: f64) {}
    fn error(&self, message: String) {
        eprintln!("AUDIO_DEVICE: {message}");
    }
}
fn fixture() -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("concat-native-speech-{}.wav", std::process::id()));
    let samples = 48000u32 / 4;
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + samples * 4).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&48000u32.to_le_bytes());
    wav.extend_from_slice(&192000u32.to_le_bytes());
    wav.extend_from_slice(&4u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(samples * 4).to_le_bytes());
    for i in 0..samples {
        let sample = ((i as f32 * 440.0 * std::f32::consts::TAU / 48000.0).sin() * 50.0) as i16;
        wav.extend_from_slice(&sample.to_le_bytes());
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(&path, wav).unwrap();
    path
}
#[test]
#[ignore = "requires an actual speaker device; plays a quiet synthetic tone"]
fn cold_start_speech_uses_device_readiness_then_completes_once() {
    let path = fixture();
    let playback = Playback::start(Arc::new(Events)).unwrap();
    let clock = playback
        .speak_tracked_file(&path)
        .expect("cold-start speech must not race device initialization");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !clock.finished() {
        assert!(Instant::now() < deadline, "output did not complete");
        std::thread::yield_now();
    }
    assert!(clock.completed(), "device must consume the full utterance");
    assert!(
        playback
            .speak_synced_file_cancellable(&path, || true)
            .is_err(),
        "cancelled preparation must never enqueue"
    );
    let interrupted = playback.speak_synced_file(&path).unwrap();
    assert!(
        !interrupted.audio_ready(),
        "speech must wait for its first picture"
    );
    playback.stop_speech();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !interrupted.finished() {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(
        !interrupted.completed(),
        "interrupt must not report successful playback"
    );
    for _ in 0..3 {
        let next = playback.speak_tracked_file(&path).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !next.finished() {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(
            next.completed(),
            "next answer must still play after interruption"
        );
        assert!(
            clock.completed(),
            "new answers must not change an old completion receipt"
        );
    }
    assert_eq!(
        playback.position(),
        0.0,
        "speech must not move the editor timeline"
    );
    std::fs::remove_file(path).unwrap();
}
