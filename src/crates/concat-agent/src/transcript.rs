// SPDX-License-Identifier: AGPL-3.0-or-later
// Adapted from HotClip src/shared/{transcript-search,edit-transcript,speech-text}.ts
// and src/core/retakes.ts.
// at 4f4347d12c8a989d16be46aaeed7455d1f35f992. Native cue-only adaptation.
//! Bounded transcript review. Text edits never invent word times or move source boundaries.
use crate::highlight::{Boundary, Evidence, Highlight, Options, SourceRange, TimedSegment};
use std::collections::{BTreeMap, BTreeSet};
use unicode_normalization_alignments::UnicodeNormalization;

// Preserve accents: this is case-insensitive literal matching, not accent-insensitive or
// fuzzy semantic search. As in HotClip, punctuation/spacing is ignored for matching only.
fn normalized(text: &str) -> String {
    text.nfkc()
        .map(|(c, _)| c)
        .flat_map(char::to_lowercase)
        .nfc()
        .map(|(c, _)| c)
        .filter(|c| c.is_alphanumeric() || ('\u{0300}'..='\u{036f}').contains(c))
        .collect()
}

/// Export source-timed cues as a new UTF-8 SRT. No word timings are synthesized.
pub fn to_srt(cues: &[TimedSegment]) -> String {
    fn time(seconds: f64) -> String {
        let ms = (seconds.max(0.0) * 1000.0).round() as u64;
        format!(
            "{:02}:{:02}:{:02},{:03}",
            ms / 3_600_000,
            ms / 60_000 % 60,
            ms / 1000 % 60,
            ms % 1000
        )
    }
    let mut result = String::new();
    for (i, c) in cues.iter().enumerate() {
        let text = c.text.split_whitespace().collect::<Vec<_>>().join(" ");
        result.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            i + 1,
            time(c.start),
            time(c.end),
            text
        ));
    }
    result
}

/// Session-local review with bounded text-only undo history.
pub struct Review {
    cues: Vec<TimedSegment>,
    undo: Vec<(usize, String)>,
    redo: Vec<(usize, String)>,
    index: String,
    offsets: Vec<(usize, usize)>,
}

/// Similar nearby source cues: a review hint, never permission to delete either take.
#[derive(Clone, Debug)]
pub struct Repetition {
    /// Earlier cue ID (not a vector index).
    pub earlier: u32,
    /// Later cue ID, within two cues and twenty source seconds.
    pub later: u32,
    /// Character-bigram Dice similarity, not a probability or quality score.
    pub similarity: f64,
}

