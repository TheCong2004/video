// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat contributors

//! Word-level ASR alignment, transcript edits, filler removal, and silence compression.
//! Preserves source-time/revision binding so that edits on the transcript map
//! deterministically into native timeline commands.

use serde::{Deserialize, Serialize};

/// Word token with millisecond or second source timing and optional speaker.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptWord {
    /// Text of the individual spoken word.
    pub text: String,
    /// Start time in seconds from the beginning of the media.
    pub start: f64,
    /// End time in seconds from the beginning of the media.
    pub end: f64,
    /// Optional speaker identity label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
}

/// Fixed filler words for mechanical cleanup across English and Vietnamese.
const ENGLISH_FILLERS: &[&str] = &[
    "um", "umm", "uh", "uhh", "uhm", "er", "erm", "ah", "hmm", "mmm",
];

const VIETNAMESE_FILLERS: &[&str] = &[
    "ừm", "ờ", "à", "thì", "là", "mà", "nhỉ", "nhé", "vậy", "dạ", "hả", "nè", "á", "hén",
];

/// Normalizes token text and tests whether it is a filler word.
pub fn is_filler(text: &str) -> bool {
    let normalized = text
        .trim()
        .to_lowercase()
        .replace(|c: char| c.is_ascii_punctuation(), "");
    if normalized.is_empty() {
        return false;
    }
    ENGLISH_FILLERS.contains(&normalized.as_str())
        || VIETNAMESE_FILLERS.contains(&normalized.as_str())
}

/// Identifies all filler word indices in a word sequence.
pub fn filler_indices(words: &[TranscriptWord]) -> Vec<usize> {
    words
        .iter()
        .enumerate()
        .filter_map(|(idx, w)| if is_filler(&w.text) { Some(idx) } else { None })
        .collect()
}

/// Detects silence gaps between consecutive words exceeding `threshold_seconds`.
/// Returns list of `(start_time, end_time)` in source seconds.
pub fn find_silence_gaps(words: &[TranscriptWord], threshold_seconds: f64) -> Vec<(f64, f64)> {
    if words.len() < 2 {
        return Vec::new();
    }
    let mut gaps = Vec::new();
    for window in words.windows(2) {
        let prev_end = window[0].end;
        let next_start = window[1].start;
        if next_start > prev_end + threshold_seconds {
            gaps.push((prev_end, next_start));
        }
    }
    gaps
}

/// Silence compression rule.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SilenceMode {
    /// Compress any silence longer than max_gap to exactly target_gap.
    Compress {
        /// Maximum tolerated silence duration in seconds.
        max_gap: f64,
        /// Target compressed duration in seconds.
        target_gap: f64,
    },
    /// Remove silence gaps completely (leaving 0 gap).
    RemoveAll {
        /// Silence duration threshold in seconds above which silence is removed.
        threshold: f64,
    },
}

/// Computes source-time ranges to cut from a clip given deleted word indices and silence gaps.
/// Output ranges are (source_start, source_end) in seconds.
pub fn compute_cut_ranges(
    words: &[TranscriptWord],
    deleted_word_indices: &[usize],
    silence_gaps: &[(f64, f64)],
) -> Vec<(f64, f64)> {
    let mut raw_ranges = Vec::new();

    for &idx in deleted_word_indices {
        if let Some(word) = words.get(idx) {
            raw_ranges.push((word.start, word.end));
        }
    }

    for &gap in silence_gaps {
        raw_ranges.push(gap);
    }

    if raw_ranges.is_empty() {
        return Vec::new();
    }

    raw_ranges.sort_by(|a, b| a.0.total_cmp(&b.0));

    // Merge overlapping or contiguous cut ranges
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for (start, end) in raw_ranges {
        if let Some(last) = merged.last_mut() {
            if start <= last.1 + 1e-6 {
                last.1 = last.1.max(end);
                continue;
            }
        }
        merged.push((start, end));
    }

    merged
}

/// Maps source cut ranges into timeline cut spans for a specific placed clip.
/// Takes into account the clip's `timeline_start`, `source_start`, and `speed`.
pub fn map_source_cuts_to_timeline(
    source_cuts: &[(f64, f64)],
    clip_timeline_start: f64,
    clip_source_start: f64,
    clip_duration: f64,
    clip_speed: f64,
) -> Vec<(f64, f64)> {
    let speed = if clip_speed <= 0.0 { 1.0 } else { clip_speed };
    let clip_source_end = clip_source_start + clip_duration * speed;

    let mut timeline_cuts = Vec::new();

    for &(src_start, src_end) in source_cuts {
        let clamped_start = src_start.max(clip_source_start);
        let clamped_end = src_end.min(clip_source_end);

        if clamped_end > clamped_start + 1e-6 {
            let tl_start = clip_timeline_start + (clamped_start - clip_source_start) / speed;
            let tl_end = clip_timeline_start + (clamped_end - clip_source_start) / speed;
            timeline_cuts.push((tl_start, tl_end));
        }
    }

    timeline_cuts
}

/// One formatted subtitle line for Vietnamese or other languages.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleCue {
    /// Formatted text.
    pub text: String,
    /// Timeline start in seconds.
    pub start: f64,
    /// Timeline end in seconds.
    pub end: f64,
}

