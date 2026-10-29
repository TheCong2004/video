//! Existing headless business API integration, NOT live GUI Control coverage.
//! Establish the real project/import/edit/export seam before extending its owner.
use concat_api::{Api, AppDirs, Event, ExportSpec, Reply, Request, Response};
use concat_project::{Command, model::VideoSettings};
use std::{
    path::PathBuf,
    process::Command as Process,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
fn value(api: &mut Api, request: Request) -> Reply {
    match api.dispatch(request) {
        Response::Result(v) => v,
        Response::Error(e) => panic!("Native API refused: {:?}: {}", e.code, e.message),
    }
}
fn view(api: &mut Api, request: Request) -> Box<concat_host::session::EditorView> {
    match value(api, request) {
        Reply::View(v) => v,
        _ => panic!("Expected native editor view"),
    }
}
#[test]
fn real_factory_output_imports_into_a_project_undoes_saves_reopens_and_exports() {
    let repo = PathBuf::from(std::env::var_os("KEDI_TEST_REPO").unwrap());
    let media = PathBuf::from(std::env::var_os("CONCAT_CONTROL_TEST_OUTPUT").unwrap());
    assert!(media.is_file());
    let root = repo.join(".runtime/ai-control-existing-api").join(format!(
        "run-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let (tx, rx) = mpsc::channel();
    let mut api = Api::with_dirs(
        AppDirs {
            config: root.join("config"),
            data: root.join("data"),
        },
        Arc::new(move |event| {
            let _ = tx.send(event);
        }),
    );
    api.restrict_writes_to(vec![root.clone()]);
    let project = root.join("Product review").display().to_string();
    let settings = VideoSettings {
        width: 720,
        height: 1280,
        rate_num: 30,
        rate_den: 1,
        ..Default::default()
    };
    view(
        &mut api,
        Request::ProjectCreate {
            location: root.display().to_string(),
            name: "Product review".into(),
            video: Some(settings),
        },
    );
    let imported = view(
        &mut api,
        Request::MediaImport {
            path: project.clone(),
            file: media.display().to_string(),
        },
    );
    let id = imported
        .created_id
        .expect("Actual media import must return its ID");
    let placed = view(
        &mut api,
        Request::EditApply {
            path: project.clone(),
            command: Box::new(Command::AddClipAtFirstFree {
                media_id: id,
                start: 0.0,
            }),
        },
    );
    assert_eq!(placed.project.active().clips.len(), 1);
    let undone = view(
        &mut api,
        Request::EditUndo {
            path: project.clone(),
        },
    );
    assert!(undone.project.active().clips.is_empty());
    let restored = view(
        &mut api,
        Request::EditRedo {
            path: project.clone(),
        },
    );
    assert_eq!(restored.project.active().clips.len(), 1);
    value(
        &mut api,
        Request::ProjectSave {
            path: project.clone(),
            name: None,
        },
    );
    value(
        &mut api,
        Request::ProjectClose {
            path: project.clone(),
            save: true,
        },
    );
    let reopened = view(
        &mut api,
        Request::ProjectOpen {
            path: project.clone(),
        },
    );
    assert_eq!(reopened.project.active().clips.len(), 1);
    let outside = root.parent().unwrap().join("refused-outside.mp4");
    assert!(matches!(
        api.dispatch(Request::ExportRun {
            path: project.clone(),
            spec: ExportSpec {
                output: outside.display().to_string(),
                ..Default::default()
            }
        }),
        Response::Error(_)
    ));
    assert!(!outside.exists());
    let output = root.join("editor-export.mp4");
    let started = match value(
        &mut api,
        Request::ExportRun {
            path: project.clone(),
            spec: ExportSpec {
                output: output.display().to_string(),
                width: Some(360),
                height: Some(640),
                crf: Some(26),
                preset: Some("ultrafast".into()),
                ..Default::default()
            },
        },
    ) {
        Reply::Started(job) => job,
        _ => panic!("Expected real export job"),
    };
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        let event = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("Export did not settle");
        match event {
            Event::ExportDone { job, .. } if job == started.job => break,
            Event::ExportFailed { job, error, .. } if job == started.job => {
                panic!("Native export failed: {}", error.message)
            }
            _ => {}
        }
    }
    let info = concat_media::probe::probe(&output).unwrap();
    let video = info.require_video().unwrap();
    assert_eq!((video.width, video.height), (360, 640));
    assert!(info.audio.is_some());
    assert!((info.duration.unwrap().as_f64() - 6.0).abs() < 0.2);
    let ffmpeg = std::env::var_os("CONCAT_TEST_FFMPEG").unwrap();
    let pcm = Process::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(&output)
        .args([
            "-map", "0:a:0", "-f", "s16le", "-ac", "1", "-ar", "8000", "-",
        ])
        .output()
        .unwrap();
    assert!(pcm.status.success());
    assert!(
        pcm.stdout
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]).unsigned_abs())
            .max()
            .unwrap_or(0)
            > 100
    );
    println!(
        "VERIFIED_EXISTING_API_OUTPUT={} video=360x640 audio=audible duration=6",
        output.display()
    );
}
