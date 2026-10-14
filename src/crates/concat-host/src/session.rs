// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer and Concat contributors

//! The engine-owned editing session.
//!
//! Open a project folder and the engine holds the edit: every mutation is a
//! `concat_project` [`Command`], applied with undo recorded, and the new
//! state is what the window draws. The window never keeps a model of its
//! own; it renders the [`Project`] this session hands back.
//!
//! Saving reuses `projects::save`'s temp-file-and-rename, so the document on
//! disk is written by exactly one code path.

use concat_project::model::VideoSettings;
use concat_project::{
    Command, DocumentSettings, DraftError, Editor, PlanningSnapshot, PreparedDraft, Project,
};
use serde::Serialize;

use crate::projects;

/// One open project: its folder, its settings and its undo history.
pub struct Session {
    /// The project folder, for saving.
    path: String,
    settings: DocumentSettings,
    editor: Editor,
    /// The links to retired effects the open rewrote, old id and new, each
    /// pair once: what the window names as the project opens.
    upgraded: Vec<(String, String)>,
    /// The clips whose retired fields the open took off.
    retired: concat_project::RetiredFields,
}

/// What every mutating call returns: the authoritative state plus history
/// availability, so undo/redo affordances are never guessing.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EditorView {
    /// Native mutation revision; advances on undo/redo, not inference.
    pub revision: u64,
    /// The whole project, as the engine holds it.
    pub project: Project,
    /// Whether there is something to undo.
    pub can_undo: bool,
    /// Whether there is something to redo.
    pub can_redo: bool,
    /// The settings as the session holds them - the document's own output
    /// size wins over the manifest's on open.
    pub settings: SettingsView,
    /// The id a creating command minted, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_id: Option<String>,
}

/// The session's settings, as the window shows them.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    /// Project name.
    pub name: String,
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
    /// Frame rate numerator.
    pub rate_num: i64,
    /// Frame rate denominator.
    pub rate_den: i64,
}

impl Session {
    /// Opens a project folder as the editing session.
    ///
    /// A folder with no document yet opens as an empty project rather than
    /// failing, but a document that is there and cannot be read or parsed
    /// is an error, because silently replacing an edit with emptiness is
    /// how projects get lost. `settings` come from the manifest and seed the first timeline
    /// of a project that has no document yet; a document that loads brings
    /// every timeline's own frame with it, and those win, because that is
    /// where an edited frame was saved.
    pub fn open(path: &str, settings: DocumentSettings) -> Result<Session, String> {
        let editor = match projects::read_document(path) {
            Ok(Some(document)) => match Editor::from_document(&document) {
                Some(editor) => editor,
                // The settings-only manifest `create` writes: a project
                // closed before its first edit reopens empty, it is not
                // corrupt.
                None if projects::is_settings_only(&document) => {
                    Editor::with_video(settings.video())
                }
                None if concat_project::document_version(&document)
                    > concat_project::DOCUMENT_VERSION =>
                {
                    return Err(format!(
                        "{path} was saved by a newer Concat than this one: update to open it"
                    ));
                }
                None => {
                    return Err(format!("{path} holds a document this build cannot read"));
                }
            },
            // No document yet - a project created moments ago.
            Ok(None) => Editor::with_video(settings.video()),
            // Unreadable or not JSON: a save cut short, a disk in trouble.
            // The one thing open must not do is answer with an empty
            // project, since the next save would replace what was edited.
            Err(error) => return Err(error),
        };
        // A link to a retired effect opens as a link to the one that stands
        // in for it, its knobs carried over (`[[replaces]]`).
        let mut editor = editor;
        let catalogue = concat_effects::Catalogue::builtin();
        let mut upgraded: Vec<(String, String)> = Vec::new();
        editor.upgrade_links(|link| {
            let old = link.id.clone();
            let changed = catalogue.upgrade(link);
            let pair = (old, link.id.clone());
            if changed && !upgraded.contains(&pair) {
                upgraded.push(pair);
            }
            changed
        });
        // Fields an earlier version wrote and this one does not read, off
        // the clips, with the clips named for the window to say so.
        let retired = editor.drop_retired_fields();
        Ok(Session {
            path: path.to_owned(),
            settings,
            editor,
            upgraded,
            retired,
        })
    }

    /// Opens the project a [`projects::ProjectInfo`] describes.
    pub fn open_info(info: &projects::ProjectInfo) -> Result<Session, String> {
        Session::open(
            &info.path,
            DocumentSettings {
                name: info.name.clone(),
                width: info.width,
                height: info.height,
                rate_num: info.rate_num,
                rate_den: info.rate_den,
            },
        )
    }

