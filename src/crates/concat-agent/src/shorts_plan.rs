// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat contributors
//! Reviewable shorts assembly through the existing native timeline/command engine.
//! Center-fill is an explicit fallback, never claimed as face/subject tracking.

use crate::highlight::{Highlight, TimedSegment};
use concat_project::commands::{NewMedia, TrimEdge};
use concat_project::model::{MediaKind, TextStyle, VideoSettings};
use concat_project::{Command, DraftError, Editor, PreparedDraft};
use serde::{Deserialize, Serialize};

/// Native subtitle appearances, with safe vertical-video text bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaptionPreset {
    None,
    Clean,
    Bold,
    Podcast,
    Cinematic,
    Gaming,
}
/// Safe refusal; never echoes paths, source text or provider output.
#[derive(Debug, thiserror::Error)]
pub enum ShortsPlanError {
    #[error("Shorts source, selection or caption bounds are invalid")]
    Input,
    #[error("Shorts exceeds native batch or subtitle bounds")]
    Limit,
    #[error("Existing source probe metadata changed; re-import source before assembly")]
    SourceChanged,
    #[error("Native shorts assembly did not preserve requested timing or geometry")]
    NativeMismatch,
    #[error(transparent)]
    Draft(#[from] DraftError),
}

fn step(
    scratch: &mut Editor,
    commands: &mut Vec<Command>,
    command: Command,
) -> Result<Option<String>, ShortsPlanError> {
    if commands.len() >= 256 {
        return Err(ShortsPlanError::Limit);
    }
    let result = scratch.apply(command.clone()).map_err(DraftError::from)?;
    if !result.applied {
        return Err(ShortsPlanError::NativeMismatch);
    }
    commands.push(command);
    Ok(result.created_id)
}
fn style(text: String, preset: CaptionPreset) -> TextStyle {
    let mut s = TextStyle {
        content: text,
        font_family: "Hanken Grotesk".into(),
        font_size: 0.032,
        font_weight: 600.0,
        max_width: 0.8,
        max_height: 0.14,
        stroke_width: 0.0015,
        shadow: true,
        ..TextStyle::default()
    };
    match preset {
        CaptionPreset::Bold => {
            s.font_weight = 900.0;
            s.font_size = 0.040;
            s.stroke_width = 0.003;
        }
        CaptionPreset::Podcast => {
            s.font_weight = 700.0;
            s.background = "#101018dd".into();
            s.background_padding_x = 0.012;
            s.background_padding_y = 0.007;
        }
        CaptionPreset::Cinematic => {
            s.font_weight = 500.0;
            s.font_size = 0.028;
            s.tracking = 0.001;
            s.background = "#00000080".into();
        }
        CaptionPreset::Gaming => {
            s.font_weight = 900.0;
            s.color = "#cbf53f".into();
            s.font_size = 0.038;
            s.stroke_width = 0.004;
        }
        CaptionPreset::None | CaptionPreset::Clean => {}
    }
    s
}

/// Assemble a NEW native timeline on scratch state, returning one revision-bound undoable draft.
/// No Session mutation, source decoding or guessed source timing. Captions retain real segment
/// text and clip the segment's display interval at each occupied source-piece boundary.
/// Static center-crop is the declared fallback; this function does not claim subject tracking.
pub fn propose(
    editor: &Editor,
    media: NewMedia,
    highlight: &Highlight,
    video: VideoSettings,
    captions: &[TimedSegment],
    preset: CaptionPreset,
) -> Result<PreparedDraft, ShortsPlanError> {
    let source_duration = media
        .duration
        .filter(|d| d.is_finite() && *d > 0.0 && *d <= 172800.0)
        .ok_or(ShortsPlanError::Input)?;
    let (width, height) = match (media.width, media.height) {
        (Some(w), Some(h)) if w > 0 && h > 0 => (w, h),
        _ => return Err(ShortsPlanError::Input),
    };
    if media.kind != MediaKind::Video
        || media.path.trim().is_empty()
        || media.path.len() > 32768
        || !video.is_sane()
        || highlight.parts.is_empty()
        || highlight.parts.len() > 8
        || captions.len() > 20000
        || highlight.title.len() > 512
    {
        return Err(ShortsPlanError::Input);
    }
    let mut previous = 0.0;
    for p in &highlight.parts {
        if !p.start.is_finite()
            || !p.end.is_finite()
            || p.start < previous
            || p.end > source_duration
            || p.end - p.start < 1.0 / 60.0
        {
            return Err(ShortsPlanError::Input);
        }
        previous = p.end;
    }
    let mut caption_ids = std::collections::HashSet::new();
    let mut total_text = 0usize;
    for c in captions {
        total_text = total_text.saturating_add(c.text.len());
        if !c.start.is_finite()
            || !c.end.is_finite()
            || c.start < 0.0
            || c.end <= c.start
            || c.end > source_duration
            || !caption_ids.insert(c.id)
        {
            return Err(ShortsPlanError::Input);
        }
        if c.text.chars().count() > 180 || total_text > 1_048_576 {
            return Err(ShortsPlanError::Limit);
        }
    }
    // Native geometry uses contain fit; this ratio changes contain to centered cover.
    let fit_x = video.width as f64 / width as f64;
    let fit_y = video.height as f64 / height as f64;
    let scale = fit_x.max(fit_y) / fit_x.min(fit_y);
    if !scale.is_finite() || scale > 8.0 {
        return Err(ShortsPlanError::Input);
    }
    let mut scratch = editor.fork_for_draft();
    let mut commands = Vec::new();
    let media_id =
        if let Some(existing) = editor.project().media.iter().find(|m| m.path == media.path) {
            if existing.duration != media.duration
                || existing.kind != media.kind
                || existing.width != media.width
                || existing.height != media.height
                || existing.color_space != media.color_space
                || existing.has_audio != media.has_audio
                || existing.audio_tracks != media.audio_tracks
                || existing.frame_rate_fraction != media.frame_rate_fraction
            {
                return Err(ShortsPlanError::SourceChanged);
            }
            existing.id.clone()
        } else {
            step(
                &mut scratch,
                &mut commands,
                Command::AddMedia { item: media },
            )?
            .ok_or(ShortsPlanError::NativeMismatch)?
        };
    let timeline_id = step(&mut scratch, &mut commands, Command::AddTimeline)?
        .ok_or(ShortsPlanError::NativeMismatch)?;
    if scratch.project().active().video != video {
        step(
            &mut scratch,
            &mut commands,
            Command::SetTimelineVideo {
                timeline_id: timeline_id.clone(),
                video,
            },
        )?;
    }
    let name = if highlight.title.trim().is_empty() {
        "Shorts candidate".into()
    } else {
        highlight.title.trim().to_owned()
    };
    if scratch.project().active().name != name {
        step(
            &mut scratch,
            &mut commands,
            Command::RenameTimeline { timeline_id, name },
        )?;
    }
    let video_track = scratch.project().active().tracks[0].id.clone();
    let caption_track = scratch.project().active().tracks[1].id.clone();
    let mut position = 0.0;
    for p in &highlight.parts {
        let id = step(
            &mut scratch,
            &mut commands,
            Command::AddClip {
                media_id: media_id.clone(),
                track_id: video_track.clone(),
                start: position,
                ripple: false,
            },
        )?
        .ok_or(ShortsPlanError::NativeMismatch)?;
        if p.start > 0.0 {
            step(
                &mut scratch,
                &mut commands,
                Command::TrimClip {
                    clip_id: id.clone(),
                    edge: TrimEdge::Start,
                    delta: p.start,
                    ripple: true,
                },
            )?;
        }
        if p.end < source_duration {
            step(
                &mut scratch,
                &mut commands,
                Command::TrimClip {
                    clip_id: id.clone(),
                    edge: TrimEdge::End,
                    delta: p.end - source_duration,
                    ripple: false,
                },
            )?;
        }
        if (scale - 1.0).abs() > 1e-12 {
            step(
                &mut scratch,
                &mut commands,
                Command::SetClipTransform {
                    clip_id: id.clone(),
                    scale: Some(scale),
                    offset_x: None,
                    offset_y: None,
                    rotation: None,
                    stretch_x: None,
                    stretch_y: None,
                },
            )?;
        }
        step(
            &mut scratch,
            &mut commands,
            Command::UpdateClip {
                clip_id: id.clone(),
                patch: concat_project::commands::ClipPatch {
                    name: Some("Shorts source [center-crop fallback; no subject tracking]".into()),
                    ..Default::default()
                },
            },
        )?;
        let clip = scratch
            .project()
            .active()
            .clips
            .iter()
            .find(|c| c.id == id)
            .ok_or(ShortsPlanError::NativeMismatch)?;
        if (clip.source_start - p.start).abs() > 1e-8
            || (clip.start - position).abs() > 1e-8
            || (clip.duration - (p.end - p.start)).abs() > 1e-8
            || (clip.speed - 1.0).abs() > 1e-12
            || (clip.scale - scale).abs() > 1e-8
        {
            return Err(ShortsPlanError::NativeMismatch);
        }
        if preset != CaptionPreset::None {
            let mut cues: Vec<_> = captions
                .iter()
                .filter_map(|c| {
                    let a = c.start.max(p.start);
                    let b = c.end.min(p.end);
                    (b - a >= 1.0 / 60.0 && !c.text.trim().is_empty()).then_some((a, b, c))
                })
                .collect();
            cues.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut previous_end = p.start;
            for (a, b, c) in cues {
                // Overlapping supplied cues cannot be silently shifted by native collision handling.
                if a < previous_end - 1e-9 {
                    return Err(ShortsPlanError::Input);
                }
                previous_end = b;
                let start = position + a - p.start;
                let duration = b - a;
                let cid = step(
                    &mut scratch,
                    &mut commands,
                    Command::AddTextClip {
                        track_id: Some(caption_track.clone()),
                        above: true,
                        start,
                        style: Some(style(c.text.clone(), preset)),
                        duration: Some(duration),
                        offset_y: Some(0.25),
                    },
                )?
                .ok_or(ShortsPlanError::NativeMismatch)?;
                let cue = scratch
                    .project()
                    .active()
                    .clips
                    .iter()
                    .find(|x| x.id == cid)
                    .ok_or(ShortsPlanError::NativeMismatch)?;
                if (cue.start - start).abs() > 1e-8 || (cue.duration - duration).abs() > 1e-8 {
                    return Err(ShortsPlanError::NativeMismatch);
                }
            }
        }
        position += p.end - p.start;
    }
    // Native standalone AddClip may follow first HDR to HLG; the requested output remains
    // authoritative, and an explicit restoration also keeps standalone/batch previews equal.
    if scratch.project().active().video != video {
        let timeline_id = scratch.project().active_timeline_id.clone();
        step(
            &mut scratch,
            &mut commands,
            Command::SetTimelineVideo { timeline_id, video },
        )?;
    }
    let draft = editor.prepare_batch(commands)?;
    if draft.project() != scratch.project() {
        return Err(ShortsPlanError::NativeMismatch);
    }
    Ok(draft)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::highlight::{Boundary, Evidence, SourceRange};
    fn media() -> NewMedia {
        NewMedia {
            path: "D:/fixture/source.mp4".into(),
            name: "source".into(),
            duration: Some(100.0),
            kind: MediaKind::Video,
            width: Some(1920),
            height: Some(1080),
            frame_rate: Some(30.0),
            frame_rate_fraction: Some("30/1".into()),
            video_codec: Some("h264".into()),
            audio_codec: None,
            has_audio: false,
            audio_tracks: vec![],
            origin: None,
            color_space: Default::default(),
        }
    }
    fn video() -> VideoSettings {
        VideoSettings {
            width: 1080,
            height: 1920,
            ..VideoSettings::default()
        }
    }
    fn highlight() -> Highlight {
        Highlight {
            id: "h1".into(),
            parts: vec![
                SourceRange {
                    start: 10.0,
                    end: 14.0,
                },
                SourceRange {
                    start: 80.0,
                    end: 83.0,
                },
            ],
            title: "Bản cắt Việt Nam".into(),
            hook: String::new(),
            reason: String::new(),
            score: 60.0,
            confidence: 0.5,
            needs_review: true,
            boundary: Boundary::Signal,
            evidence: vec![Evidence {
                kind: "motion".into(),
                segment_ids: vec![],
                ranges: vec![],
            }],
            gate_notes: vec![],
            provenance: "fixture".into(),
        }
    }
    #[test]
    fn native_stitched_draft_preserves_original_one_undo_redo_and_metadata() {
        let mut editor = Editor::new();
        let before = editor.project().clone();
        let draft = propose(
            &editor,
            media(),
            &highlight(),
            video(),
            &[],
            CaptionPreset::None,
        )
        .unwrap();
        assert_eq!(editor.project(), &before);
        assert_eq!(draft.project().timelines.len(), 2);
        let clips = &draft.project().active().clips;
        assert_eq!(clips.len(), 2);
        assert_eq!(
            (
                clips[0].source_start,
                clips[0].start,
                clips[0].duration,
                clips[0].speed
            ),
            (10.0, 0.0, 4.0, 1.0)
        );
        assert_eq!(
            (clips[1].source_start, clips[1].start, clips[1].duration),
            (80.0, 4.0, 3.0)
        );
        assert!((clips[0].scale - ((1920.0 / 1080.0) / (1080.0 / 1920.0))).abs() < 1e-9);
        assert!(clips[0].name.contains("center-crop fallback"));
        assert_eq!(draft.project().timelines[0], before.timelines[0]);
        let expected = draft.project().clone();
        editor.commit_draft(draft).unwrap();
        assert_eq!(editor.project(), &expected);
        editor.undo();
        assert_eq!(editor.project(), &before);
        editor.redo();
        assert_eq!(editor.project(), &expected);
    }
    #[test]
    fn captions_clip_and_remap_to_each_occupied_piece_unicode_safe_zones() {
        let editor = Editor::new();
        let captions = vec![
            TimedSegment {
                id: 1,
                start: 9.0,
                end: 12.0,
                text: "Xin chào Việt Nam".into(),
            },
            TimedSegment {
                id: 2,
                start: 81.0,
                end: 85.0,
                text: "Cảm ơn bạn".into(),
            },
            TimedSegment {
                id: 3,
                start: 40.0,
                end: 50.0,
                text: "Never included".into(),
            },
        ];
        for preset in [
            CaptionPreset::Clean,
            CaptionPreset::Bold,
            CaptionPreset::Podcast,
            CaptionPreset::Cinematic,
            CaptionPreset::Gaming,
        ] {
            let draft =
                propose(&editor, media(), &highlight(), video(), &captions, preset).unwrap();
            let captions: Vec<_> = draft
                .project()
                .active()
                .clips
                .iter()
                .filter(|c| c.text.is_some())
                .collect();
            assert_eq!(captions.len(), 2);
            assert_eq!((captions[0].start, captions[0].duration), (0.0, 2.0));
            assert_eq!((captions[1].start, captions[1].duration), (5.0, 2.0));
            assert_eq!(
                captions[0].text.as_ref().unwrap().content,
                "Xin chào Việt Nam"
            );
            for c in captions {
                let style = c.text.as_ref().unwrap();
                assert!(style.max_width <= 0.8);
                assert!(0.5 + c.offset_y + style.max_height / 2.0 <= 0.85);
                assert!(0.5 + c.offset_y - style.max_height / 2.0 >= 0.12);
            }
        }
    }
    #[test]
    fn duplicate_media_reused_and_changed_probe_refused() {
        let mut editor = Editor::new();
        editor.apply(Command::AddMedia { item: media() }).unwrap();
        let draft = propose(
            &editor,
            media(),
            &highlight(),
            video(),
            &[],
            CaptionPreset::None,
        )
        .unwrap();
        assert_eq!(draft.project().media.len(), 1);
        let mut changed = media();
        changed.duration = Some(90.0);
        assert!(matches!(
            propose(
                &editor,
                changed,
                &highlight(),
                video(),
                &[],
                CaptionPreset::None
            ),
            Err(ShortsPlanError::SourceChanged)
        ));
    }
    #[test]
    fn stale_and_invalid_evidence_never_change_live_editor() {
        let mut editor = Editor::new();
        let draft = propose(
            &editor,
            media(),
            &highlight(),
            video(),
            &[],
            CaptionPreset::None,
        )
        .unwrap();
        editor.apply(Command::AddTimeline).unwrap();
        editor.undo();
        let before = editor.project().clone();
        assert!(editor.commit_draft(draft).is_err());
        assert_eq!(editor.project(), &before);
        for parts in [
            vec![(10.0, 101.0)],
            vec![(f64::NAN, 13.0)],
            vec![(20.0, 22.0), (10.0, 12.0)],
            vec![(0.0, 0.001)],
        ] {
            let mut h = highlight();
            h.parts = parts
                .into_iter()
                .map(|(start, end)| SourceRange { start, end })
                .collect();
            assert!(propose(&editor, media(), &h, video(), &[], CaptionPreset::None).is_err());
            assert_eq!(editor.project(), &before);
        }
    }
    #[test]
    fn hdr_source_still_obeys_requested_sdr_and_batch_parity() {
        let editor = Editor::new();
        let mut source = media();
        source.color_space = concat_project::model::ColorSpace::Hlg;
        let draft = propose(
            &editor,
            source,
            &highlight(),
            video(),
            &[],
            CaptionPreset::None,
        )
        .unwrap();
        assert_eq!(draft.project().active().video, video());
        assert_eq!(draft.project().timelines[0], editor.project().timelines[0]);
    }
    #[test]
    fn invalid_aspect_and_overlapping_cues_are_refused_instead_of_clamped() {
        let editor = Editor::new();
        let mut wide = media();
        wide.width = Some(20000);
        wide.height = Some(100);
        assert!(
            propose(
                &editor,
                wide,
                &highlight(),
                video(),
                &[],
                CaptionPreset::None
            )
            .is_err()
        );
        let cues = vec![
            TimedSegment {
                id: 1,
                start: 10.0,
                end: 13.0,
                text: "A".into(),
            },
            TimedSegment {
                id: 2,
                start: 12.0,
                end: 14.0,
                text: "B".into(),
            },
        ];
        assert!(
            propose(
                &editor,
                media(),
                &highlight(),
                video(),
                &cues,
                CaptionPreset::Clean
            )
            .is_err()
        );
    }
}
