// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat contributors
use super::*;
use concat_project::commands::NewMedia;
use concat_project::model::MediaKind;
use concat_project::{Command, Editor};
use serde_json::{Value, json};

fn fixture() -> (Editor, String, String) {
    let mut editor = Editor::new();
    let media = editor
        .apply(Command::AddMedia {
            item: NewMedia {
                path: "fixture.mp4".into(),
                name: "fixture".into(),
                duration: Some(10.0),
                kind: MediaKind::Video,
                width: Some(128),
                height: Some(128),
                frame_rate: Some(30.0),
                frame_rate_fraction: Some("30/1".into()),
                video_codec: Some("h264".into()),
                audio_codec: Some("aac".into()),
                has_audio: true,
                audio_tracks: vec![],
                origin: None,
                color_space: Default::default(),
            },
        })
        .unwrap()
        .created_id
        .unwrap();
    let track = editor.project().active().tracks[0].id.clone();
    (editor, media, track)
}
fn compile(editor: &Editor, operations: Value) -> Result<CompiledPlan, PlanError> {
    EditPlan::parse(
        &json!({"version":1,"units":"seconds",
        "timelineId":editor.project().active_timeline_id,"baseRevision":editor.revision(),
        "operations":operations})
        .to_string(),
    )?
    .compile(editor)
}

#[test]
fn placed_media_retime_audio_motion_caption_atomic_roundtrip() {
    let (mut editor, media, track) = fixture();
    let before = editor.project().clone();
    let proposal = compile(&editor, json!([
        {"op":"place","mediaId":media,"trackId":track,"start":0.0,"ripple":false,"as":"video"},
        {"op":"trim","clipId":"@video","edge":"end","delta":-2.0,"ripple":false},
        {"op":"trim","clipId":"@video","edge":"end","delta":1.0,"ripple":false},
        {"op":"speed","clipId":"@video","speed":2.0},
        {"op":"audio","clipId":"@video","volume":0.5,"muted":true,"fadeIn":0.1,"fadeOut":0.2},
        {"op":"transform","clipId":"@video","scale":1.2,"offsetX":0.1,"offsetY":0.2,"rotation":15.0},
        {"op":"keyframe","clipId":"@video","property":"scale","time":2.0,"value":1.5},
        {"op":"title","text":"caption","start":0.0,"duration":3.0,"as":"caption"},
        {"op":"text","clipId":"@caption","content":"Xin chào Việt Nam","fontSize":40.0}
    ])).unwrap();
    let preview = proposal.draft().project().clone();
    assert_eq!(editor.project(), &before);
    let video = preview
        .active()
        .clips
        .iter()
        .find(|c| c.media_id == media)
        .unwrap();
    assert_eq!(video.duration, 4.5);
    assert_eq!(video.speed, 2.0);
    assert_eq!(video.volume, 0.5);
    assert_eq!(video.muted, Some(true));
    assert_eq!(video.keys.len(), 1);
    editor.commit_draft(proposal.into_draft()).unwrap();
    assert_eq!(editor.project(), &preview);
    let settings = concat_project::DocumentSettings {
        name: "test".into(),
        width: 128,
        height: 128,
        rate_num: 30,
        rate_den: 1,
    };
    assert_eq!(
        Editor::from_document(&editor.to_document(&settings))
            .unwrap()
            .project(),
        &preview
    );
    editor.undo();
    assert_eq!(editor.project(), &before);
    editor.redo();
    assert_eq!(editor.project(), &preview);
}

#[test]
fn disjoint_ranges_remove_actual_source_spans_with_ripple() {
    let (mut editor, media, track) = fixture();
    let clip = editor
        .apply(Command::AddClip {
            media_id: media,
            track_id: track,
            start: 0.0,
            ripple: false,
        })
        .unwrap()
        .created_id
        .unwrap();
    let before = editor.project().clone();
    let proposal = compile(
        &editor,
        json!([{"op":"removeRanges","clipId":clip,
        "ranges":[{"start":2.0,"end":3.0},{"start":5.0,"end":6.0}],"ripple":true}]),
    )
    .unwrap();
    let preview = proposal.draft().project().clone();
    let mut clips = preview.active().clips.clone();
    clips.sort_by(|a, b| a.start.total_cmp(&b.start));
    assert_eq!(clips.len(), 3);
    assert_eq!(
        clips
            .iter()
            .map(|c| (c.start, c.source_start, c.duration))
            .collect::<Vec<_>>(),
        vec![(0.0, 0.0, 2.0), (2.0, 3.0, 2.0), (4.0, 6.0, 4.0)]
    );
    editor.commit_draft(proposal.into_draft()).unwrap();
    editor.undo();
    assert_eq!(editor.project(), &before);
}

#[test]
fn extension_beyond_source_and_invalid_keyframe_refuse_whole_plan() {
    let (mut editor, media, track) = fixture();
    let clip = editor
        .apply(Command::AddClip {
            media_id: media,
            track_id: track,
            start: 0.0,
            ripple: false,
        })
        .unwrap()
        .created_id
        .unwrap();
    let before = editor.project().clone();
    for bad in [
        json!({"op":"trim","clipId":clip,"edge":"end","delta":1.0,"ripple":false}),
        json!({"op":"keyframe","clipId":clip,"property":"scale","time":11.0,"value":1.2}),
        json!({"op":"transform","clipId":clip,"scale":100.0}),
        json!({"op":"removeMany","clipIds":[clip,"missing"],"ripple":false}),
    ] {
        assert!(compile(&editor,json!([
            {"op":"title","text":"never committed","start":0.0,"duration":2.0,"as":"title"},bad
        ])).is_err());
        assert_eq!(editor.project(), &before);
    }
}

#[test]
fn track_alias_and_fifty_operations_are_one_undo() {
    let (mut editor, media, _) = fixture();
    let before = editor.project().clone();
    let proposal = compile(
        &editor,
        json!([
            {"op":"addTrack","bottom":false,"as":"lane"},
            {"op":"place","mediaId":media,"trackId":"@lane","start":0.0,"ripple":false,"as":"new"},
            {"op":"removeMany","clipIds":["@new"],"ripple":false},
            {"op":"removeTrack","trackId":"@lane","deleteClips":false}
        ]),
    );
    // A net no-op is refused rather than reporting an approved edit.
    assert!(proposal.is_err());
    let operations = (0..50)
        .map(|n| {
            json!({"op":"title","text":format!("title{n}"),
        "start":n as f64,"duration":0.5,"as":format!("title{n}")})
        })
        .collect::<Vec<_>>();
    let proposal = compile(&editor, json!(operations)).unwrap();
    editor.commit_draft(proposal.into_draft()).unwrap();
    editor.undo();
    assert_eq!(editor.project(), &before);
}

#[test]
fn thirtieth_failed_operation_preserves_live_project_revision_and_next_id() {
    let (mut editor, _, _) = fixture();
    let before = editor.project().clone();
    let revision = editor.revision();
    let mut control = editor.fork_for_draft();
    let mut operations = (0..29)
        .map(|n| {
            json!({"op":"title","text":format!("title{n}"),
        "start":n as f64,"duration":0.5,"as":format!("title{n}")})
        })
        .collect::<Vec<_>>();
    operations.push(json!({"op":"remove","clipId":"missing-thirtieth","ripple":true}));
    assert!(compile(&editor, json!(operations)).is_err());
    assert_eq!(editor.project(), &before);
    assert_eq!(editor.revision(), revision);
    let next = Command::AddTrack { bottom: false };
    assert_eq!(
        editor.apply(next.clone()).unwrap().created_id,
        control.apply(next).unwrap().created_id
    );
}