    /// The links to retired effects this open read as links to the
    /// effects that stand in for them, old id and new. Empty when the
    /// document named none; the change is the document's once it is saved.
    pub fn upgraded(&self) -> &[(String, String)] {
        &self.upgraded
    }

    /// The clips this open found with fields an earlier version wrote and
    /// this one no longer reads, now taken off: reversed ones that play
    /// forwards, animated ones that hold still. Empty for most documents.
    pub fn retired(&self) -> &concat_project::RetiredFields {
        &self.retired
    }

    /// The project folder.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The session's settings: the project's name, and the frame and rate of
    /// the timeline being edited.
    ///
    /// Built rather than stored, because the frame is the active timeline's
    /// and the active timeline changes under a tab click. Everything that
    /// renders - the monitor, the export - asks here and gets the frame of
    /// whatever it is about to draw.
    pub fn settings(&self) -> DocumentSettings {
        let video = self.editor.project().active().video;
        DocumentSettings {
            name: self.settings.name.clone(),
            width: video.width,
            height: video.height,
            rate_num: video.rate_num,
            rate_den: video.rate_den,
        }
    }

    /// The active timeline's frame and rate.
    pub fn video(&self) -> VideoSettings {
        self.editor.project().active().video
    }

    /// Sets the active timeline's frame and rate, as an edit - undoable,
    /// and this timeline's alone. Anything a frame could not be is ignored
    /// by the command, for the reason a zero dimension is in
    /// [`Session::prepare_save`].
    pub fn set_video(&mut self, video: VideoSettings) -> Result<EditorView, String> {
        let timeline_id = self.editor.project().active_timeline_id.clone();
        self.apply(Command::SetTimelineVideo { timeline_id, video })
    }

    /// The edit as it stands.
    pub fn project(&self) -> &Project {
        self.editor.project()
    }

    /// Read the mutation revision without cloning a planning snapshot.
    pub fn revision(&self) -> u64 {
        self.editor.revision()
    }

    /// Whether there is something to undo.
    pub fn can_undo(&self) -> bool {
        self.editor.can_undo()
    }

    /// Whether there is something to redo.
    pub fn can_redo(&self) -> bool {
        self.editor.can_redo()
    }

    /// The current state without changing anything.
    pub fn view(&self) -> EditorView {
        self.view_with(None)
    }

    fn view_with(&self, created_id: Option<String>) -> EditorView {
        let settings = self.settings();
        EditorView {
            revision: self.editor.revision(),
            project: self.editor.project().clone(),
            can_undo: self.editor.can_undo(),
            can_redo: self.editor.can_redo(),
            settings: SettingsView {
                name: settings.name,
                width: settings.width,
                height: settings.height,
                rate_num: settings.rate_num,
                rate_den: settings.rate_den,
            },
            created_id,
        }
    }

    /// Applies one edit command and returns the new state.
    pub fn apply(&mut self, command: Command) -> Result<EditorView, String> {
        self.apply_within(None, command)
    }

    /// Isolated draft with this exact session's reserved ID high-watermark.
    pub fn planning_snapshot(&self) -> PlanningSnapshot {
        self.editor.planning_snapshot()
    }

    /// Prepare commands without changing the live project or its history.
    pub fn prepare_batch(&self, commands: Vec<Command>) -> Result<PreparedDraft, DraftError> {
        self.editor.prepare_batch(commands)
    }

    /// Apply a human-approved draft only to the unchanged originating Session.
    pub fn commit_draft(&mut self, draft: PreparedDraft) -> Result<EditorView, DraftError> {
        let outcome = self.editor.commit_draft(draft)?;
        Ok(self.view_with(outcome.created_id))
    }

    /// Applies one edit command as a move of `gesture`, so that a knob
    /// dragged through many values is one undo step; see
    /// `concat_project::Editor::apply_within`. `None` is a step of its own.
    pub fn apply_within(
        &mut self,
        gesture: Option<&str>,
        command: Command,
    ) -> Result<EditorView, String> {
        let outcome = self
            .editor
            .apply_within(gesture, command)
            .map_err(|error| error.to_string())?;
        Ok(self.view_with(outcome.created_id))
    }

    /// Ends the gesture in progress: the next command naming it starts a
    /// new undo step.
    pub fn end_gesture(&mut self) {
        self.editor.end_gesture();
    }

