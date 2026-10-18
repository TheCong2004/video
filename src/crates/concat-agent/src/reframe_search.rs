// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer and Concat contributors

//! Subject tracking / auto-reframe & semantic cache/search ported from OpenChatCut
//! (`src/reframe/geometry-focus.ts`, `src/reframe/detect.ts`, `src/agent/semantic-cache.ts`).
//!
//! Provides:
//! - Visual geometry-driven focal point derivation with EMA smoothing for dynamic
//!   aspect-ratio reframing (e.g. 16:9 widescreen to 9:16 vertical TikTok/Shorts).
//! - Translation into timeline keyframes for `EditPlan` Crop & Transform operations.
//! - Revision-bound semantic vector index and LRU cache with cosine similarity
//!   and staleness invalidation.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Bounding box normalized to 0.0..1.0.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct NormalizedRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl NormalizedRect {
    pub fn center(&self) -> (f64, f64) {
        (
            (self.x + self.w / 2.0).clamp(0.0, 1.0),
            (self.y + self.h / 2.0).clamp(0.0, 1.0),
        )
    }
}

/// Visual detection zone in a clip segment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VisualZone {
    pub subject: Option<NormalizedRect>,
    pub face: Option<NormalizedRect>,
}

/// A segment of visual geometry spanning source seconds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeometrySegment {
    pub start_sec: f64,
    pub end_sec: f64,
    pub zone: VisualZone,
}

/// Detected reframe keyframe on a timeline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DetectedKeyframe {
    pub frame: u64,
    pub time_sec: f64,
    pub focal_x: f64,
    pub focal_y: f64,
    pub magnification: f64,
}

/// Reframe computation options matching OpenChatCut.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReframeOptions {
    pub interval_frames: u64,
    pub max_samples: usize,
    pub smooth: f64,
    pub magnification: f64,
    pub target_aspect: (u32, u32), // e.g. (9, 16)
    pub source_aspect: (u32, u32), // e.g. (16, 9)
}

impl Default for ReframeOptions {
    fn default() -> Self {
        Self {
            interval_frames: 15,
            max_samples: 200,
            smooth: 0.65,
            magnification: 1.0,
            target_aspect: (9, 16),
            source_aspect: (16, 9),
        }
    }
}

/// Applies Exponential Moving Average (EMA) smoothing to focal point trajectories.
pub fn smooth_focal_path(points: &[(f64, f64)], smooth: f64) -> Vec<(f64, f64)> {
    if points.is_empty() {
        return Vec::new();
    }
    let s = smooth.clamp(0.0, 0.99);
    let mut out = Vec::with_capacity(points.len());
    let mut cur = points[0];
    out.push(cur);

    for &next in &points[1..] {
        cur = (
            (cur.0 * s + next.0 * (1.0 - s)).clamp(0.0, 1.0),
            (cur.1 * s + next.1 * (1.0 - s)).clamp(0.0, 1.0),
        );
        out.push(cur);
    }
    out
}

/// Finds the focal point (x, y) for a given source timestamp.
pub fn focal_at_source_time(segments: &[GeometrySegment], source_sec: f64) -> (f64, f64) {
    for seg in segments {
        if source_sec >= seg.start_sec - 0.01 && source_sec < seg.end_sec {
            if let Some(subject) = seg.zone.subject {
                return subject.center();
            }
            if let Some(face) = seg.zone.face {
                return face.center();
            }
        }
    }
    if let Some(last) = segments.last() {
        if source_sec >= last.end_sec {
            if let Some(subject) = last.zone.subject {
                return subject.center();
            }
            if let Some(face) = last.zone.face {
                return face.center();
            }
        }
    }
    (0.5, 0.5)
}

/// Derives smoothed focal keyframes across a clip duration.
pub fn compute_reframe_keyframes(
    segments: &[GeometrySegment],
    duration_sec: f64,
    fps: f64,
    options: &ReframeOptions,
) -> Vec<DetectedKeyframe> {
    if duration_sec <= 0.0 || fps <= 0.0 {
        return Vec::new();
    }
    let total_frames = (duration_sec * fps).round() as u64;
    let step = options.interval_frames.max(1);

    let mut sampled_frames = Vec::new();
    let mut f = 0;
    while f < total_frames && sampled_frames.len() < options.max_samples {
        sampled_frames.push(f);
        f += step;
    }
    if sampled_frames.is_empty() {
        return Vec::new();
    }

    let raw_points: Vec<(f64, f64)> = sampled_frames
        .iter()
        .map(|&fr| {
            let t = (fr as f64) / fps;
            focal_at_source_time(segments, t)
        })
        .collect();

    let smoothed = smooth_focal_path(&raw_points, options.smooth);

    sampled_frames
        .into_iter()
        .zip(smoothed)
        .map(|(frame, (fx, fy))| DetectedKeyframe {
            frame,
            time_sec: (frame as f64) / fps,
            focal_x: fx,
            focal_y: fy,
            magnification: options.magnification,
        })
        .collect()
}

// ── Semantic Search & LRU Cache ─────────────────────────────────────────────

/// A document or segment indexed for semantic vector search.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SemanticEntry {
    pub id: String,
    pub asset_path: String,
    pub source_revision: String,
    pub content_hash: String,
    pub text: String,
    pub embedding: Vec<f32>,
    pub timestamp_sec: f64,
}

/// Cosine similarity between two float vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0;
    let mut norm_a = 0.0;
    let mut norm_b = 0.0;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }
    let denom = (norm_a.sqrt() * norm_b.sqrt()).max(1e-8);
    dot / denom
}

/// A search match result with similarity score.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchMatch {
    pub id: String,
    pub asset_path: String,
    pub text: String,
    pub timestamp_sec: f64,
    pub score: f32,
}

