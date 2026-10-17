// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer and Concat contributors

//! The editing session: a project, its id mint, and undo.
//!
//! Undo is whole-state snapshots, and they are cheap: the project's
//! timelines and clips sit behind `Arc`, so a snapshot copies pointers and
//! a command copies only the clip it writes to. Impossible to get wrong in
//! the way inverse operations are, and a heavy project's two hundred undos
//! are a few megabytes, not a few hundred.
//!
//! Two refinements on "one snapshot per command":
//!
//! - A **gesture** is one undo step. A knob being dragged commits on every
//!   pause of the pointer; the caller names the gesture, and commits that
//!   name the same gesture as the last recorded step fold into it. See
//!   [`Editor::apply_within`].
//! - **View state is not an edit.** Which timeline tab is showing, and the
//!   order of the tabs, is saved with the document but never recorded in
//!   history: undo steps back through what changed the picture, not
//!   through what the person was looking at.

use crate::model::AppliedFilter;
use std::collections::VecDeque;
use std::sync::Arc;

use serde_json::Value;

use crate::commands::{Command, CommandError, IdMint, Outcome, apply};
use crate::doc::{DocumentSettings, from_document, to_document};
use crate::model::{Clip, Project};

const UNDO_DEPTH: usize = 200;

/// The state before one recorded step, and the gesture it belonged to.
struct Snapshot {
    before: Project,
    /// Set while the gesture that produced this step may still continue;
    /// cleared by [`Editor::end_gesture`] or by any step of another kind.
    gesture: Option<String>,
}

/// One editing session: the project, the id mint that keeps its ids unique,
/// and the undo/redo stacks. This is the object a host holds per open
/// project; everything else in the crate is reachable through it.
pub struct Editor {
    authority: Arc<()>,
    revision: u64,
    project: Project,
    mint: IdMint,
    /// Oldest snapshot at the front: overflowing the depth cap evicts from
    /// the front in constant time, where a Vec would shuffle the whole stack.
    undo: VecDeque<Snapshot>,
    redo: Vec<Project>,
}

/// An isolated, validated native batch. Not cloneable or deserializable:
/// consuming it on approval prevents an accidental repeated commit.
pub struct PreparedDraft {
    authority: Arc<()>,
    revision: u64,
    commands: Vec<Command>,
    project: Project,
}

/// Native immutable checkpoint, scoped to one editor opening. A restore uses
/// the same undo stack and does not rewind the ID mint or mutation revision.
#[derive(Clone)]
pub struct Checkpoint {
    authority: Arc<()>,
    project: Project,
}

/// Read-only planning basis that can move to a worker. It preserves the live
/// editor binding without exposing a second mutable owner of that binding.
pub struct PlanningSnapshot {
    editor: Editor,
}

impl std::ops::Deref for PlanningSnapshot {
    type Target = Editor;
    fn deref(&self) -> &Editor {
        &self.editor
    }
}

impl PreparedDraft {
    /// Read-only state for native preview; never the authoritative project.
    pub fn project(&self) -> &Project {
        &self.project
    }
    /// Revision against which this draft was prepared.
    pub fn revision(&self) -> u64 {
        self.revision
    }
}

/// Refusal at the isolated draft or live approval boundary.
#[derive(Debug, thiserror::Error)]
pub enum DraftError {
    /// Another editor or any intervening mutation owns the current state.
    #[error("draft belongs to another editor or an outdated revision")]
    Stale,
    /// Empty, oversized, nested or view-only batch.
    #[error("draft must contain 1..=256 editing operations, with no nested batches")]
    InvalidBatch,
    /// A native operation silently refused its input or was a no-op.
    #[error("operation {0} did not change the project")]
    NoChange(usize),
    /// Sequential compilation cannot reproduce the final native batch.
    #[error("sequential draft and native batch disagree")]
    PreviewMismatch,
    /// A native operation failed.
    #[error(transparent)]
    Command(#[from] CommandError),
}

impl Editor {
    /// Capture a named-checkpoint backing snapshot without adding an undo step.
    pub fn checkpoint(&self) -> Checkpoint {
        Checkpoint { authority: self.authority.clone(), project: self.project.clone() }
    }

