// SPDX-License-Identifier: AGPL-3.0-or-later
//! Replay the canonical event fixtures through the same owner used by the desktop.
use concat_studio::{kedi_live::*, live_brain::*};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = KediLiveRuntime::new(
        RuntimeConfig {
            session_id: "demo-live-01".into(),
            tenant_id: "demo".into(),
            store_id: "demo_store".into(),
            dev_mode: true,
        },
        LiveFeatureFlags {
            live_enabled: true,
            ..Default::default()
        },
    )
    .map_err(|e| format!("{e:?}"))?;
    let snapshot: ProductSnapshot = serde_json::from_str(include_str!(
        "../../../../docs/kedi-live-codex/examples/product_snapshot.json"
    ))?;
    let sku = snapshot.sku.clone();
    let revision = snapshot.revision;
    runtime
        .install_snapshot(snapshot)
        .map_err(|e| format!("{e:?}"))?;
    let now = "2026-10-26T03:30:00Z";
    for (id, action) in [
        ("feature", ControlAction::Feature { sku, revision }),
        ("start", ControlAction::Start),
    ] {
        runtime
            .execute(
                ControlRequest {
                    command_id: id.into(),
                    expected_revision: runtime.status().revision,
                    action,
                },
                Role::Operator,
                now,
                0,
            )
            .map_err(|e| format!("{e:?}"))?;
    }
    for (index, line) in include_str!("../../../../docs/kedi-live-codex/examples/events.ndjson")
        .lines()
        .enumerate()
    {
        let event: LiveEvent = serde_json::from_str(line)?;
        let at = event.received_at.clone();
        let accepted = runtime
            .ingest(event, &at, index as u64 * 1000)
            .map_err(|e| format!("{e:?}"))?;
        println!(
            "{}",
            serde_json::json!({"kind":"ingress","accepted":accepted})
        );
        if let Some(task) = runtime.dispatch(&at, index as u64 * 1000) {
            println!(
                "{}",
                serde_json::json!({"kind":"simulated_speech","completion_source":"simulated","task":task})
            );
            runtime.playback_started(task.generation_id, &at);
            runtime.playback_finished(task.generation_id, true);
        }
    }
    runtime
        .execute(
            ControlRequest {
                command_id: "stop".into(),
                expected_revision: 0,
                action: ControlAction::Stop,
            },
            Role::Operator,
            now,
            10000,
        )
        .map_err(|e| format!("{e:?}"))?;
    println!(
        "{}",
        serde_json::json!({"kind":"final_status","status":runtime.status()})
    );
    Ok(())
}