impl Review {
    /// HotClip-style nearby retake hints. Bounded to 2,000 pairs; no semantic/word claims.
    pub fn repetitions(&self) -> Vec<Repetition> {
        fn bigrams(chars: &[char]) -> BTreeMap<(char, char), usize> {
            let mut counts = BTreeMap::new();
            for pair in chars.windows(2) {
                *counts.entry((pair[0], pair[1])).or_default() += 1;
            }
            counts
        }
        let mut hits = Vec::new();
        for (i, cue) in self.cues.iter().enumerate() {
            let (a, b) = self.offsets[i];
            let chars = self.index[a..b].chars().collect::<Vec<_>>();
            if chars.len() < 6 {
                continue;
            }
            let grams = bigrams(&chars);
            for j in i + 1..(i + 3).min(self.cues.len()) {
                let next = &self.cues[j];
                if next.start - cue.end > 20.0 {
                    break;
                }
                let (a, b) = self.offsets[j];
                let other = self.index[a..b].chars().collect::<Vec<_>>();
                if other.len() < 6 {
                    continue;
                }
                let counts = bigrams(&other);
                let shared = grams
                    .iter()
                    .map(|(pair, n)| (*n).min(*counts.get(pair).unwrap_or(&0)))
                    .sum::<usize>();
                let similarity = 2.0 * shared as f64 / (chars.len() + other.len() - 2) as f64;
                if similarity >= 0.72 {
                    hits.push(Repetition {
                        earlier: cue.id,
                        later: next.id,
                        similarity,
                    });
                    break;
                }
            }
            if hits.len() == 2000 {
                break;
            }
        }
        hits
    }
    /// Validate imported source-time cues before exposing them for review.
    pub fn new(cues: Vec<TimedSegment>, duration: f64) -> Result<Self, &'static str> {
        if !duration.is_finite() || duration <= 0.0 || duration > 172800.0 || cues.len() > 20000 {
            return Err("Lời thoại vượt giới hạn an toàn.");
        }
        let mut ids = BTreeSet::new();
        let mut previous = 0.0;
        let mut bytes = 0usize;
        for c in &cues {
            bytes = bytes.saturating_add(c.text.len());
            if !c.start.is_finite()
                || !c.end.is_finite()
                || c.start < previous
                || c.end <= c.start
                || c.end > duration
                || !ids.insert(c.id)
                || c.text.chars().count() > 1000
                || bytes > 1_048_576
            {
                return Err("Lời thoại hoặc mốc thời gian không hợp lệ.");
            }
            previous = c.end;
        }
        let mut value = Self {
            cues,
            undo: vec![],
            redo: vec![],
            index: String::new(),
            offsets: vec![],
        };
        value.reindex();
        Ok(value)
    }
    fn reindex(&mut self) {
        self.index.clear();
        self.offsets.clear();
        for cue in &self.cues {
            let start = self.index.len();
            self.index.push_str(&normalized(&cue.text));
            self.offsets.push((start, self.index.len()));
        }
    }
    /// Original ordered source cues, including current text corrections.
    pub fn cues(&self) -> &[TimedSegment] {
        &self.cues
    }
    /// Return cue indices matching a phrase, including phrases spanning consecutive cues.
    pub fn search(&self, query: &str) -> Vec<usize> {
        if query.trim().is_empty() {
            return (0..self.cues.len()).collect();
        }
        if query.chars().count() > 500 {
            return vec![];
        }
        let needle = normalized(query);
        if needle.is_empty() {
            return vec![];
        }
        let mut hits = BTreeSet::new();
        for (start, _) in self.index.match_indices(&needle).take(2000) {
            let end = start + needle.len();
            let first = self.offsets.partition_point(|(_, b)| *b <= start);
            let last = self.offsets.partition_point(|(a, _)| *a < end);
            hits.extend(first..last);
        }
        hits.into_iter().collect()
    }
    /// Correct text only. Returns false for an unchanged value.
    pub fn edit(&mut self, id: u32, text: &str) -> Result<bool, &'static str> {
        let text = text.trim();
        if text.is_empty() || text.chars().count() > 180 || text.chars().any(char::is_control) {
            return Err("Nhập một câu từ 1–180 ký tự, không chứa ký tự điều khiển.");
        }
        let i = self
            .cues
            .iter()
            .position(|c| c.id == id)
            .ok_or("Không tìm thấy câu đã chọn.")?;
        if self.cues[i].text == text {
            return Ok(false);
        }
        if self.cues.iter().map(|c| c.text.len()).sum::<usize>() - self.cues[i].text.len()
            + text.len()
            > 1_048_576
        {
            return Err("Lời thoại vượt giới hạn an toàn.");
        }
        if self.undo.len() == 64 {
            self.undo.remove(0);
        }
        self.undo
            .push((i, std::mem::replace(&mut self.cues[i].text, text.into())));
        self.redo.clear();
        self.reindex();
        Ok(true)
    }
    /// Undo or redo a correction; never affects a timeline.
    pub fn history(&mut self, redo: bool) -> bool {
        let from = if redo { &mut self.redo } else { &mut self.undo };
        let Some((i, text)) = from.pop() else {
            return false;
        };
        let prior = std::mem::replace(&mut self.cues[i].text, text);
        let to = if redo { &mut self.undo } else { &mut self.redo };
        to.push((i, prior));
        self.reindex();
        true
    }
    /// Whether the requested history action is available.
    pub fn can_history(&self, redo: bool) -> bool {
        if redo {
            !self.redo.is_empty()
        } else {
            !self.undo.is_empty()
        }
    }
    /// Create a reviewable manual candidate at actual cue boundaries, not estimated word times.
    pub fn candidate(
        &self,
        first: u32,
        last: u32,
        options: &Options,
        serial: u64,
    ) -> Result<Highlight, &'static str> {
        let start = self
            .cues
            .iter()
            .position(|c| c.id == first)
            .ok_or("Chọn câu bắt đầu.")?;
        let end = self
            .cues
            .iter()
            .position(|c| c.id == last)
            .ok_or("Chọn câu kết thúc.")?;
        if end < start {
            return Err("Câu kết thúc phải nằm sau câu bắt đầu.");
        }
        let part = SourceRange {
            start: self.cues[start].start,
            end: self.cues[end].end,
        };
        let duration = part.end - part.start;
        if !options.min_duration.is_finite()
            || !options.max_duration.is_finite()
            || options.min_duration < 1.0
            || options.max_duration > 180.0
            || duration < options.min_duration
            || duration > options.max_duration
        {
            return Err("Độ dài vùng chọn nằm ngoài giới hạn clip đã đặt. Chọn lại câu đầu/cuối.");
        }
        Ok(Highlight {
            id: format!("manual-{first}-{last}-{serial}"),
            parts: vec![part.clone()],
            title: self.cues[start].text.chars().take(100).collect(),
            hook: self.cues[start].text.clone(),
            reason: "Bạn chọn theo lời thoại nguồn; giữ cả khoảng nghỉ giữa các câu.".into(),
            score: 0.0,
            confidence: 0.0,
            needs_review: true,
            boundary: Boundary::Segment,
            evidence: vec![Evidence {
                kind: "manual-transcript".into(),
                segment_ids: self.cues[start..=end].iter().map(|c| c.id).collect(),
                ranges: vec![part],
            }],
            gate_notes: vec![
                "Mốc cắt theo câu, không phải từng từ. Xem trước và kiểm tra khung hình.".into(),
            ],
            provenance: "manual-transcript".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Review {
        Review::new(
            vec![
                TimedSegment {
                    id: 9,
                    start: 1.0,
                    end: 4.0,
                    text: "Xin chào,".into(),
                },
                TimedSegment {
                    id: 2,
                    start: 5.0,
                    end: 9.0,
                    text: "Việt Nam!".into(),
                },
                TimedSegment {
                    id: 7,
                    start: 10.0,
                    end: 12.0,
                    text: "Kết thúc.".into(),
                },
            ],
            15.0,
        )
        .unwrap()
    }
    #[test]
    fn unicode_phrase_search_crosses_cues_without_claiming_word_times() {
        let r = fixture();
        assert_eq!(r.search("CHÀO việt"), vec![0, 1]);
        assert_eq!(r.search("xin chao"), Vec::<usize>::new());
        assert_eq!(r.search("?!"), Vec::<usize>::new());
        assert_eq!(r.search(""), vec![0, 1, 2]);
        assert_eq!(r.search("cha\u{0300}o vie\u{0323}\u{0302}t"), vec![0, 1]);
    }
    #[test]
    fn correction_undo_redo_preserve_ids_and_times() {
        let mut r = fixture();
        assert!(r.edit(9, "Chào bạn").unwrap());
        assert_eq!(r.search("chào bạn"), vec![0]);
        assert!(r.history(false));
        assert_eq!(r.cues()[0].text, "Xin chào,");
        assert!(r.history(true));
        assert_eq!(r.cues()[0].text, "Chào bạn");
        assert_eq!(
            (r.cues()[0].id, r.cues()[0].start, r.cues()[0].end),
            (9, 1.0, 4.0)
        );
        assert!(!r.edit(9, "Chào bạn").unwrap());
        assert!(r.edit(99, "X").is_err());
        assert!(r.edit(9, "").is_err());
        assert!(r.edit(9, &"a".repeat(181)).is_err());
        assert!(r.history(false));
        r.edit(9, "Mới").unwrap();
        assert!(!r.can_history(true));
    }
    #[test]
    fn manual_range_uses_cue_order_not_numeric_ids_and_enforces_duration() {
        let r = fixture();
        let o = Options {
            min_duration: 4.0,
            max_duration: 10.0,
            ..Default::default()
        };
        let h = r.candidate(9, 2, &o, 1).unwrap();
        assert_eq!(
            h.parts,
            vec![SourceRange {
                start: 1.0,
                end: 9.0
            }]
        );
        assert_eq!(h.evidence[0].segment_ids, vec![9, 2]);
        assert_eq!(h.boundary, Boundary::Segment);
        assert_eq!(h.provenance, "manual-transcript");
        assert!(h.needs_review);
        assert!(r.candidate(2, 9, &o, 2).is_err());
        assert!(r.candidate(7, 7, &o, 2).is_err());
        assert!(r.candidate(9, 7, &o, 2).is_err());
        assert!(r.candidate(9, 99, &o, 2).is_err());
    }
    #[test]
    fn malformed_or_unbounded_source_is_refused() {
        let cue = |id, start, end| TimedSegment {
            id,
            start,
            end,
            text: "x".into(),
        };
        assert!(Review::new(vec![cue(1, 0.0, f64::NAN)], 10.0).is_err());
        assert!(Review::new(vec![cue(1, 0.0, 2.0), cue(1, 3.0, 4.0)], 10.0).is_err());
        assert!(Review::new(vec![cue(1, 0.0, 3.0), cue(2, 2.0, 4.0)], 10.0).is_err());
        assert!(Review::new(vec![cue(1, 0.0, 11.0)], 10.0).is_err());
        assert!(
            Review::new(vec![], 10.0)
                .unwrap()
                .search("hello")
                .is_empty()
        );
    }
    #[test]
    fn srt_copy_uses_source_time_and_preserves_corrected_vietnamese() {
        let mut r = fixture();
        r.edit(2, "Việt Nam tươi đẹp").unwrap();
        let srt = to_srt(r.cues());
        assert!(srt.starts_with("1\n00:00:01,000 --> 00:00:04,000\nXin chào,\n\n"));
        assert!(srt.contains("2\n00:00:05,000 --> 00:00:09,000\nViệt Nam tươi đẹp\n\n"));
    }
    #[test]
    fn history_is_bounded_and_overlong_search_does_not_expand_work() {
        let mut r = fixture();
        for n in 0..100 {
            r.edit(9, &format!("Câu {n}")).unwrap();
        }
        let mut undos = 0;
        while r.history(false) {
            undos += 1;
        }
        assert_eq!(undos, 64);
        assert!(r.search(&"a".repeat(501)).is_empty());
        assert_eq!(r.cues()[0].text, "Câu 35");
    }
    #[test]
    fn nearby_repeated_takes_are_hints_and_never_mutate_source() {
        let cues = vec![
            TimedSegment {
                id: 8,
                start: 0.0,
                end: 3.0,
                text: "Sản phẩm này rất dễ sử dụng!".into(),
            },
            TimedSegment {
                id: 1,
                start: 4.0,
                end: 5.0,
                text: "Nói lại".into(),
            },
            TimedSegment {
                id: 4,
                start: 6.0,
                end: 9.0,
                text: "Sản phẩm này rất dễ sử dụng.".into(),
            },
            TimedSegment {
                id: 2,
                start: 10.0,
                end: 13.0,
                text: "Sản phẩm này rất dễ sử dụng.".into(),
            },
        ];
        let r = Review::new(cues, 20.0).unwrap();
        let hits = r.repetitions();
        assert_eq!(
            hits.iter()
                .map(|h| (h.earlier, h.later))
                .collect::<Vec<_>>(),
            vec![(8, 4), (4, 2)]
        );
        assert_eq!(hits[0].similarity, 1.0);
        assert_eq!(r.cues().len(), 4);
        assert!(!r.can_history(false));
    }
    #[test]
    fn repeated_short_acknowledgements_distant_lines_and_different_text_are_ignored() {
        let r = Review::new(
            vec![
                TimedSegment {
                    id: 0,
                    start: 0.0,
                    end: 1.0,
                    text: "Ừ!".into(),
                },
                TimedSegment {
                    id: 1,
                    start: 1.2,
                    end: 2.0,
                    text: "Ừ!".into(),
                },
                TimedSegment {
                    id: 2,
                    start: 3.0,
                    end: 5.0,
                    text: "Một câu nói rất dài".into(),
                },
                TimedSegment {
                    id: 3,
                    start: 30.0,
                    end: 32.0,
                    text: "Một câu nói rất dài".into(),
                },
                TimedSegment {
                    id: 4,
                    start: 33.0,
                    end: 35.0,
                    text: "zzzzzzzzzzzzzz".into(),
                },
            ],
            40.0,
        )
        .unwrap();
        assert!(r.repetitions().is_empty());
    }
}
