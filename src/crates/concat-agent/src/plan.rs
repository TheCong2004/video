// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat contributors
//! Bounded native editing vocabulary. Unsupported source-editor features
//! fail decoding rather than silently becoming successful native edits.
use concat_project::commands::{ClipMove, ClipPatch, TrimEdge};
use concat_project::model::{
    Clip, ClipKind, Crop, Cutout, CutoutMode, KeyEase, KeyProperty, Subject, TextStyle, Transition,
};
use concat_project::{Command, DraftError, Editor, PreparedDraft};
use serde::Deserialize;
use std::collections::HashMap;

const MAX_INPUT: usize = 64 * 1024;
const MAX_TIME: f64 = 100.0 * 3600.0;
const MIN_DURATION: f64 = 1.0 / 60.0;

/// Refused untrusted plan. No partial edit reaches the live project.
#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    /// Input has an unsupported schema, value or reference.
    #[error("invalid edit plan: {0}")]
    Invalid(String),
    /// Plan references an old revision or a different active timeline.
    #[error("edit plan is stale; regenerate against the current timeline")]
    Stale,
    /// Native draft refusal.
    #[error(transparent)]
    Draft(#[from] DraftError),
}

#[derive(Deserialize)]
enum Units {
    #[serde(rename = "seconds")]
    Seconds,
}

/// Versioned AI response contract; callers supply the live editor to compile.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditPlan {
    version: u32,
    units: Units,
    timeline_id: String,
    base_revision: u64,
    operations: Vec<Operation>,
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", deny_unknown_fields)]
enum Operation {
    Place {
        #[serde(rename = "mediaId")]
        media_id: String,
        #[serde(rename = "trackId")]
        track_id: String,
        start: f64,
        ripple: bool,
        #[serde(rename = "as")]
        alias: String,
    },
    Speed {
        #[serde(rename = "clipId")]
        clip_id: String,
        speed: f64,
    },
    Audio {
        #[serde(rename = "clipId")]
        clip_id: String,
        volume: Option<f64>,
        muted: Option<bool>,
        #[serde(rename = "fadeIn")]
        fade_in: Option<f64>,
        #[serde(rename = "fadeOut")]
        fade_out: Option<f64>,
    },
    Transform {
        #[serde(rename = "clipId")]
        clip_id: String,
        scale: Option<f64>,
        #[serde(rename = "offsetX")]
        offset_x: Option<f64>,
        #[serde(rename = "offsetY")]
        offset_y: Option<f64>,
        rotation: Option<f64>,
    },
    Keyframe {
        #[serde(rename = "clipId")]
        clip_id: String,
        property: KeyProperty,
        time: f64,
        value: f64,
    },
    Text {
        #[serde(rename = "clipId")]
        clip_id: String,
        content: String,
        #[serde(rename = "fontSize")]
        font_size: Option<f64>,
    },
    AddTrack {
        bottom: bool,
        #[serde(rename = "as")]
        alias: String,
    },
    RemoveTrack {
        #[serde(rename = "trackId")]
        track_id: String,
        #[serde(rename = "deleteClips")]
        delete_clips: bool,
    },
    RemoveMany {
        #[serde(rename = "clipIds")]
        clip_ids: Vec<String>,
        ripple: bool,
    },
    RemoveRanges {
        #[serde(rename = "clipId")]
        clip_id: String,
        ranges: Vec<TimeRange>,
        ripple: bool,
    },
    Split {
        #[serde(rename = "clipId")]
        clip_id: String,
        time: f64,
        #[serde(rename = "as")]
        alias: String,
    },
    Remove {
        #[serde(rename = "clipId")]
        clip_id: String,
        ripple: bool,
    },
    Move {
        #[serde(rename = "clipId")]
        clip_id: String,
        start: f64,
        #[serde(rename = "trackId")]
        track_id: String,
    },
    Trim {
        #[serde(rename = "clipId")]
        clip_id: String,
        edge: TrimEdge,
        delta: f64,
        ripple: bool,
    },
    Title {
        text: String,
        start: f64,
        duration: f64,
        #[serde(rename = "as")]
        alias: String,
    },
    Transition {
        #[serde(rename = "clipId")]
        clip_id: String,
        #[serde(rename = "transitionId")]
        transition_id: String,
        duration: Option<f64>,
    },
    Crop {
        #[serde(rename = "clipId")]
        clip_id: String,
        left: Option<f64>,
        top: Option<f64>,
        right: Option<f64>,
        bottom: Option<f64>,
    },
    Appearance {
        #[serde(rename = "clipId")]
        clip_id: String,
        opacity: Option<f64>,
        blend: Option<String>,
        #[serde(rename = "flipH")]
        flip_h: Option<bool>,
        #[serde(rename = "flipV")]
        flip_v: Option<bool>,
    },
    Layer {
        #[serde(rename = "trackId")]
        track_id: Option<String>,
        start: f64,
        duration: f64,
        #[serde(rename = "effectId")]
        effect_id: String,
        name: String,
        #[serde(rename = "as")]
        alias: String,
    },
    Shape {
        #[serde(rename = "trackId")]
        track_id: Option<String>,
        start: f64,
        duration: f64,
        name: String,
        #[serde(rename = "as")]
        alias: String,
    },
    Cutout {
        #[serde(rename = "clipId")]
        clip_id: String,
        enabled: bool,
    },
}

