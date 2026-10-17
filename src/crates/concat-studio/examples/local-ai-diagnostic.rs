//! Read-only staged-runtime diagnostic; never calls an inference endpoint.
use concat_studio::local_ai::LocalAiRuntime;
use std::{sync::atomic::AtomicBool, time::Duration};
fn main() -> Result<(), String> {
    let resources = LocalAiRuntime::resources().map_err(|e| format!("Runtime: {e:?}"))?;
    let root = std::env::temp_dir().join(format!("concat-ai-diagnostic-{}", rand::random::<u64>()));
    let result = (|| {
        let mut runtime = LocalAiRuntime::start(
            &resources,
            &root,
            &AtomicBool::new(false),
            Duration::from_secs(180),
        )
        .map_err(|e| format!("Start: {e:?}"))?;
        let client = runtime.client().map_err(|e| format!("Client: {e:?}"))?;
        println!(
            "PROVIDERS={:?}",
            client.ai_providers().map(|rows| rows.len())
        );
        println!("MODELS={:?}", client.ai_models().map(|rows| rows.len()));
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(root);
    result
}
