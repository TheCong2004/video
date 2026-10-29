//! Actual persisted workflow + existing Rust Factory adapter integration.
//! Requires operator-provided media paths; no model, account or synthetic product.
use concat_studio::{
    ai_control::{Capability, Mode, Registry, Status, Step, Store, Workflow, digest},
    factory_edit::{self, EditRequest},
};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::AtomicBool,
};
fn env_path(name: &str) -> PathBuf {
    std::env::var_os(name)
        .unwrap_or_else(|| panic!("Missing {name}"))
        .into()
}
fn step(id: &str, tool: &str, deps: Vec<&str>, input: &str) -> Step {
    Step {
        id: id.into(),
        tool: tool.into(),
        dependencies: deps.into_iter().map(str::to_owned).collect(),
        input: input.parse().unwrap(),
        status: Status::Pending,
        output: None,
        attempts: 0,
        error: None,
        approval_digest: None,
    }
}
fn registry() -> Registry {
    let mut r = Registry::default();
    for (name, read) in [("factory.render", false), ("artifact.validate", true)] {
        r.register(Capability {
            name: name.into(),
            description: name.into(),
            module: "Factory integration".into(),
            input_schema: "{\"type\":\"object\"}".parse().unwrap(),
            output_schema: "{\"type\":\"object\"}".parse().unwrap(),
            scope: "owned-test".into(),
            read_only: read,
            replay_safe: true,
            approval_required: false,
            timeout_seconds: 1800,
        })
        .unwrap();
    }
    r
}
#[test]
fn persisted_supervised_render_resumes_to_actual_artifact_validation() {
    let repo = env_path("KEDI_TEST_REPO");
    let image = env_path("CONCAT_CONTROL_TEST_IMAGE");
    let voice = env_path("CONCAT_CONTROL_TEST_VOICE");
    let ffmpeg = env_path("CONCAT_TEST_FFMPEG");
    assert!(image.is_file() && voice.is_file());
    let root = repo.join(".runtime/ai-control-real-render").join(format!(
        "run-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let r = registry();
    let scopes = BTreeSet::from(["owned-test".to_owned()]);
    let id;
    {
        let store = Store::open(&root.join("workflows")).unwrap();
        let input = format!(
            "{{\"paths\":[\"{}\"],\"audioPath\":\"{}\",\"targetDuration\":6}}",
            image.display().to_string().replace('\\', "/"),
            voice.display().to_string().replace('\\', "/")
        );
        let mut w = Workflow::new(
            "Dựng video 6 giây từ sản phẩm và giọng đọc được cung cấp".into(),
            Mode::Supervised,
            vec![
                step("render", "factory.render", vec![], &input),
                step(
                    "validate",
                    "artifact.validate",
                    vec!["render"],
                    r#"{"path":{"$step":"render","pointer":"/path"}}"#,
                ),
            ],
            &r,
        )
        .unwrap();
        store.save(&w).unwrap();
        assert!(store.claim(&mut w, &r, &scopes).unwrap().is_none());
        assert_eq!(w.steps[0].status, Status::WaitingApproval);
        let reviewed = w.review_input("render").unwrap();
        w.approve("render", &digest(&reviewed)).unwrap();
        let ticket = store.claim(&mut w, &r, &scopes).unwrap().unwrap();
        // Opening a second owner must fail while the worker owns its store.
        assert!(Store::open(&root.join("workflows")).is_err());
        let req = EditRequest {
            paths: vec![PathBuf::from(ticket.input["paths"][0].as_str().unwrap())],
            seconds: 0.5,
            size: (720, 1280),
            script:
                "Sản phẩm tăm nước cầm tay. Xem giá và thông tin từ người bán trước khi đặt hàng."
                    .into(),
            audio_mode: 1,
            voice_index: 0,
            audio_path: ticket.input["audioPath"].as_str().unwrap().into(),
        };
        let output = factory_edit::render_target(
            &factory_edit::python(&repo),
            &ffmpeg,
            &repo.join("src/crates/concat-text/fonts/HankenGrotesk-Regular.ttf"),
            &req,
            &root.join("render-1"),
            ticket.input["targetDuration"].as_f64(),
            &AtomicBool::new(false),
            |s| println!("{s}"),
        )
        .unwrap();
        assert!(output.metadata().unwrap().len() > 10000);
        let value = format!(
            "{{\"path\":\"{}\"}}",
            output.display().to_string().replace('\\', "/")
        );
        w.settle(&ticket, Ok(value.parse().unwrap())).unwrap();
        store.save(&w).unwrap();
        id = w.id;
    }
    {
        let store = Store::open(&root.join("workflows")).unwrap();
        let mut w = store.load(&id).unwrap();
        assert_eq!(w.steps[0].status, Status::Done);
        assert!(!w.finished());
        let t = store.claim(&mut w, &r, &scopes).unwrap().unwrap();
        assert_eq!(t.step, "validate");
        let output = Path::new(t.input["path"].as_str().unwrap());
        let result = Command::new(ffmpeg.with_file_name("ffprobe.exe"))
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_type:format=duration",
                "-of",
                "default=noprint_wrappers=1",
            ])
            .arg(output)
            .output()
            .unwrap();
        assert!(result.status.success());
        let text = String::from_utf8(result.stdout).unwrap();
        assert!(text.contains("codec_type=video") && text.contains("codec_type=audio"));
        let seconds: f64 = text
            .lines()
            .find_map(|l| l.strip_prefix("duration="))
            .unwrap()
            .parse()
            .unwrap();
        assert!((seconds - 6.0).abs() < 0.2);
        println!(
            "VERIFIED_CONTROL_OUTPUT={} duration={seconds}",
            output.display()
        );
        w.settle(
            &t,
            Ok(
                format!("{{\"duration\":{seconds},\"video\":true,\"audio\":true}}")
                    .parse()
                    .unwrap(),
            ),
        )
        .unwrap();
        store.save(&w).unwrap();
        assert!(w.finished());
    }
    println!("VERIFIED_CONTROL_STORE={}", root.display());
}