#[derive(Deserialize, Clone, Copy)]
#[serde(deny_unknown_fields)]
struct TimeRange {
    start: f64,
    end: f64,
}

/// An actual native draft plus bounded descriptions for the approval surface.
pub struct CompiledPlan {
    draft: PreparedDraft,
    summaries: Vec<String>,
}
impl CompiledPlan {
    /// Isolated project for preview.
    pub fn draft(&self) -> &PreparedDraft {
        &self.draft
    }
    /// Human-visible description of each operation, in execution order.
    pub fn summaries(&self) -> &[String] {
        &self.summaries
    }
    /// Approval consumes the proposal; caller still enforces user permission.
    pub fn into_draft(self) -> PreparedDraft {
        self.draft
    }
}

fn invalid(message: impl Into<String>) -> PlanError {
    PlanError::Invalid(message.into())
}
fn time(value: f64) -> Result<(), PlanError> {
    if !value.is_finite() || !(0.0..=MAX_TIME).contains(&value) {
        return Err(invalid("time out of range"));
    }
    Ok(())
}
fn resolve(reference: &str, aliases: &HashMap<String, String>) -> Result<String, PlanError> {
    if reference.is_empty() || reference.len() > 128 {
        return Err(invalid("invalid clip reference"));
    }
    match reference.strip_prefix('@') {
        Some(alias) => aliases
            .get(alias)
            .cloned()
            .ok_or_else(|| invalid("unknown created alias")),
        None => Ok(reference.to_owned()),
    }
}
fn clip<'a>(editor: &'a Editor, id: &str) -> Result<&'a Clip, PlanError> {
    editor
        .project()
        .active()
        .clips
        .iter()
        .find(|clip| clip.id == id)
        .map(|clip| clip.as_ref())
        .ok_or_else(|| invalid("clip not in active timeline"))
}
fn check_alias(alias: &str, aliases: &HashMap<String, String>) -> Result<(), PlanError> {
    if alias.is_empty()
        || alias.len() > 64
        || !alias
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_')
        || aliases.contains_key(alias)
    {
        return Err(invalid("invalid or duplicate alias"));
    }
    Ok(())
}

fn bounded(value: f64, min: f64, max: f64) -> Result<(), PlanError> {
    if !value.is_finite() || !(min..=max).contains(&value) {
        return Err(invalid("property outside native bounds"));
    }
    Ok(())
}
fn key_value(property: KeyProperty, value: f64) -> Result<(), PlanError> {
    use concat_project::model::ranges::*;
    match property {
        KeyProperty::Scale => bounded(value, MIN_SCALE, MAX_SCALE),
        KeyProperty::OffsetX | KeyProperty::OffsetY => bounded(value, -MAX_OFFSET, MAX_OFFSET),
        KeyProperty::Rotation => bounded(value, -MAX_TIME, MAX_TIME),
        KeyProperty::Opacity => bounded(value, 0.0, 1.0),
        KeyProperty::Volume => bounded(value, 0.0, 16.0),
    }
}
fn track(editor: &Editor, id: &str) -> Result<(), PlanError> {
    if !editor.project().active().tracks.iter().any(|t| t.id == id) {
        return Err(invalid("track not in active timeline"));
    }
    Ok(())
}
fn execute(
    scratch: &mut Editor,
    commands: &mut Vec<Command>,
    command: Command,
) -> Result<Option<String>, PlanError> {
    if commands.len() >= 256 {
        return Err(invalid("expanded plan exceeds 256 native operations"));
    }
    let result = scratch.apply(command.clone()).map_err(DraftError::from)?;
    if !result.applied {
        return Err(invalid("native operation was refused or made no change"));
    }
    commands.push(command);
    Ok(result.created_id)
}

