// SPDX-License-Identifier: AGPL-3.0-or-later
//! Opt-in real native renderer checks, with fixtures owned by the caller.
use super::*;
#[test]
#[ignore = "Requires explicit local fixture/output directory; renders real MP4 files"]
fn real_native_analysis_draft_captions_and_atomic_export() {
    let root = PathBuf::from(std::env::var_os("CONCAT_SHORTS_TEST_ROOT").expect("owned test root"));
    let source =
        PathBuf::from(std::env::var_os("CONCAT_SHORTS_TEST_SOURCE").expect("fixture source"));
    let sidecar = root.join("fixture.srt");
    assert!(root.is_dir());
    std::fs::write(&sidecar,"1\n00:00:00,200 --> 00:00:01,200\nXin chào Việt Nam.\n\n2\n00:00:01,200 --> 00:00:02,200\nChúng ta cùng dựng video.\n\n3\n00:00:06,200 --> 00:00:07,200\nNhững dấu tiếng Việt rõ ràng.\n\n4\n00:00:07,200 --> 00:00:08,200\nCảm ơn và hẹn gặp lại.\n").unwrap();
    let dirs = concat_host::AppDirs {
        config: root.join("profile/config"),
        data: root.join("profile/data"),
    };
    let engine = Engine::new(
        dirs,
        Arc::new(concat_speech::transcribe::Transcriber::new()),
    )
    .unwrap();
    let cancel = AtomicBool::new(false);
    let progress: Progress = Arc::new(|_, _| {});
    let cfg = Config {
        width: 180,
        height: 320,
        min_duration: 1.,
        max_duration: 4.,
        target_count: 2,
        transcript_path: Some(sidecar),
        ..Default::default()
    };
    let mut analysis = engine
        .analyze(&source, cfg.clone(), None, &cancel, progress.clone())
        .unwrap();
    assert_eq!(analysis.segments.len(), 4);
    assert!(!analysis.signals.is_empty());
    assert!(!analysis.highlights.is_empty());
    assert!(analysis.highlights.iter().all(|h| h.needs_review));
    // Strict parser test fixture selections, not a real paid provider call.
    analysis.highlights=highlight::resolve_model_response(r#"{"clips":[{"startSegmentId":1,"endSegmentId":2,"title":"Xin chào Việt Nam"},{"startSegmentId":3,"endSegmentId":4,"title":"Cảm ơn"}]}"#,&analysis.segments,&analysis.highlights,analysis.source.duration,&cfg.options()).unwrap();
    assert_eq!(analysis.highlights.len(), 2);
    let editor = Editor::with_video(VideoSettings::default());
    let before = editor.project().clone();
    let draft = engine
        .draft(&analysis, &analysis.highlights[0].id, &editor)
        .unwrap();
    assert_ne!(draft.project(), &before);
    assert_eq!(editor.project(), &before);
    let outputs = engine
        .export(
            &analysis,
            &analysis
                .highlights
                .iter()
                .map(|h| h.id.clone())
                .collect::<Vec<_>>(),
            &root,
            &cancel,
            progress.clone(),
        )
        .unwrap();
    assert_eq!(outputs.len(), 2);
    for out in &outputs {
        assert!(out.video_path.is_file());
        assert!(out.cover_path.as_ref().unwrap().is_file());
        assert!(out.metadata_path.is_file());
        assert_eq!((out.qa.width, out.qa.height), (180, 320));
        assert!(out.qa.decoded_samples >= 5);
        assert!(out.qa.full_decode);
        assert!((out.qa.duration - 2.).abs() < 0.15);
    }
    let queue = engine.queue().list().unwrap();
    assert!(queue.iter().any(|j| j.stage == Stage::Completed));
    let again = engine
        .analyze(&source, cfg, None, &cancel, progress)
        .unwrap();
    assert_eq!(again.job_id, analysis.job_id);
    let mut forged = analysis.clone();
    forged.source.width += 2;
    assert!(
        engine
            .draft(&forged, &forged.highlights[0].id, &editor)
            .is_err()
    );
    assert!(
        engine
            .preview(
                &analysis,
                &analysis.highlights[0].id,
                &AtomicBool::new(true)
            )
            .is_err()
    );
}