    /// Restore within this editor as one native undo step. Old proposals stay
    /// stale even when the checkpoint happens to contain their original state.
    pub fn restore_checkpoint(&mut self, checkpoint: &Checkpoint) -> Result<bool, DraftError> {
        if !Arc::ptr_eq(&self.authority, &checkpoint.authority) { return Err(DraftError::Stale); }
        if self.project == checkpoint.project { return Ok(false); }
        self.end_gesture();
        let before=std::mem::replace(&mut self.project, checkpoint.project.clone());
        self.mint.adopt_project(&self.project);
        self.undo.push_back(Snapshot { before, gesture: None });
        if self.undo.len()>UNDO_DEPTH { self.undo.pop_front(); }
        self.redo.clear();
        self.advance_revision();
        Ok(true)
    }
    /// A fresh, empty project.
    pub fn new() -> Self {
        Self::with_video(crate::model::VideoSettings::default())
    }

    /// A fresh editor whose first timeline renders to `video`.
    pub fn with_video(video: crate::model::VideoSettings) -> Self {
        Self {
            authority: Arc::new(()),
            revision: 0,
            project: Project::with_video(video),
            mint: IdMint::default(),
            undo: VecDeque::new(),
            redo: Vec::new(),
        }
    }

    /// Restores a project from a document, adopting every id it uses so the
    /// mint can never re-issue one. Returns None when the document holds
    /// nothing recognisable.
    pub fn from_document(document: &Value) -> Option<Self> {
        let project = from_document(document)?;
        let mut mint = IdMint::default();
        mint.adopt_project(&project);
        Some(Self {
            authority: Arc::new(()),
            revision: 0,
            project,
            mint,
            undo: VecDeque::new(),
            redo: Vec::new(),
        })
    }

    /// Rewrites the picture links `upgrade` changes - one to a retired
    /// effect, read as a link to the effect that stands in for it - as the
    /// document is read, before any edit: not a step to undo, since it is
    /// what the document means now. Returns whether any link changed.
    pub fn upgrade_links(&mut self, mut upgrade: impl FnMut(&mut AppliedFilter) -> bool) -> bool {
        use std::sync::Arc;
        let mut changed = false;
        for timeline in &mut self.project.timelines {
            for index in 0..timeline.clips.len() {
                let mut links = timeline.clips[index].video_effects.clone();
                let mut any = false;
                for link in &mut links {
                    any |= upgrade(link);
                }
                if any {
                    let timeline = Arc::make_mut(timeline);
                    Arc::make_mut(&mut timeline.clips[index]).video_effects = links;
                    changed = true;
                }
            }
        }
        if changed {
            self.advance_revision();
        }
        changed
    }

    /// Takes off the clip fields earlier versions wrote and this one no
    /// longer reads - 0.2.4's `reverse` flag and its preset animations -
    /// as the document is read, the way [`Editor::upgrade_links`] does: not
    /// a step to undo. They rode along unread, so a reversed clip played
    /// forwards and an animated one sat still with nothing said (audit
    /// 2026-09-28, #5). Returns the names of the clips that had each, so
    /// the open can say which; the next save makes it so.
    pub fn drop_retired_fields(&mut self) -> RetiredFields {
        use std::sync::Arc;
        const ANIMATIONS: [&str; 4] = [
            "animationIn",
            "animationOut",
            "animationCombo",
            "animationLoop",
        ];
        let mut found = RetiredFields::default();
        let mut changed = false;
        for timeline in &mut self.project.timelines {
            for index in 0..timeline.clips.len() {
                let extra = &timeline.clips[index].extra;
                let reversed = extra.get("reverse").and_then(|v| v.as_bool()) == Some(true);
                let animated = ANIMATIONS
                    .iter()
                    .any(|key| extra.get(*key).is_some_and(|value| !value.is_null()));
                let any = extra.contains_key("reverse")
                    || ANIMATIONS.iter().any(|key| extra.contains_key(*key));
                if !any {
                    continue;
                }
                changed = true;
                let name = timeline.clips[index].name.clone();
                if reversed && !found.reversed.contains(&name) {
                    found.reversed.push(name.clone());
                }
                if animated && !found.animated.contains(&name) {
                    found.animated.push(name);
                }
                let timeline = Arc::make_mut(timeline);
                let clip = Arc::make_mut(&mut timeline.clips[index]);
                clip.extra.remove("reverse");
                for key in ANIMATIONS {
                    clip.extra.remove(key);
                }
            }
        }
        if changed {
            self.advance_revision();
        }
        found
    }

    /// The current state, read-only: all mutation goes through
    /// [`Editor::apply`] so nothing can change without being undoable.
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// Monotonic mutation revision, including undo/redo and view changes.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn advance_revision(&mut self) {
        self.revision = self
            .revision
            .checked_add(1)
            .expect("editor revision exhausted");
    }