/// Descending source-preserving cuts keep the original head ID available for
/// earlier ranges. Native split IDs are resolved on the exact reserved mint.
fn remove_ranges(
    scratch: &mut Editor,
    commands: &mut Vec<Command>,
    id: &str,
    ranges: &[TimeRange],
    ripple: bool,
) -> Result<(), PlanError> {
    if ranges.is_empty() || ranges.len() > 32 {
        return Err(invalid("expected 1..=32 ranges"));
    }
    let item = clip(scratch, id)?;
    let (start, end) = (item.start, item.start + item.duration);
    let mut sorted = ranges.to_vec();
    for range in &sorted {
        time(range.start)?;
        time(range.end)?;
        if range.start < start || range.end > end || range.end - range.start < MIN_DURATION {
            return Err(invalid("range outside clip or shorter than native minimum"));
        }
    }
    sorted.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut union: Vec<TimeRange> = Vec::new();
    for range in sorted {
        if let Some(last) = union.last_mut()
            && range.start <= last.end
        {
            last.end = last.end.max(range.end);
        } else {
            union.push(range);
        }
    }
    for range in union.iter().rev() {
        let item = clip(scratch, id)?;
        let (head, end) = (item.start, item.start + item.duration);
        if range.end < end - 1e-9 {
            if end - range.end <= MIN_DURATION {
                return Err(invalid("range leaves an unrenderable tail"));
            }
            execute(
                scratch,
                commands,
                Command::SplitClips {
                    clip_ids: vec![id.into()],
                    time: range.end,
                },
            )?;
        }
        let removed = if range.start > head + 1e-9 {
            if range.start - head <= MIN_DURATION {
                return Err(invalid("range leaves an unrenderable head"));
            }
            execute(
                scratch,
                commands,
                Command::SplitClips {
                    clip_ids: vec![id.into()],
                    time: range.start,
                },
            )?
            .ok_or_else(|| invalid("split failed to identify removed span"))?
        } else {
            id.to_owned()
        };
        execute(
            scratch,
            commands,
            Command::RemoveClips {
                clip_ids: vec![removed],
                ripple,
            },
        )?;
    }
    Ok(())
}

impl EditPlan {
    /// Decode with byte, schema, version and operation-count limits.
    pub fn parse(input: &str) -> Result<Self, PlanError> {
        if input.len() > MAX_INPUT {
            return Err(invalid("response exceeds 64 KiB"));
        }
        let plan: Self =
            serde_json::from_str(input).map_err(|_| invalid("unsupported JSON schema"))?;
        if plan.version != 1
            || plan.operations.is_empty()
            || plan.operations.len() > 256
            || plan.timeline_id.is_empty()
            || plan.timeline_id.len() > 128
        {
            return Err(invalid("unsupported version or operation count"));
        }
        Ok(plan)
    }