    /// Steps the history back one edit.
    pub fn undo(&mut self) -> EditorView {
        self.editor.undo();
        self.view()
    }

    /// Steps the history forward one edit.
    pub fn redo(&mut self) -> EditorView {
        self.editor.redo();
        self.view()
    }

    /// Takes a new name, if there is one, and hands back what a save must
    /// write: the folder and the document. The frame is not a parameter any
    /// more - it is the active timeline's, set through [`Session::set_video`]
    /// as an edit. The disk write is the caller's, so it can happen off the
    /// thread that owns the session; [`Session::save`] does both.
    pub fn prepare_save(&mut self, name: Option<&str>) -> (String, serde_json::Value) {
        if let Some(name) = name {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                self.settings.name = trimmed.to_owned();
            }
        }
        (self.path.clone(), self.editor.to_document(&self.settings))
    }

    /// Writes the session's document to its project folder.
    pub fn save(&mut self, name: Option<&str>) -> Result<(), String> {
        let (path, document) = self.prepare_save(name);
        projects::save(&path, &document)
    }

    /// The document as it would be saved.
    pub fn document(&self) -> serde_json::Value {
        self.editor.to_document(&self.settings)
    }

    /// The active timeline flattened for rendering. This is what export and
    /// preview consume: the engine flattens its own session, so the pixels
    /// rendered are the model's, never a copy of it.
    pub fn flattened_clips(&self) -> Vec<concat_export::ExportClip> {
        concat_export::flatten::flatten_timeline_in(
            self.editor.project(),
            None,
            Some(std::path::Path::new(&self.path)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> DocumentSettings {
        DocumentSettings {
            name: "Test".to_owned(),
            width: 1920,
            height: 1080,
            rate_num: 30,
            rate_den: 1,
        }
    }

    #[test]
    fn worker_draft_commit_undo_disk_save_and_reopen() {
        let scratch =
            std::env::temp_dir().join(format!("concat-draft-session-{}", std::process::id()));
        std::fs::create_dir_all(&scratch).expect("scratch dir");
        let info =
            projects::create(&scratch.to_string_lossy(), "Draft", 1920, 1080, 30, 1).unwrap();
        let mut session = Session::open_info(&info).unwrap();
        let before = session.project().clone();
        let snapshot = session.planning_snapshot();
        let draft = std::thread::spawn(move || {
            snapshot
                .prepare_batch(vec![Command::AddTextClip {
                    track_id: None,
                    above: true,
                    start: 0.0,
                    style: None,
                    duration: Some(3.0),
                    offset_y: None,
                }])
                .unwrap()
        })
        .join()
        .unwrap();
        assert_eq!(session.project(), &before);
        let expected = draft.project().clone();
        let view = session.commit_draft(draft).unwrap();
        assert!(view.can_undo);
        assert!(view.revision > 0);
        assert_eq!(session.project(), &expected);
        session.save(None).unwrap();
        let reopened = Session::open_info(&info).unwrap();
        assert_eq!(reopened.project(), &expected);
        assert_eq!(session.undo().project, before);
        assert_eq!(session.redo().project, expected);
        // This is an explicit, test-owned directory underneath temp_dir.
        std::fs::remove_dir_all(&scratch).unwrap();
    }

    #[test]
    fn reopened_and_mutated_sessions_cannot_approve_old_draft() {
        let scratch =
            std::env::temp_dir().join(format!("concat-stale-session-{}", std::process::id()));
        std::fs::create_dir_all(&scratch).unwrap();
        let info =
            projects::create(&scratch.to_string_lossy(), "Stale", 1920, 1080, 30, 1).unwrap();
        let mut session = Session::open_info(&info).unwrap();
        let draft = session
            .prepare_batch(vec![Command::AddTrack { bottom: false }])
            .unwrap();
        let mut reopened = Session::open_info(&info).unwrap();
        assert!(reopened.commit_draft(draft).is_err());
        let draft = session
            .prepare_batch(vec![Command::AddTrack { bottom: false }])
            .unwrap();
        session.apply(Command::AddTrack { bottom: false }).unwrap();
        session.undo();
        assert!(session.commit_draft(draft).is_err());
        assert!(session.can_redo());
        std::fs::remove_dir_all(&scratch).unwrap();
    }

    #[test]
    fn approved_draft_composites_real_footage_title_and_exports() {
        use concat_core::{FrameRate, frame::Frame};
        use concat_media::{EncodeOptions, Encoder, FrameSink};
        use concat_project::commands::NewMedia;
        use concat_project::model::{MediaKind, TextStyle};
        use std::sync::{Arc, atomic::AtomicBool};

        let root = std::env::temp_dir().join(format!("concat-draft-render-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("source.mp4");
        // No skip-on-error: inability to encode/render is an actual failed
        // acceptance test, never evidence of a working export.
        let mut encoder = Encoder::create(
            &source,
            128,
            128,
            FrameRate::THIRTY,
            &EncodeOptions::default(),
        )
        .unwrap();
        for _ in 0..30 {
            encoder.write_frame(&Frame::black(128, 128)).unwrap();
        }
        encoder.finish().unwrap();
        drop(encoder);
        let info = projects::create(&root.to_string_lossy(), "Render", 128, 128, 30, 1).unwrap();
        let mut session = Session::open_info(&info).unwrap();
        let media = session
            .apply(Command::AddMedia {
                item: NewMedia {
                    path: source.to_string_lossy().into_owned(),
                    name: "source.mp4".into(),
                    duration: Some(1.0),
                    kind: MediaKind::Video,
                    width: Some(128),
                    height: Some(128),
                    frame_rate: Some(30.0),
                    frame_rate_fraction: Some("30/1".into()),
                    video_codec: Some("h264".into()),
                    audio_codec: None,
                    has_audio: false,
                    audio_tracks: vec![],
                    origin: None,
                    color_space: Default::default(),
                },
            })
            .unwrap()
            .created_id
            .unwrap();
        let track = session.project().active().tracks[0].id.clone();
        session
            .apply(Command::AddClip {
                media_id: media,
                track_id: track,
                start: 0.0,
                ripple: false,
            })
            .unwrap();
        let before = session.project().clone();
        let draft = session
            .prepare_batch(vec![Command::AddTextClip {
                track_id: None,
                above: true,
                start: 0.2,
                duration: Some(0.5),
                offset_y: None,
                style: Some(TextStyle {
                    content: "AI".into(),
                    ..Default::default()
                }),
            }])
            .unwrap();
        let titles = crate::titles::Titles::new(&crate::dirs::AppDirs::under(&root.join("cache")));
        let mut clips = concat_export::flatten::flatten_timeline_in(
            draft.project(),
            None,
            Some(std::path::Path::new(session.path())),
        );
        clips.extend(
            titles
                .clips(draft.project(), 128, 128)
                .into_iter()
                .map(|title| title.clip),
        );
        let monitor = crate::preview::Monitor::new();
        let settings = session.settings();
        let frame = |time| {
            monitor
                .frame(
                    Arc::new(clips.clone()),
                    &settings,
                    crate::preview::FrameSpec {
                        time,
                        width: 128,
                        height: 128,
                        moving: false,
                        proxy: false,
                        color_space: Default::default(),
                    },
                )
                .unwrap()
        };
        let bright = |pixels: Vec<u8>| {
            pixels
                .chunks_exact(4)
                .filter(|p| p[0] > 100 && p[1] > 100 && p[2] > 100)
                .count()
        };
        assert_eq!(
            bright(frame(0.05)),
            0,
            "title must not appear before its start"
        );
        assert!(
            bright(frame(0.3)) > 10,
            "title must be composited over real footage"
        );
        assert_eq!(
            bright(frame(0.8)),
            0,
            "title must end at its requested time"
        );
        assert_eq!(
            session.project(),
            &before,
            "rendering a draft cannot mutate live state"
        );
        let times = [0.05, 0.3, 0.8];
        let sheet = crate::contact_sheet::render_sheet(
            &monitor,
            Arc::new(clips.clone()),
            &settings,
            &times,
            Default::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(sheet.times, times);
        let mut missing_layer = clips.clone();
        missing_layer[0].path = root
            .join("missing-footage.mp4")
            .to_string_lossy()
            .into_owned();
        assert!(
            crate::contact_sheet::render_sheet(
                &monitor,
                Arc::new(missing_layer),
                &settings,
                &[0.3],
                Default::default(),
                &AtomicBool::new(false)
            )
            .is_err(),
            "a visible title must not hide a missing footage layer from approval inspection"
        );
        assert_eq!((sheet.width, sheet.height), (384, 128));
        for (index, time) in times.iter().enumerate() {
            let actual = frame(*time);
            for row in 0..128usize {
                assert_eq!(
                    &sheet.pixels
                        [(row * 384 + index * 128) * 4..(row * 384 + index * 128 + 128) * 4],
                    &actual[row * 128 * 4..(row + 1) * 128 * 4],
                    "contact sheet must equal native compositor pixels"
                );
            }
        }
        assert!(
            crate::contact_sheet::render_sheet(
                &monitor,
                Arc::new(clips.clone()),
                &settings,
                &times,
                Default::default(),
                &AtomicBool::new(true)
            )
            .is_err()
        );
        session.commit_draft(draft).unwrap();
        session.save(None).unwrap();
        assert_eq!(
            Session::open_info(&info).unwrap().project(),
            session.project()
        );
        let output = root.join("approved.mp4");
        let request = concat_export::ExportRequest {
            output: output.to_string_lossy().into_owned(),
            width: 128,
            height: 128,
            rate_num: 30,
            rate_den: 1,
            crf: 24,
            preset: "ultrafast".into(),
            codec: concat_media::VideoCodec::H264,
            ten_bit: false,
            rate_mode: concat_media::RateMode::Vbr,
            bitrate_kbps: 0,
            color_range: concat_media::ColorRange::Limited,
            color_space: Default::default(),
            hdr: false,
            clips,
        };
        let cancel = AtomicBool::new(false);
        let mut progress = |_: i64, _: i64, _: &'static str| {};
        concat_export::render(
            &request,
            concat_export::Reporter {
                progress: &mut progress,
                cancel: &cancel,
            },
        )
        .unwrap();
        let metadata = concat_media::probe(&output).unwrap();
        assert!(metadata.duration.is_some());
        assert!(std::fs::metadata(&output).unwrap().len() > 100);
        assert_eq!(session.undo().project, before);
        // The app-global decoder pool retains open readers between previews.
        // Release only this test's source before Windows removes its fixture.
        crate::scheduler().pool().forget(&source);
        for clip in &request.clips {
            crate::scheduler()
                .pool()
                .forget(std::path::Path::new(&clip.path));
        }
        crate::scheduler()
            .pool()
            .close_idle(std::time::Duration::ZERO);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_fresh_project_opens_empty_and_round_trips_a_save() {
        let scratch =
            std::env::temp_dir().join(format!("concat-session-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).expect("scratch dir");
        let info = projects::create(&scratch.to_string_lossy(), "Fresh", 1920, 1080, 30, 1)
            .expect("creates");

        let mut session = Session::open_info(&info).expect("opens the settings-only manifest");
        assert!(!session.can_undo());
        assert_eq!(
            (session.settings().width, session.settings().height),
            (1920, 1080),
            "the manifest's frame seeds the first timeline"
        );
        let view = session
            .apply(Command::AddTrack { bottom: false })
            .expect("adds a track");
        assert!(view.can_undo);
        session
            .set_video(VideoSettings {
                width: 1280,
                height: 720,
                rate_num: 25,
                rate_den: 1,
                color_space: Default::default(),
            })
            .expect("sets the frame");
        session.save(Some("Renamed")).expect("saves");

        let reopened = Session::open(&info.path, settings()).expect("reopens");
        assert_eq!(
            reopened.settings().name,
            "Test",
            "the manifest's name is what open gets"
        );
        assert_eq!(
            (reopened.settings().width, reopened.settings().height),
            (1280, 720),
            "the document's frame wins over the manifest's"
        );
        assert_eq!(reopened.settings().rate_num, 25, "and so does its rate");
        assert_eq!(
            reopened.project().active().tracks.len(),
            session.project().active().tracks.len()
        );

        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn a_corrupt_document_is_refused() {
        let scratch =
            std::env::temp_dir().join(format!("concat-corrupt-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).expect("scratch dir");
        std::fs::write(scratch.join("concat.json"), br#"{"timelines": "garbage"}"#)
            .expect("writes");
        assert!(Session::open(&scratch.to_string_lossy(), settings()).is_err());
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn a_truncated_document_is_refused_rather_than_replaced() {
        let scratch =
            std::env::temp_dir().join(format!("concat-truncated-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).expect("scratch dir");
        // A save cut short by a power cut or an outside tool: the file is
        // there, and it is not JSON. Opening it as an empty project would
        // let the next save replace whatever was edited.
        std::fs::write(
            scratch.join("concat.json"),
            br#"{"version": 1, "timelines": ["#,
        )
        .expect("writes");
        let opened = Session::open(&scratch.to_string_lossy(), settings());
        assert!(opened.is_err(), "a half document is not an empty project");
        let _ = std::fs::remove_dir_all(&scratch);
    }
}
