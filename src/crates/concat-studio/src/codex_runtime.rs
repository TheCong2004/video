// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded app-server health handshake in a separate runtime home. No developer
//! credentials, repository tools, model turn or approval authority are imported.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::Stdio,
    sync::mpsc,
    time::{Duration, Instant},
};
/// Health is not a claim that runtime model/tool execution has been enabled.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(missing_docs)]
pub struct Health {
    pub protocol_ready: bool,
    pub authenticated: bool,
    pub execution_enabled: bool,
}
fn response(value: Value, id: u64) -> Result<Option<Value>, String> {
    if value.get("id").and_then(Value::as_u64) != Some(id) {
        return Ok(None);
    }
    if value.get("error").is_some() {
        return Err("Codex app-server refused this protocol request".into());
    }
    value
        .get("result")
        .cloned()
        .map(Some)
        .ok_or("Invalid app-server response".into())
}
/// Probe only initialize/account-read; never start a model turn or silently log in.
pub fn probe(executable: &Path, root: &Path) -> Result<Health, String> {
    std::fs::create_dir_all(root.join("home"))
        .map_err(|_| "Cannot create isolated Codex runtime home")?;
    std::fs::create_dir_all(root.join("sandbox")).map_err(|_| "Cannot create runtime sandbox")?;
    let mut command = crate::lux::make_command(executable);
    command
        .args(["app-server", "--listen", "stdio://"])
        .current_dir(root.join("sandbox"))
        .env_clear()
        .env("CODEX_HOME", root.join("home"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for key in [
        "PATH",
        "SystemRoot",
        "WINDIR",
        "TEMP",
        "TMP",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "COMSPEC",
        "PATHEXT",
    ] {
        if let Some(v) = std::env::var_os(key) {
            command.env(key, v);
        }
    }
    let mut child = command
        .spawn()
        .map_err(|_| "Codex executable unavailable; choose its actual installed path")?;
    #[cfg(windows)]
    let _job = match crate::local_ai::owned_job(&child) {
        Ok(j) => j,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Cannot own Codex runtime process".into());
        }
    };
    let output = child.stdout.take().ok_or("Missing Codex stdout")?;
    let mut input = child.stdin.take().ok_or("Missing Codex stdin")?;
    let (tx, rx) = mpsc::sync_channel(16);
    std::thread::spawn(move || {
        let mut reader = BufReader::new(output);
        loop {
            let mut line = String::new();
            match reader.by_ref().take(1024 * 1024 + 1).read_line(&mut line) {
                Ok(0) => break,
                Ok(_) if line.len() <= 1024 * 1024 => {
                    if let Ok(v) = serde_json::from_str::<Value>(&line) {
                        if tx.send(v).is_err() {
                            break;
                        }
                    }
                }
                _ => break,
            }
        }
    });
    let result = (|| {
        let send = |input: &mut std::process::ChildStdin, v: Value| -> Result<(), String> {
            writeln!(input, "{v}")
                .and_then(|_| input.flush())
                .map_err(|_| "Codex connection closed".into())
        };
        send(
            &mut input,
            json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"concat-runtime-health","version":"1.0.0"},"capabilities":{}}}),
        )?;
        for id in [1, 2] {
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                let v = rx
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .map_err(|_| "Codex app-server handshake timed out")?;
                // Probe grants no server-initiated command, filesystem or permission request.
                if v.get("method").is_some() && v.get("id").is_some() {
                    send(
                        &mut input,
                        json!({"id":v["id"],"error":{"code":-32601,"message":"Runtime execution is not enabled by the health probe"}}),
                    )?;
                    continue;
                }
                if let Some(result) = response(v, id)? {
                    if id == 1 {
                        send(&mut input, json!({"method":"initialized"}))?;
                        send(
                            &mut input,
                            json!({"id":2,"method":"account/read","params":{"refreshToken":false}}),
                        )?;
                        break;
                    }
                    return Ok(Health {
                        protocol_ready: true,
                        authenticated: result.get("account").is_some_and(|a| !a.is_null()),
                        execution_enabled: false,
                    });
                }
            }
        }
        Err("Missing Codex health response".into())
    })();
    let _ = child.kill();
    let _ = child.wait();
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unrelated_events_and_errors_cannot_claim_a_connection() {
        assert!(
            response(json!({"method":"turn/completed"}), 1)
                .unwrap()
                .is_none()
        );
        assert!(
            response(json!({"id":1,"error":{"message":"secret fixture"}}), 1)
                .unwrap_err()
                .contains("refused")
        );
        assert!(response(json!({"id":1}), 1).is_err());
    }
    #[test]
    fn authentication_is_not_execution_authority() {
        let h = Health {
            protocol_ready: true,
            authenticated: true,
            execution_enabled: false,
        };
        assert_eq!(serde_json::to_value(h).unwrap()["executionEnabled"], false);
    }
}