    /// Resolve sequential aliases against scratch native state, then prepare
    /// exactly one native batch. All times are timeline seconds, never ms.
    pub fn compile(self, editor: &Editor) -> Result<CompiledPlan, PlanError> {
        let Units::Seconds = self.units;
        if self.base_revision != editor.revision()
            || self.timeline_id != editor.project().active_timeline_id
        {
            return Err(PlanError::Stale);
        }
        let mut scratch = editor.fork_for_draft();
        let mut aliases = HashMap::new();
        let mut commands = Vec::new();
        let mut summaries = Vec::new();
        for operation in self.operations {
            if let Operation::RemoveRanges {
                clip_id,
                ranges,
                ripple,
            } = &operation
            {
                let id = resolve(clip_id, &aliases)?;
                remove_ranges(&mut scratch, &mut commands, &id, ranges, *ripple)?;
                summaries.push(format!(
                    "Remove {} ranges from {clip_id}; ripple={ripple}",
                    ranges.len()
                ));
                continue;
            }
            let mut expected_trim: Option<(String, f64)> = None;
            let (command, alias, summary) = match operation {
                Operation::Place {
                    media_id,
                    track_id,
                    start,
                    ripple,
                    alias,
                } => {
                    time(start)?;
                    check_alias(&alias, &aliases)?;
                    let track_id = resolve(&track_id, &aliases)?;
                    track(&scratch, &track_id)?;
                    let media = scratch
                        .project()
                        .media
                        .iter()
                        .find(|m| m.id == media_id)
                        .ok_or_else(|| invalid("media not in this project's bin"))?;
                    time(start + media.duration.unwrap_or(5.0))?;
                    (
                        Command::AddClip {
                            media_id,
                            track_id,
                            start,
                            ripple,
                        },
                        Some(alias),
                        format!("Place media at {start:.3}s"),
                    )
                }
                Operation::Speed { clip_id, speed } => {
                    bounded(speed, 0.0625, 16.0)?;
                    let id = resolve(&clip_id, &aliases)?;
                    let item = clip(&scratch, &id)?;
                    if !matches!(item.kind, ClipKind::Video | ClipKind::Audio) {
                        return Err(invalid("speed requires video/audio"));
                    }
                    bounded(
                        item.duration * item.speed / speed,
                        MIN_DURATION,
                        MAX_TIME - item.start,
                    )?;
                    (
                        Command::SetClipSpeed { clip_id: id, speed },
                        None,
                        format!("Retime {clip_id} to {speed}x"),
                    )
                }
                Operation::Audio {
                    clip_id,
                    volume,
                    muted,
                    fade_in,
                    fade_out,
                } => {
                    let id = resolve(&clip_id, &aliases)?;
                    let item = clip(&scratch, &id)?;
                    if !matches!(item.kind, ClipKind::Video | ClipKind::Audio) {
                        return Err(invalid("audio requires video/audio"));
                    }
                    if let Some(v) = volume {
                        bounded(v, 0.0, 16.0)?;
                    }
                    for v in [fade_in, fade_out].into_iter().flatten() {
                        bounded(v, 0.0, item.duration)?;
                    }
                    (
                        Command::UpdateClip {
                            clip_id: id,
                            patch: ClipPatch {
                                volume,
                                muted,
                                fade_in,
                                fade_out,
                                ..Default::default()
                            },
                        },
                        None,
                        format!("Update audio {clip_id}"),
                    )
                }
                Operation::Transform {
                    clip_id,
                    scale,
                    offset_x,
                    offset_y,
                    rotation,
                } => {
                    let id = resolve(&clip_id, &aliases)?;
                    let item = clip(&scratch, &id)?;
                    if matches!(item.kind, ClipKind::Audio) {
                        return Err(invalid("audio has no visual transform"));
                    }
                    if let Some(v) = scale {
                        key_value(KeyProperty::Scale, v)?;
                    }
                    for v in [offset_x, offset_y].into_iter().flatten() {
                        bounded(v, -3.0, 3.0)?;
                    }
                    if let Some(v) = rotation {
                        bounded(v, -180.0, 180.0)?;
                    }
                    (
                        Command::SetClipTransform {
                            clip_id: id,
                            scale,
                            offset_x,
                            offset_y,
                            rotation,
                            stretch_x: None,
                            stretch_y: None,
                        },
                        None,
                        format!("Transform {clip_id}"),
                    )
                }
                Operation::Keyframe {
                    clip_id,
                    property,
                    time: at,
                    value,
                } => {
                    let id = resolve(&clip_id, &aliases)?;
                    let item = clip(&scratch, &id)?;
                    bounded(at, item.start, item.start + item.duration)?;
                    key_value(property, value)?;
                    let position = (at - item.start) / item.duration;
                    (
                        Command::SetClipKey {
                            clip_id: id,
                            property,
                            at: position,
                            value,
                            ease: KeyEase::LINEAR,
                        },
                        None,
                        format!("Key {} on {clip_id} at {at:.3}s", property.name()),
                    )
                }
                Operation::Text {
                    clip_id,
                    content,
                    font_size,
                } => {
                    let id = resolve(&clip_id, &aliases)?;
                    let item = clip(&scratch, &id)?;
                    if item.kind != ClipKind::Text
                        || content.trim().is_empty()
                        || content.len() > 4096
                    {
                        return Err(invalid("invalid text clip/content"));
                    }
                    let mut style = item.text.clone().unwrap_or_default();
                    style.content = content;
                    if let Some(px) = font_size {
                        let fraction = px / f64::from(scratch.project().active().video.height);
                        bounded(fraction, 0.01, 1.0)?;
                        style.font_size = fraction;
                    }
                    (
                        Command::UpdateClip {
                            clip_id: id,
                            patch: ClipPatch {
                                text: Some(Some(style)),
                                ..Default::default()
                            },
                        },
                        None,
                        format!("Update text {clip_id}"),
                    )
                }
                Operation::AddTrack { bottom, alias } => {
                    check_alias(&alias, &aliases)?;
                    (
                        Command::AddTrack { bottom },
                        Some(alias),
                        "Add native track".into(),
                    )
                }
                Operation::RemoveTrack {
                    track_id,
                    delete_clips,
                } => {
                    let id = resolve(&track_id, &aliases)?;
                    track(&scratch, &id)?;
                    if !delete_clips
                        && scratch
                            .project()
                            .active()
                            .clips
                            .iter()
                            .any(|c| c.track_id == id)
                    {
                        return Err(invalid("track still contains clips"));
                    }
                    (
                        Command::RemoveTrack { track_id: id },
                        None,
                        format!("Remove track {track_id}"),
                    )
                }
                Operation::RemoveMany { clip_ids, ripple } => {
                    if clip_ids.is_empty() || clip_ids.len() > 256 {
                        return Err(invalid("expected 1..=256 clip IDs"));
                    }
                    let ids = clip_ids
                        .iter()
                        .map(|id| resolve(id, &aliases))
                        .collect::<Result<Vec<_>, _>>()?;
                    let mut seen = std::collections::HashSet::new();
                    for id in &ids {
                        clip(&scratch, id)?;
                        if !seen.insert(id) {
                            return Err(invalid("duplicate clip reference"));
                        }
                    }
                    (
                        Command::RemoveClips {
                            clip_ids: ids,
                            ripple,
                        },
                        None,
                        format!("Remove {} clips", clip_ids.len()),
                    )
                }
                Operation::RemoveRanges { .. } => {
                    return Err(invalid("range compilation dispatch failed"));
                }
                Operation::Split {
                    clip_id,
                    time: cut,
                    alias,
                } => {
                    check_alias(&alias, &aliases)?;
                    time(cut)?;
                    let id = resolve(&clip_id, &aliases)?;
                    let item = clip(&scratch, &id)?;
                    if cut <= item.start + MIN_DURATION
                        || cut >= item.start + item.duration - MIN_DURATION
                    {
                        return Err(invalid("split outside clip interior"));
                    }
                    (
                        Command::SplitClips {
                            clip_ids: vec![id],
                            time: cut,
                        },
                        Some(alias),
                        format!("Split {clip_id} at {cut:.3}s"),
                    )
                }
                Operation::Remove { clip_id, ripple } => {
                    let id = resolve(&clip_id, &aliases)?;
                    clip(&scratch, &id)?;
                    (
                        Command::RemoveClips {
                            clip_ids: vec![id],
                            ripple,
                        },
                        None,
                        format!("Remove {clip_id}; ripple={ripple}"),
                    )
                }
                Operation::Move {
                    clip_id,
                    start,
                    track_id,
                } => {
                    time(start)?;
                    let id = resolve(&clip_id, &aliases)?;
                    let item = clip(&scratch, &id)?;
                    if start + item.duration > MAX_TIME
                        || !scratch
                            .project()
                            .active()
                            .tracks
                            .iter()
                            .any(|t| t.id == track_id)
                    {
                        return Err(invalid("invalid destination track or span"));
                    }
                    (
                        Command::MoveClips {
                            moves: vec![ClipMove {
                                clip_id: id,
                                start,
                                track_id,
                            }],
                        },
                        None,
                        format!("Move {clip_id} to {start:.3}s"),
                    )
                }
                Operation::Trim {
                    clip_id,
                    edge,
                    delta,
                    ripple,
                } => {
                    let id = resolve(&clip_id, &aliases)?;
                    let item = clip(&scratch, &id)?;
                    if !delta.is_finite() || delta == 0.0 || item.speed_curve.is_some() {
                        return Err(invalid(
                            "trim requires finite nonzero delta and constant speed",
                        ));
                    }
                    let duration = match edge {
                        TrimEdge::Start => item.duration - delta,
                        TrimEdge::End => item.duration + delta,
                    };
                    bounded(duration, MIN_DURATION, MAX_TIME)?;
                    let source_start = item.source_start
                        + if edge == TrimEdge::Start {
                            delta * item.speed
                        } else {
                            0.0
                        };
                    time(source_start)?;
                    if let Some(media) = scratch
                        .project()
                        .media
                        .iter()
                        .find(|m| m.id == item.media_id)
                        && matches!(item.kind, ClipKind::Video | ClipKind::Audio)
                    {
                        let bound = media
                            .duration
                            .ok_or_else(|| invalid("trim source duration is unknown"))?;
                        if source_start + duration * item.speed > bound + 1e-9 {
                            return Err(invalid("trim exceeds source media bounds"));
                        }
                    }
                    expected_trim = Some((id.clone(), duration));
                    (
                        Command::TrimClip {
                            clip_id: id,
                            edge,
                            delta,
                            ripple,
                        },
                        None,
                        format!("Trim {clip_id} by {delta:.3}s"),
                    )
                }
                Operation::Title {
                    text,
                    start,
                    duration,
                    alias,
                } => {
                    check_alias(&alias, &aliases)?;
                    time(start)?;
                    time(duration)?;
                    time(start + duration)?;
                    if duration < MIN_DURATION || text.trim().is_empty() || text.len() > 4096 {
                        return Err(invalid("invalid title text or duration"));
                    }
                    (
                        Command::AddTextClip {
                            track_id: None,
                            above: true,
                            start,
                            style: Some(TextStyle {
                                content: text,
                                ..Default::default()
                            }),
                            duration: Some(duration),
                            offset_y: None,
                        },
                        Some(alias),
                        format!("Title at {start:.3}s for {duration:.3}s"),
                    )
                }
                Operation::Transition {
                    clip_id,
                    transition_id,
                    duration,
                } => {
                    let id = resolve(&clip_id, &aliases)?;
                    let item = clip(&scratch, &id)?;
                    if transition_id.trim().is_empty() || transition_id.len() > 128 {
                        return Err(invalid("invalid transition id"));
                    }
                    let d = duration.unwrap_or(1.0);
                    bounded(d, MIN_DURATION, item.duration)?;
                    (
                        Command::UpdateClip {
                            clip_id: id,
                            patch: ClipPatch {
                                transition_in: Some(Some(Transition {
                                    id: transition_id.clone(),
                                    duration: d,
                                })),
                                ..Default::default()
                            },
                        },
                        None,
                        format!("Add transition {transition_id} to {clip_id}"),
                    )
                }
                Operation::Crop {
                    clip_id,
                    left,
                    top,
                    right,
                    bottom,
                } => {
                    let id = resolve(&clip_id, &aliases)?;
                    let item = clip(&scratch, &id)?;
                    if matches!(item.kind, ClipKind::Audio) {
                        return Err(invalid("audio cannot be cropped"));
                    }
                    for v in [left, top, right, bottom].into_iter().flatten() {
                        bounded(v, 0.0, 0.9)?;
                    }
                    let c = Crop {
                        left: left.unwrap_or(0.0),
                        top: top.unwrap_or(0.0),
                        right: right.unwrap_or(0.0),
                        bottom: bottom.unwrap_or(0.0),
                    }
                    .tidy();
                    (
                        Command::UpdateClip {
                            clip_id: id,
                            patch: ClipPatch {
                                crop: Some(Some(c)),
                                ..Default::default()
                            },
                        },
                        None,
                        format!("Crop {clip_id}"),
                    )
                }
                Operation::Appearance {
                    clip_id,
                    opacity,
                    blend,
                    flip_h,
                    flip_v,
                } => {
                    let id = resolve(&clip_id, &aliases)?;
                    let item = clip(&scratch, &id)?;
                    if matches!(item.kind, ClipKind::Audio) {
                        return Err(invalid("audio has no visual appearance"));
                    }
                    if let Some(op) = opacity {
                        bounded(op, 0.0, 1.0)?;
                    }
                    if let Some(ref b) = blend {
                        if !["", "normal", "multiply", "screen", "add", "lighten", "darken"]
                            .contains(&b.as_str())
                        {
                            return Err(invalid("unsupported blend mode"));
                        }
                    }
                    (
                        Command::UpdateClip {
                            clip_id: id,
                            patch: ClipPatch {
                                opacity,
                                blend,
                                flip_h,
                                flip_v,
                                ..Default::default()
                            },
                        },
                        None,
                        format!("Update appearance {clip_id}"),
                    )
                }
                Operation::Layer {
                    track_id,
                    start,
                    duration,
                    effect_id,
                    name,
                    alias,
                } => {
                    check_alias(&alias, &aliases)?;
                    time(start)?;
                    bounded(duration, MIN_DURATION, MAX_TIME)?;
                    time(start + duration)?;
                    let track_id = track_id
                        .map(|t| resolve(&t, &aliases))
                        .transpose()?;
                    if let Some(ref t) = track_id {
                        track(&scratch, t)?;
                    }
                    if effect_id.trim().is_empty() || name.trim().is_empty() {
                        return Err(invalid("invalid layer effect/name"));
                    }
                    (
                        Command::AddLayerClip {
                            track_id,
                            start,
                            duration: Some(duration),
                            effect_id,
                            name,
                        },
                        Some(alias),
                        format!("Add layer at {start:.3}s"),
                    )
                }
                Operation::Shape {
                    track_id,
                    start,
                    duration,
                    name,
                    alias,
                } => {
                    check_alias(&alias, &aliases)?;
                    time(start)?;
                    bounded(duration, MIN_DURATION, MAX_TIME)?;
                    time(start + duration)?;
                    let track_id = track_id
                        .map(|t| resolve(&t, &aliases))
                        .transpose()?;
                    if let Some(ref t) = track_id {
                        track(&scratch, t)?;
                    }
                    if name.trim().is_empty() {
                        return Err(invalid("invalid shape name"));
                    }
                    (
                        Command::AddShapeClip {
                            track_id,
                            above: true,
                            start,
                            style: None,
                            duration: Some(duration),
                            name,
                        },
                        Some(alias),
                        format!("Add shape at {start:.3}s"),
                    )
                }
                Operation::Cutout { clip_id, enabled } => {
                    let id = resolve(&clip_id, &aliases)?;
                    let item = clip(&scratch, &id)?;
                    if matches!(item.kind, ClipKind::Audio) {
                        return Err(invalid("audio has no cutout"));
                    }
                    let cutout = if enabled {
                        Some(Cutout {
                            mode: CutoutMode::Auto,
                            subject: Subject::Auto,
                            feather: 0.005,
                            strokes: Vec::new(),
                        })
                    } else {
                        None
                    };
                    (
                        Command::SetClipCutout { clip_id: id, cutout },
                        None,
                        format!("Set cutout on {clip_id}"),
                    )
                }
            };
            let result = scratch.apply(command.clone()).map_err(DraftError::from)?;
            if !result.applied {
                return Err(invalid("native operation was refused or made no change"));
            }
            if let Some((id, duration)) = expected_trim
                && (clip(&scratch, &id)?.duration - duration).abs() > 1e-9
            {
                return Err(invalid(
                    "trim was limited by native neighbour/source bounds",
                ));
            }
            // Moves must land as requested; collision correction is a refusal
            // here, not an invisible alteration to the model's edit plan.
            if let Command::MoveClips { moves } = &command {
                for wanted in moves {
                    let actual = clip(&scratch, &wanted.clip_id)?;
                    if (actual.start - wanted.start).abs() > 1e-9
                        || actual.track_id != wanted.track_id
                    {
                        return Err(invalid("destination collides with an existing clip"));
                    }
                }
            }
            if let Some(alias) = alias {
                if let Command::AddClip {
                    start, track_id, ..
                } = &command
                {
                    let actual = clip(
                        &scratch,
                        result
                            .created_id
                            .as_deref()
                            .ok_or_else(|| invalid("placement did not create a clip"))?,
                    )?;
                    if (actual.start - start).abs() > 1e-9 || &actual.track_id != track_id {
                        return Err(invalid("media placement collides"));
                    }
                }
                aliases.insert(
                    alias,
                    result.created_id.ok_or_else(|| {
                        invalid("native operation did not create requested alias")
                    })?,
                );
            }
            summaries.push(summary);
            commands.push(command);
        }
        if scratch.project() == editor.project() {
            return Err(invalid("plan makes no final project change"));
        }
        Ok(CompiledPlan {
            draft: editor.prepare_batch(commands)?,
            summaries,
        })
    }
}
