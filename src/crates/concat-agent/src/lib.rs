// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat contributors
//! Structured plans are untrusted data, not executable code or permissions.
//! Compilation uses the existing native editor, never another timeline engine.
pub mod highlight;
mod plan;
pub mod shorts_plan;
pub use plan::{CompiledPlan, EditPlan, PlanError};
pub mod reframe_search;
pub mod runtime;
pub use reframe_search::{
    DetectedKeyframe, GeometrySegment, NormalizedRect, ReframeOptions, SearchMatch, SemanticEntry,
    SemanticIndex, VisualZone, compute_reframe_keyframes, cosine_similarity, smooth_focal_path,
};

#[cfg(test)]
mod extended_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use concat_project::{Command, Editor};
    use serde_json::json;

    fn fixture() -> (Editor, String) {
        let mut editor = Editor::new();
        let id = editor
            .apply(Command::AddTextClip {
                track_id: None,
                above: true,
                start: 0.0,
                style: None,
                duration: Some(10.0),
                offset_y: None,
            })
            .unwrap()
            .created_id
            .unwrap();
        (editor, id)
    }

    fn plan(editor: &Editor, operations: serde_json::Value) -> EditPlan {
        EditPlan::parse(
            &json!({
                "version": 1, "units": "seconds",
                "timelineId": editor.project().active_timeline_id,
                "baseRevision": editor.revision(),
                "operations": operations,
            })
            .to_string(),
        )
        .unwrap()
    }

    #[test]
    fn split_five_parts_uses_descending_absolute_cuts_and_one_undo() {
        let (mut editor, clip_id) = fixture();
        // Offset the clip: split.time is timeline time, never local/source time.
        let track = editor.project().active().clip(&clip_id).unwrap().track_id.clone();
        let moved = plan(&editor, json!([
            {"op":"move","clipId":clip_id,"start":10.0,"trackId":track}
        ])).compile(&editor).unwrap();
        editor.commit_draft(moved.into_draft()).unwrap();
        let next = editor.apply(Command::AddTextClip {
            track_id: Some(track.clone()), above: true, start: 20.0,
            style: None, duration: Some(3.0), offset_y: None,
        }).unwrap().created_id.unwrap();
        let before = editor.project().clone();
        let ascending = json!([
            {"op":"split","clipId":clip_id,"time":12.0,"as":"tail1"},
            {"op":"split","clipId":clip_id,"time":14.0,"as":"tail2"}
        ]);
        let error = plan(&editor, ascending).compile(&editor).err().unwrap();
        assert!(error.to_string().contains("split outside clip interior"));
        assert_eq!(editor.project(), &before);
        let operations = [18.0, 16.0, 14.0, 12.0].iter().enumerate().map(|(i, cut)| {
            json!({"op":"split","clipId":clip_id,"time":cut,"as":format!("part{i}")})
        }).collect::<Vec<_>>();
        let draft = plan(&editor, json!(operations)).compile(&editor).unwrap();
        editor.commit_draft(draft.into_draft()).unwrap();
        let mut pieces = editor.project().active().clips.iter()
            .filter(|clip| clip.id != next).collect::<Vec<_>>();
        pieces.sort_by(|a,b| a.start.total_cmp(&b.start));
        assert_eq!(pieces.len(), 5);
        for (i, clip) in pieces.iter().enumerate() {
            assert_eq!((clip.start, clip.duration, clip.source_start), (10.0 + 2.0*i as f64, 2.0, 2.0*i as f64));
        }
        assert_eq!(editor.project().active().clip(&next).unwrap(), before.active().clip(&next).unwrap());
        editor.undo();
        assert_eq!(editor.project(), &before);
    }

    #[test]
    fn sequential_aliases_draft_approval_one_undo_save_reopen() {
        let (mut editor, clip) = fixture();
        let before = editor.project().clone();
        let compiled = plan(&editor, json!([
            {"op":"split", "clipId":clip, "time":4.0, "as":"tail"},
            {"op":"remove", "clipId":"@tail", "ripple":false},
            {"op":"title", "text":"Xin chào Việt Nam", "start":0.0, "duration":3.0, "as":"caption"}
        ])).compile(&editor).unwrap();
        assert_eq!(editor.project(), &before);
        assert_eq!(compiled.summaries().len(), 3);
        let preview = compiled.draft().project().clone();
        editor.commit_draft(compiled.into_draft()).unwrap();
        assert_eq!(editor.project(), &preview);
        let settings = concat_project::DocumentSettings {
            name: "AI draft".into(),
            width: 1920,
            height: 1080,
            rate_num: 24,
            rate_den: 1,
        };
        let reopened = Editor::from_document(&editor.to_document(&settings)).unwrap();
        assert_eq!(reopened.project(), editor.project());
        editor.undo();
        assert_eq!(editor.project(), &before);
        editor.redo();
        assert_eq!(editor.project(), &preview);
    }

    #[test]
    fn malformed_unknown_oversized_and_wrong_units_refused() {
        assert!(EditPlan::parse("{}").is_err());
        assert!(EditPlan::parse(&" ".repeat(64 * 1024 + 1)).is_err());
        let (editor, _) = fixture();
        let valid = json!({"version":1,"units":"seconds","timelineId":editor.project().active_timeline_id,
            "baseRevision":editor.revision(),"operations":[{"op":"shell","command":"anything"}]});
        assert!(EditPlan::parse(&valid.to_string()).is_err());
        let mut wrong = valid;
        wrong["units"] = json!("milliseconds");
        assert!(EditPlan::parse(&wrong.to_string()).is_err());
    }

    #[test]
    fn move_trim_and_worker_compilation_use_the_same_live_editor() {
        let (mut editor, clip) = fixture();
        let track = editor.project().active().tracks[0].id.clone();
        let input = plan(
            &editor,
            json!([
                {"op":"trim","clipId":clip,"edge":"end","delta":-2.0,"ripple":false},
                {"op":"move","clipId":clip,"start":5.0,"trackId":track}
            ]),
        );
        let snapshot = editor.planning_snapshot();
        let proposal = std::thread::spawn(move || input.compile(&snapshot).unwrap())
            .join()
            .unwrap();
        let preview = proposal.draft().project().clone();
        assert_eq!(preview.active().clips[0].start, 5.0);
        assert_eq!(preview.active().clips[0].duration, 8.0);
        editor.commit_draft(proposal.into_draft()).unwrap();
        assert_eq!(editor.project(), &preview);
    }

    #[test]
    fn reference_and_value_limits_refuse_without_clamping() {
        let (editor, clip) = fixture();
        for operation in [
            json!({"op":"trim","clipId":clip,"edge":"end","delta":-20.0,"ripple":false}),
            json!({"op":"trim","clipId":clip,"edge":"start","delta":11.0,"ripple":false}),
            json!({"op":"move","clipId":clip,"start":-1.0,"trackId":"missing"}),
            json!({"op":"move","clipId":clip,"start":5.0,"trackId":"missing"}),
            json!({"op":"title","text":"x","start":360000.0,"duration":2.0,"as":"caption"}),
            json!({"op":"title","text":"x","start":0.0,"duration":0.0,"as":"caption"}),
        ] {
            assert!(plan(&editor, json!([operation])).compile(&editor).is_err());
        }
        let too_many = json!((0..257).map(|n| json!({"op":"title","text":"x","start":0.0,"duration":1.0,"as":format!("x{n}")})).collect::<Vec<_>>());
        let input = json!({"version":1,"units":"seconds","timelineId":editor.project().active_timeline_id,
            "baseRevision":editor.revision(),"operations":too_many});
        assert!(EditPlan::parse(&input.to_string()).is_err());
    }

    #[test]
    fn collision_correction_is_not_presented_as_requested_move() {
        let (mut editor, clip) = fixture();
        let track = editor.project().active().tracks[0].id.clone();
        editor
            .apply(Command::AddTextClip {
                track_id: Some(track.clone()),
                above: false,
                start: 20.0,
                style: None,
                duration: Some(10.0),
                offset_y: None,
            })
            .unwrap();
        let before = editor.project().clone();
        assert!(
            plan(
                &editor,
                json!([{"op":"move","clipId":clip,"start":21.0,"trackId":track}])
            )
            .compile(&editor)
            .is_err()
        );
        assert_eq!(editor.project(), &before);
    }

    #[test]
    fn stale_unknown_duplicate_alias_and_partial_failure_leave_live_unchanged() {
        let (mut editor, clip) = fixture();
        let stale = plan(
            &editor,
            json!([{"op":"split","clipId":clip,"time":4.0,"as":"tail"}]),
        );
        editor
            .apply(Command::SplitClips {
                clip_ids: vec![clip.clone()],
                time: 6.0,
            })
            .unwrap();
        editor.undo();
        assert!(stale.compile(&editor).is_err());
        let before = editor.project().clone();
        for operations in [
            json!([{"op":"remove","clipId":"missing","ripple":false}]),
            json!([{"op":"split","clipId":clip,"time":4.0,"as":"tail"},
                   {"op":"remove","clipId":"@unknown","ripple":false}]),
            json!([{"op":"split","clipId":clip,"time":4.0,"as":"same"},
                   {"op":"title","text":"x","start":0.0,"duration":2.0,"as":"same"}]),
            json!([{"op":"split","clipId":clip,"time":50.0,"as":"tail"}]),
            json!([{"op":"title","text":"","start":0.0,"duration":2.0,"as":"caption"}]),
        ] {
            assert!(plan(&editor, operations).compile(&editor).is_err());
            assert_eq!(editor.project(), &before);
            assert!(editor.can_redo());
        }
    }
}