/// Segments a word stream into displayable subtitle cues adhering to Vietnamese
/// phrase boundaries and display duration constraints (e.g. max 10 words, max 45 chars, pause > 350ms).
pub fn segment_subtitles(
    words: &[TranscriptWord],
    max_words_per_cue: usize,
    max_chars_per_cue: usize,
    pause_split_threshold: f64,
) -> Vec<SubtitleCue> {
    if words.is_empty() {
        return Vec::new();
    }

    let mut cues = Vec::new();
    let mut current_words: Vec<&TranscriptWord> = Vec::new();

    for (i, word) in words.iter().enumerate() {
        let is_pause = if let Some(last) = current_words.last() {
            word.start - last.end > pause_split_threshold
        } else {
            false
        };

        let current_text_len: usize = current_words.iter().map(|w| w.text.len() + 1).sum();
        let would_exceed_chars = current_text_len + word.text.len() > max_chars_per_cue;
        let would_exceed_words = current_words.len() >= max_words_per_cue;

        let is_punct = word.text.ends_with('.') || word.text.ends_with('?') || word.text.ends_with('!') || word.text.ends_with(';');

        if !current_words.is_empty() && (is_pause || would_exceed_words || would_exceed_chars) {
            let start = current_words.first().unwrap().start;
            let end = current_words.last().unwrap().end;
            let text = current_words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
            cues.push(SubtitleCue { text, start, end });
            current_words.clear();
        }

        current_words.push(word);

        if is_punct && current_words.len() >= 3 && i < words.len() - 1 {
            let start = current_words.first().unwrap().start;
            let end = current_words.last().unwrap().end;
            let text = current_words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
            cues.push(SubtitleCue { text, start, end });
            current_words.clear();
        }
    }

    if !current_words.is_empty() {
        let start = current_words.first().unwrap().start;
        let end = current_words.last().unwrap().end;
        let text = current_words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
        cues.push(SubtitleCue { text, start, end });
    }

    cues
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filler_detection_multilingual() {
        assert!(is_filler("um"));
        assert!(is_filler("Uhm,"));
        assert!(is_filler("ừm"));
        assert!(is_filler("à..."));
        assert!(is_filler("thì"));
        assert!(!is_filler("Việt Nam"));
        assert!(!is_filler("video"));

        let words = vec![
            TranscriptWord { text: "Xin".into(), start: 0.0, end: 0.3, speaker: None },
            TranscriptWord { text: "chào".into(), start: 0.35, end: 0.6, speaker: None },
            TranscriptWord { text: "ừm".into(), start: 0.7, end: 0.9, speaker: None },
            TranscriptWord { text: "các".into(), start: 1.0, end: 1.2, speaker: None },
            TranscriptWord { text: "bạn".into(), start: 1.25, end: 1.5, speaker: None },
        ];
        let fillers = filler_indices(&words);
        assert_eq!(fillers, vec![2]);
    }

    #[test]
    fn test_silence_gap_detection_and_cut_computation() {
        let words = vec![
            TranscriptWord { text: "Hello".into(), start: 0.0, end: 0.5, speaker: None },
            TranscriptWord { text: "world".into(), start: 2.0, end: 2.5, speaker: None },
            TranscriptWord { text: "again".into(), start: 2.6, end: 3.0, speaker: None },
        ];
        let gaps = find_silence_gaps(&words, 0.5);
        assert_eq!(gaps, vec![(0.5, 2.0)]);

        let cuts = compute_cut_ranges(&words, &[], &gaps);
        assert_eq!(cuts, vec![(0.5, 2.0)]);

        let timeline_cuts = map_source_cuts_to_timeline(&cuts, 10.0, 0.0, 5.0, 1.0);
        assert_eq!(timeline_cuts, vec![(10.5, 12.0)]);
    }

    #[test]
    fn test_subtitle_segmentation_vietnamese() {
        let words = vec![
            TranscriptWord { text: "Hôm".into(), start: 0.0, end: 0.2, speaker: None },
            TranscriptWord { text: "nay".into(), start: 0.22, end: 0.4, speaker: None },
            TranscriptWord { text: "chúng".into(), start: 0.42, end: 0.6, speaker: None },
            TranscriptWord { text: "ta".into(), start: 0.62, end: 0.8, speaker: None },
            TranscriptWord { text: "học".into(), start: 0.82, end: 1.0, speaker: None },
            TranscriptWord { text: "lập".into(), start: 1.02, end: 1.2, speaker: None },
            TranscriptWord { text: "trình.".into(), start: 1.22, end: 1.5, speaker: None },
            TranscriptWord { text: "Chúc".into(), start: 2.5, end: 2.8, speaker: None },
            TranscriptWord { text: "vui".into(), start: 2.82, end: 3.0, speaker: None },
            TranscriptWord { text: "vẻ.".into(), start: 3.02, end: 3.3, speaker: None },
        ];
        let cues = segment_subtitles(&words, 10, 50, 0.5);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "Hôm nay chúng ta học lập trình.");
        assert_eq!(cues[1].text, "Chúc vui vẻ.");
    }
}
