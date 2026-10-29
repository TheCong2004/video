// SPDX-License-Identifier: AGPL-3.0-or-later
//! Local stdio controller: --demo explicitly enables mock facts; --operator permits local controls.
use concat_studio::{
    kedi_control::{MAX_CONTROL_BYTES, handle_rpc},
    kedi_live::{KediLiveRuntime, Role, RuntimeConfig, utc_now},
    live_brain::{LiveFeatureFlags, ProductSnapshot},
};
use std::io::{self, BufRead, Read, Write};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    let demo = args.iter().any(|s| s == "--demo");
    let role = if args.iter().any(|s| s == "--operator") {
        Role::Operator
    } else {
        Role::Viewer
    };
    let mut flags = LiveFeatureFlags::from_env();
    if demo {
        flags.live_enabled = true;
    }
    let mut config = RuntimeConfig::demo();
    config.dev_mode = demo;
    if let Ok(v) = std::env::var("KEDI_TENANT_ID") {
        config.tenant_id = v;
    }
    if let Ok(v) = std::env::var("KEDI_STORE_ID") {
        config.store_id = v;
    }
    let mut runtime = KediLiveRuntime::new(config, flags).map_err(|e| format!("{e:?}"))?;
    if demo {
        runtime
            .install_snapshot(ProductSnapshot {
                schema_version: 1,
                tenant_id: runtime.config.tenant_id.clone(),
                store_id: runtime.config.store_id.clone(),
                sku: "DEMO-01".into(),
                revision: 1,
                currency: "VND".into(),
                title: "Xoài sấy demo".into(),
                price_vnd: Some(89000),
                stock_state: "available".into(),
                observed_at: "2026-01-01T00:00:00Z".into(),
                valid_until: "2099-01-01T00:00:00Z".into(),
                verification_source: "mock".into(),
                ..Default::default()
            })
            .map_err(|e| format!("{e:?}"))?;
    }
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut stdout = io::stdout().lock();
    let clock = std::time::Instant::now();
    loop {
        let mut line = Vec::new();
        let n = reader
            .by_ref()
            .take(MAX_CONTROL_BYTES as u64 + 1)
            .read_until(b'\n', &mut line)?;
        if n == 0 {
            break;
        }
        if n > MAX_CONTROL_BYTES {
            writeln!(
                stdout,
                "{{\"jsonrpc\":\"2.0\",\"id\":null,\"error\":{{\"code\":-32600,\"message\":\"Request too large\"}}}}"
            )?;
            break;
        }
        let request = match serde_json::from_slice(&line) {
            Ok(v) => v,
            Err(_) => {
                writeln!(
                    stdout,
                    "{{\"jsonrpc\":\"2.0\",\"id\":null,\"error\":{{\"code\":-32700,\"message\":\"Parse error\"}}}}"
                )?;
                continue;
            }
        };
        if let Some(reply) = handle_rpc(
            &mut runtime,
            request,
            role,
            &utc_now(),
            clock.elapsed().as_millis() as u64,
        ) {
            writeln!(stdout, "{reply}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}
