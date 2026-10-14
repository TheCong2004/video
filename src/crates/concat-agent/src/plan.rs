// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat contributors
//! Deliberately small first vocabulary. Unsupported source-editor features
//! fail decoding rather than silently becoming successful native edits.
use concat_project::commands::{ClipMove, TrimEdge};
use concat_project::model::{Clip, TextStyle};
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
            let (command, alias, summary) = match operation {
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
                    // First contract only shortens clips; source extension needs
                    // source/speed bounds, not native clamping masquerading as success.
                    let removed = match edge {
                        TrimEdge::Start => delta,
                        TrimEdge::End => -delta,
                    };
                    if !removed.is_finite()
                        || removed <= 0.0
                        || removed >= item.duration - MIN_DURATION
                    {
                        return Err(invalid("trim must shorten within clip bounds"));
                    }
                    (
                        Command::TrimClip {
                            clip_id: id,
                            edge,
                            delta,
                            ripple,
                        },
                        None,
                        format!("Trim {clip_id} by {removed:.3}s"),
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
            };
            let result = scratch.apply(command.clone()).map_err(DraftError::from)?;
            if !result.applied {
                return Err(invalid("native operation was refused or made no change"));
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
        Ok(CompiledPlan {
            draft: editor.prepare_batch(commands)?,
            summaries,
        })
    }
}
