//! SDK stdio -> DPAPI -> loopback -> native draft -> explicit commit/Undo.
//! No GUI or paid AI provider is involved; isolated fixture only.
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
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap();
    let data = std::env::temp_dir().join(format!("concat-mcp-native-{}", std::process::id()));
    let mut editor = Editor::new();
    let clip = editor
        .apply(Command::AddTextClip {
            track_id: None,
            above: true,
            start: 0.,
            style: None,
            duration: Some(10.),
            offset_y: None,
        })
        .map_err(|e| e.to_string())?
        .created_id
        .unwrap();
    let before = editor.project().clone();
    let bridge = EditBridge::start(&data).map_err(String::from)?;
    let mut command = Process::new(root.join(".deps/omniroute/node.exe"));
    command
        .arg(root.join("tools/ai-edit-mcp/smoke-client.mjs"))
        .arg(bridge.connection_file())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|_| "Cannot start MCP smoke client")?;
    let deadline = Instant::now() + Duration::from_secs(45);
    let mut proposal = None;
    let outcome = loop {
        if let Some(call) = bridge.try_next() {
            if !call.active() {
                continue;
            }
            let result = match &call.request {
                EditRequest::Context {} => {
                    json!({"ok":true,"context":{"scope":"fixture","timelineId":editor.project().active_timeline_id,"baseRevision":editor.revision(),"clipId":clip}})
                }
                EditRequest::Status {} => {
                    json!({"ok":true,"draftReady":proposal.is_some(),"applied":editor.project()!=&before})
                }
                EditRequest::Propose { scope, plan } => {
                    if scope != "fixture" || proposal.is_some() {
                        json!({"ok":false,"applied":false})
                    } else {
                        match EditPlan::parse(&plan.to_string()).and_then(|p| p.compile(&editor)) {
                            Ok(p) => {
                                proposal = Some(p);
                                json!({"ok":true,"applied":false})
                            }
                            Err(_) => json!({"ok":false,"applied":false}),
                        }
                    }
                }
            };
            call.respond(result);
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|_| "Cannot wait for smoke client")?
        {
            break status.success();
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            break false;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut stdout = String::new();
    let _ = child
        .stdout
        .take()
        .unwrap()
        .take(65536)
        .read_to_string(&mut stdout);
    if !outcome {
        return Err("MCP smoke client failed; no live project involved".into());
    }
    assert!(stdout.contains("MCP_STDIO_DPAPI_NATIVE_DRAFT_PASSED"));
    assert_eq!(
        editor.project(),
        &before,
        "MCP must not apply the proposed edit"
    );
    editor
        .commit_draft(proposal.ok_or("Missing native draft")?.into_draft())
        .map_err(|e| e.to_string())?;
    assert_ne!(editor.project(), &before);
    editor.undo();
    assert_eq!(editor.project(), &before);
    drop(bridge);
    // The bridge cleanup thread removes its exact own encrypted file. The
    // caller may remove the now-empty fixture directory later, never user data.
    println!(
        "PASS: SDK stdio, encrypted connection, stale refusal, native draft, explicit approval, one Undo. No provider usage."
    );
    Ok(())
}