/// An in-memory, revision-bound semantic cache and search index.
pub struct SemanticIndex {
    capacity: usize,
    entries: HashMap<String, SemanticEntry>,
    order: Vec<String>,
}

impl SemanticIndex {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: HashMap::new(),
            order: Vec::new(),
        }
    }

    /// Inserts or updates an entry. If at capacity, evicts the oldest entry.
    pub fn insert(&mut self, entry: SemanticEntry) {
        let key = format!("{}:{}", entry.asset_path, entry.id);
        if let Some(pos) = self.order.iter().position(|k| k == &key) {
            self.order.remove(pos);
        } else if self.entries.len() >= self.capacity && !self.order.is_empty() {
            let oldest = self.order.remove(0);
            self.entries.remove(&oldest);
        }
        self.order.push(key.clone());
        self.entries.insert(key, entry);
    }

    /// Invalidates all entries for an asset if its source revision has changed.
    pub fn invalidate_stale(&mut self, asset_path: &str, current_revision: &str) {
        let to_remove: Vec<String> = self
            .entries
            .values()
            .filter(|e| e.asset_path == asset_path && e.source_revision != current_revision)
            .map(|e| format!("{}:{}", e.asset_path, e.id))
            .collect();

        for key in to_remove {
            self.entries.remove(&key);
            if let Some(pos) = self.order.iter().position(|k| k == &key) {
                self.order.remove(pos);
            }
        }
    }

    /// Queries the index using cosine similarity against the query embedding.
    pub fn query(&self, query_embedding: &[f32], top_k: usize, min_score: f32) -> Vec<SearchMatch> {
        if query_embedding.is_empty() {
            return Vec::new();
        }
        let mut matches: Vec<SearchMatch> = self
            .entries
            .values()
            .filter_map(|entry| {
                let score = cosine_similarity(query_embedding, &entry.embedding);
                if score >= min_score {
                    Some(SearchMatch {
                        id: entry.id.clone(),
                        asset_path: entry.asset_path.clone(),
                        text: entry.text.clone(),
                        timestamp_sec: entry.timestamp_sec,
                        score,
                    })
                } else {
                    None
                }
            })
            .collect();

        matches.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        matches.truncate(top_k);
        matches
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_focal_point_smoothing_and_keyframe_generation() {
        let segments = vec![
            GeometrySegment {
                start_sec: 0.0,
                end_sec: 2.0,
                zone: VisualZone {
                    subject: Some(NormalizedRect {
                        x: 0.2,
                        y: 0.3,
                        w: 0.4,
                        h: 0.4,
                    }),
                    face: None,
                },
            },
            GeometrySegment {
                start_sec: 2.0,
                end_sec: 4.0,
                zone: VisualZone {
                    subject: None,
                    face: Some(NormalizedRect {
                        x: 0.6,
                        y: 0.2,
                        w: 0.2,
                        h: 0.2,
                    }),
                },
            },
        ];

        let options = ReframeOptions {
            interval_frames: 15,
            max_samples: 50,
            smooth: 0.5,
            magnification: 1.5,
            target_aspect: (9, 16),
            source_aspect: (16, 9),
        };

        let keyframes = compute_reframe_keyframes(&segments, 4.0, 30.0, &options);
        assert!(!keyframes.is_empty());
        assert_eq!(keyframes[0].magnification, 1.5);
        // First segment center: x = 0.2 + 0.2 = 0.4, y = 0.3 + 0.2 = 0.5
        assert!((keyframes[0].focal_x - 0.4).abs() < 0.05);
        assert!((keyframes[0].focal_y - 0.5).abs() < 0.05);

        // Later keyframe should transition smoothly towards 0.7
        let last = keyframes.last().unwrap();
        assert!(last.focal_x > 0.5);
    }

    #[test]
    fn test_semantic_index_cosine_similarity_and_stale_invalidation() {
        let mut index = SemanticIndex::new(3);

        let e1 = SemanticEntry {
            id: "seg-1".into(),
            asset_path: "video1.mp4".into(),
            source_revision: "rev-1".into(),
            content_hash: "hash1".into(),
            text: "A cat playing with a red ball".into(),
            embedding: vec![1.0, 0.0, 0.0],
            timestamp_sec: 1.5,
        };

        let e2 = SemanticEntry {
            id: "seg-2".into(),
            asset_path: "video1.mp4".into(),
            source_revision: "rev-1".into(),
            content_hash: "hash2".into(),
            text: "A dog running in the green park".into(),
            embedding: vec![0.0, 1.0, 0.0],
            timestamp_sec: 5.0,
        };

        index.insert(e1);
        index.insert(e2);
        assert_eq!(index.len(), 2);

        // Query for cat-like embedding [0.9, 0.1, 0.0]
        let matches = index.query(&[0.9, 0.1, 0.0], 5, 0.7);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id, "seg-1");
        assert!(matches[0].score > 0.85);

        // Invalidate stale revisions
        index.invalidate_stale("video1.mp4", "rev-2");
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn test_semantic_index_lru_capacity_eviction() {
        let mut index = SemanticIndex::new(2);
        for i in 1..=3 {
            index.insert(SemanticEntry {
                id: format!("seg-{i}"),
                asset_path: "clip.mp4".into(),
                source_revision: "rev-1".into(),
                content_hash: format!("hash-{i}"),
                text: format!("Text {i}"),
                embedding: vec![i as f32, 0.0, 0.0],
                timestamp_sec: i as f64,
            });
        }
        // Capacity was 2, so seg-1 should have been evicted
        assert_eq!(index.len(), 2);
        let matches = index.query(&[1.0, 0.0, 0.0], 5, 0.99);
        assert!(matches.iter().all(|m| m.id != "seg-1"));
    }
}