    /// Preserve reserved IDs, including IDs no longer present after undo.
    /// The fork has independent history and cannot approve a live draft.
    pub fn fork_for_draft(&self) -> Self {
        Self {
            authority: Arc::new(()),
            revision: 0,
            project: self.project.clone(),
            mint: self.mint.clone(),
            undo: VecDeque::new(),
            redo: Vec::new(),
        }
    }

    /// Capture planning state while holding the Session owner, then release
    /// it before inference. Approval still requires the original live editor.
    pub fn planning_snapshot(&self) -> PlanningSnapshot {
        let mut editor = self.fork_for_draft();
        editor.authority = self.authority.clone();
        editor.revision = self.revision;
        PlanningSnapshot { editor }
    }

    /// Execute only on scratch state, then verify parity with one native batch.
    pub fn prepare_batch(&self, commands: Vec<Command>) -> Result<PreparedDraft, DraftError> {
        if commands.is_empty()
            || commands.len() > 256
            || commands
                .iter()
                .any(|c| c.is_view_state() || matches!(c, Command::Batch { .. }))
        {
            return Err(DraftError::InvalidBatch);
        }
        let mut sequential = self.fork_for_draft();
        for (index, command) in commands.iter().enumerate() {
            if !sequential.apply(command.clone())?.applied {
                return Err(DraftError::NoChange(index));
            }
        }
        let mut batched = self.fork_for_draft();
        batched.apply(Command::Batch {
            commands: commands.clone(),
        })?;
        if batched.project != sequential.project {
            return Err(DraftError::PreviewMismatch);
        }
        Ok(PreparedDraft {
            authority: self.authority.clone(),
            revision: self.revision,
            commands,
            project: batched.project,
        })
    }

    /// Call only after human permission, on the Session owner thread.
    /// A changed editor (even edit then undo) rejects the old proposal.
    pub fn commit_draft(&mut self, draft: PreparedDraft) -> Result<Outcome, DraftError> {
        if !Arc::ptr_eq(&self.authority, &draft.authority) || self.revision != draft.revision {
            return Err(DraftError::Stale);
        }
        Ok(self.apply(Command::Batch {
            commands: draft.commands,
        })?)
    }

    /// The project, writable around the undo stack: for tests that need a
    /// state no command makes any more, such as the stacked clips an older
    /// document can hold.
    #[cfg(test)]
    pub(crate) fn project_mut(&mut self) -> &mut Project {
        &mut self.project
    }

    /// Applies one command, recording the state before it for undo.
    ///
    /// A command that fails leaves the project and history untouched, and
    /// the snapshot is only kept when the command reports it actually
    /// changed something ([`Outcome::applied`]), so undo never replays a
    /// no-op - a missing id, a value already in place.
    pub fn apply(&mut self, command: Command) -> Result<Outcome, CommandError> {
        self.apply_within(None, command)
    }

    /// [`Editor::apply`] as one move of a gesture.
    ///
    /// The first command naming a gesture records a step as usual; every
    /// later command naming the same gesture, with no other step recorded
    /// in between, folds into that step, so undo lands where the gesture
    /// began. Commands that set absolute values (every inspector knob) are
    /// the ones this is for. `None` is a step of its own, and ends any
    /// gesture in progress.
    pub fn apply_within(
        &mut self,
        gesture: Option<&str>,
        command: Command,
    ) -> Result<Outcome, CommandError> {
        if command.is_view_state() {
            let outcome = apply(&mut self.project, &mut self.mint, command)?;
            if outcome.applied {
                self.advance_revision();
            }
            return Ok(outcome);
        }
        let before = self.project.clone();
        let outcome = apply(&mut self.project, &mut self.mint, command)?;
        if !outcome.applied {
            return Ok(outcome);
        }
        self.tidy_touched(&before);
        self.follow_first_hdr(&before);
        let continues = match (gesture, self.undo.back()) {
            (Some(gesture), Some(last)) => last.gesture.as_deref() == Some(gesture),
            _ => false,
        };
        if !continues {
            self.undo.push_back(Snapshot {
                before,
                gesture: gesture.map(str::to_owned),
            });
            if self.undo.len() > UNDO_DEPTH {
                self.undo.pop_front();
            }
        }
        self.redo.clear();
        self.advance_revision();
        Ok(outcome)
    }

