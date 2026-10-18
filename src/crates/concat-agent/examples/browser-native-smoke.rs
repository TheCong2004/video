//! Real browser DOM -> encrypted bridge -> validated native automatic edit -> Undo.
//! Uses an isolated timing fixture and fulfilled chat page, not a real AI service.
use concat_agent::EditPlan;
use concat_project::{Command, Editor};
use concat_studio::edit_bridge::{EditBridge, EditRequest};
use serde_json::json;
use std::{
    io::Read,
    path::Path,
    process::{Command as Process, Stdio},
    time::{Duration, Instant},
};

fn main() -> Result<(), String> {
    let five_splits = std::env::args().any(|arg| arg == "--five-splits");
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap();
    let scratch = root.join(".tmp").join(format!(
        "browser-native-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&scratch).map_err(|error| error.to_string())?;
    let mut editor = Editor::new();
    let first = editor
        .apply(Command::AddTextClip {
            track_id: None,
            above: true,
            start: 0.,
            style: None,
            duration: Some(10.),
            offset_y: None,
        })
        .map_err(|error| error.to_string())?
        .created_id
        .unwrap();
    let track = editor
        .project()
        .active()
        .clip(&first)
        .unwrap()
        .track_id
        .clone();
    let second = editor
        .apply(Command::AddTextClip {
            track_id: Some(track),
            above: true,
            start: 10.,
            style: None,
            duration: Some(5.),
            offset_y: None,
        })
        .map_err(|error| error.to_string())?
        .created_id
        .unwrap();
    let before = editor.project().clone();
    let revision = editor.revision();
    let bridge = EditBridge::start(&scratch).map_err(String::from)?;
    let mut command = Process::new(root.join(".deps/omniroute/node.exe"));
    command
        .arg(root.join("tools/ai-edit-mcp/browser-native-client.mjs"))
        .arg(bridge.connection_file())
        .env("CONCAT_NATIVE_BROWSER_SMOKE", "1")
        .env("TMP", &scratch)
        .env("TEMP", &scratch)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    if five_splits {
        command.env("CONCAT_NATIVE_FIVE_SPLITS", "1");
    }
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut commits = 0;
    let success = loop {
        if let Some(call) = bridge.try_next() {
            if !call.active() {
                continue;
            }
            let scope = format!("fixture:{}", editor.revision());
            let result = match &call.request {
                EditRequest::Context {} => json!({"ok":true,"context":{
                    "scope":scope,"timelineId":editor.project().active_timeline_id,
                    "baseRevision":editor.revision(),"clipId":first,
                    "editPlanSchema":serde_json::from_str::<serde_json::Value>(include_str!("../../concat-studio/src/local_edit_schema.json")).unwrap()
                }}),
                EditRequest::Status {} => json!({"ok":true,"applied":commits>0}),
                EditRequest::Propose {
                    scope: actual,
                    plan,
                } if actual == &scope => {
                    match EditPlan::parse(&plan.to_string()).and_then(|plan| plan.compile(&editor))
                    {
                        Ok(plan) => {
                            let count = plan.summaries().len();
                            editor
                                .commit_draft(plan.into_draft())
                                .map_err(|error| error.to_string())?;
                            commits += 1;
                            json!({"ok":true,"applied":true,"count":count})
                        }
                        Err(error) => json!({"ok":false,"applied":false,"error":error.to_string()}),
                    }
                }
                EditRequest::Propose { .. } => {
                    json!({"ok":false,"applied":false,"error":"Stale scope"})
                }
            };
            call.respond(result);
        }
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status.success();
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break false;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut output = String::new();
    let _ = child
        .stdout
        .take()
        .unwrap()
        .take(65536)
        .read_to_string(&mut output);
    if !success {
        let mut error = String::new();
        let _ = child
            .stderr
            .take()
            .unwrap()
            .take(8192)
            .read_to_string(&mut error);
        eprintln!("Fixture diagnostics: {error}");
    }
    assert!(success, "browser-native fixture failed");
    assert!(output.contains("BROWSER_NATIVE_AUTO_APPLY_PASSED"));
    assert_eq!(commits, 1, "DOM rerender must never apply a second time");
    assert!(editor.revision() > revision);
    let timeline = editor.project().active();
    if five_splits {
        let mut pieces = timeline.clips.iter().filter(|clip| clip.id != second).collect::<Vec<_>>();
        pieces.sort_by(|a,b| a.start.total_cmp(&b.start));
        assert_eq!(pieces.len(), 5);
        for (index, clip) in pieces.iter().enumerate() {
            assert_eq!((clip.start, clip.duration, clip.source_start), (2.0*index as f64, 2.0, 2.0*index as f64));
        }
        assert_eq!(timeline.clip(&second).unwrap(), before.active().clip(&second).unwrap());
    } else {
    let clip = timeline.clip(&first).unwrap();
    assert_eq!((clip.start, clip.source_start, clip.duration), (0., 2., 8.));
    assert_eq!(
        (
            timeline.clip(&second).unwrap().start,
            timeline.clip(&second).unwrap().duration
        ),
        (8., 5.)
    );
    let title = timeline
        .clips
        .iter()
        .find(|clip| {
            clip.text
                .as_ref()
                .is_some_and(|text| text.content == "Xin chào")
        })
        .unwrap();
    assert_eq!((title.start, title.duration), (1., 3.));
    }
    assert_ne!(editor.project(), &before);
    editor.undo();
    assert_eq!(
        editor.project(),
        &before,
        "one Undo restores the whole edit"
    );
    let file = bridge.connection_file().to_path_buf();
    drop(bridge);
    for _ in 0..100 {
        if !file.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // Exact owned fixture directory, never the workspace or the user's profile.
    if scratch.parent() == Some(root.join(".tmp").as_path()) {
        let _ = std::fs::remove_dir_all(&scratch);
    }
    println!("PASS: Browser Send -> live context -> DOM result -> DPAPI/TCP -> native {} committed once; stale refused; one Undo restores project.", if five_splits { "five split pieces" } else { "trim+title" });
    Ok(())
}
