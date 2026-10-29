// SPDX-License-Identifier: AGPL-3.0-or-later
//! Audio-device clock and first-picture barrier for a buffered LIVE utterance.
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

/// Shared by the video consumer and audio callback, without locks or UI access.
#[derive(Default)]
pub struct SpeechClock {
    state: AtomicU8,
    micros: AtomicU64,
}

impl SpeechClock {
    /// Audio is held until the UI has consumed the first decoded picture.
    pub fn acknowledge_picture(&self) {
        let _ = self
            .state
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire);
    }
    /// Whether audio may advance (a cancelled clock never becomes ready again).
    pub fn audio_ready(&self) -> bool {
        matches!(self.state.load(Ordering::Acquire), 1 | 2)
    }
    /// Device-reported position; never a separate video wall clock.
    pub fn seconds(&self) -> f64 {
        self.micros.load(Ordering::Acquire) as f64 / 1_000_000.0
    }
    /// Called by the audio callback after consuming these source seconds.
    pub fn advance(&self, seconds: f64) {
        if self.audio_ready() && seconds.is_finite() && seconds >= 0.0 {
            self.micros
                .store((seconds * 1_000_000.0) as u64, Ordering::Release);
        }
    }
    /// Complete or cancel this utterance, including while waiting for picture.
    pub fn finish(&self) {
        let _ = self
            .state
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |state| {
                (state < 3).then_some(3)
            });
    }
    /// Mark normal end-of-audio separately from cancellation/device loss.
    pub fn complete(&self) {
        let _ = self
            .state
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |state| {
                (state < 3).then_some(4)
            });
    }
    /// True only when the audio callback consumed the entire utterance.
    pub fn completed(&self) -> bool {
        self.state.load(Ordering::Acquire) == 4
    }
    /// Finished buffers must not loop, restart or release queued audio.
    pub fn finished(&self) -> bool {
        self.state.load(Ordering::Acquire) >= 3
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn picture_barrier_device_time_and_cancellation() {
        let clock = SpeechClock::default();
        assert!(!clock.audio_ready());
        clock.advance(2.0);
        assert_eq!(clock.seconds(), 0.0);
        clock.acknowledge_picture();
        assert!(clock.audio_ready());
        clock.advance(1.25);
        assert_eq!(clock.seconds(), 1.25);
        clock.advance(f64::NAN);
        assert_eq!(clock.seconds(), 1.25);
        clock.finish();
        clock.acknowledge_picture();
        assert!(clock.finished());
        assert!(!clock.audio_ready());
    }
    #[test]
    fn cancellation_before_picture_never_starts_audio() {
        let clock = SpeechClock::default();
        clock.finish();
        clock.acknowledge_picture();
        assert!(!clock.audio_ready());
    }
    #[test]
    fn completed_audio_is_not_mistaken_for_an_interruption() {
        let clock = SpeechClock::default();
        clock.acknowledge_picture();
        clock.advance(1.0);
        clock.complete();
        clock.finish(); // Dropping the consumed buffer must preserve completion.
        assert!(clock.completed());
        assert!(clock.finished());
        assert!(!clock.audio_ready());
        let cancelled = SpeechClock::default();
        cancelled.finish();
        cancelled.complete();
        assert!(!cancelled.completed());
    }
}