    /// Runs [`Clip::tidy`] over every clip the command wrote, so the clamps
    /// live in one place and a document is the same on screen as it is
    /// after a save and a reopen (audit 2026-09-23, #13). Only a clip
    /// whose `Arc` the command replaced is looked at: the rest are still
    /// the snapshot's, and were tidy already.
    fn tidy_touched(&mut self, before: &Project) {
        use std::collections::HashSet;
        use std::sync::Arc;
        let previous: std::collections::HashMap<&str, &Arc<crate::model::Timeline>> = before
            .timelines
            .iter()
            .map(|timeline| (timeline.id.as_str(), timeline))
            .collect();
        for timeline in &mut self.project.timelines {
            let old = previous.get(timeline.id.as_str()).copied();
            if old.is_some_and(|old| Arc::ptr_eq(old, timeline)) {
                continue;
            }
            let kept: HashSet<*const Clip> = old
                .map(|old| old.clips.iter().map(Arc::as_ptr).collect())
                .unwrap_or_default();
            let timeline = Arc::make_mut(timeline);
            for clip in &mut timeline.clips {
                if kept.contains(&Arc::as_ptr(clip)) {
                    continue;
                }
                let tidied = (**clip).clone().tidy();
                if tidied != **clip {
                    *clip = Arc::new(tidied);
                }
            }
        }
    }

    /// Makes an SDR timeline HLG when the command put its first HDR clip on
    /// it - a clip of a file recorded in HLG or PQ, where before it had
    /// none - in the same step, so an undo takes both back. A timeline set
    /// back to SDR with HDR clips already on it stays SDR (decided 26 Sept:
    /// the Modify sheet can change it back), and only a timeline the
    /// command wrote, and did not make, is looked at.
    fn follow_first_hdr(&mut self, before: &Project) {
        use std::collections::HashSet;
        use std::sync::Arc;
        let hdr = |project: &Project| -> HashSet<String> {
            project
                .media
                .iter()
                .filter(|media| media.color_space.is_hdr())
                .map(|media| media.id.clone())
                .collect()
        };
        let now = hdr(&self.project);
        if now.is_empty() {
            return;
        }
        let then = hdr(before);
        let previous: std::collections::HashMap<&str, &Arc<crate::model::Timeline>> = before
            .timelines
            .iter()
            .map(|timeline| (timeline.id.as_str(), timeline))
            .collect();
        for timeline in &mut self.project.timelines {
            if timeline.video.color_space.is_hdr() {
                continue;
            }
            // A timeline the command made keeps the colour it was made
            // with - a copy of one set back to SDR stays SDR.
            let Some(old) = previous.get(timeline.id.as_str()).copied() else {
                continue;
            };
            if Arc::ptr_eq(old, timeline) {
                continue;
            }
            let had = old.clips.iter().any(|clip| then.contains(&clip.media_id));
            let has = timeline
                .clips
                .iter()
                .any(|clip| now.contains(&clip.media_id));
            if has && !had {
                Arc::make_mut(timeline).video.color_space = crate::model::ColorSpace::Hlg;
            }
        }
    }

    /// Ends the gesture in progress, if any: the next command naming it
    /// starts a new step rather than folding into the last one. Called when
    /// the pointer goes up, or when enough time has passed that a second
    /// drag of the same knob is a second edit.
    pub fn end_gesture(&mut self) {
        if let Some(last) = self.undo.back_mut() {
            last.gesture = None;
        }
    }

    /// Steps back one edit. Returns whether anything changed.
    pub fn undo(&mut self) -> bool {
        match self.undo.pop_back() {
            Some(previous) => {
                self.redo
                    .push(std::mem::replace(&mut self.project, previous.before));
                self.advance_revision();
                true
            }
            None => false,
        }
    }

    /// Steps forward again. Returns whether anything changed.
    pub fn redo(&mut self) -> bool {
        match self.redo.pop() {
            Some(next) => {
                let before = std::mem::replace(&mut self.project, next);
                self.undo.push_back(Snapshot {
                    before,
                    gesture: None,
                });
                self.advance_revision();
                true
            }
            None => false,
        }
    }

    /// Whether [`Editor::undo`] would do anything - what the UI's undo
    /// button greys out on.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether [`Editor::redo`] would do anything; the redo button's twin
    /// of [`Editor::can_undo`].
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The full document for saving.
    pub fn to_document(&self, settings: &DocumentSettings) -> Value {
        to_document(settings, &self.project)
    }
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

/// The clips [`Editor::drop_retired_fields`] found, by name, each once.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RetiredFields {
    /// Clips an earlier version played backwards, which now play forwards.
    pub reversed: Vec<String>,
    /// Clips an earlier version moved with a preset animation.
    pub animated: Vec<String>,
}
