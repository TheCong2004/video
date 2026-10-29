//! Single-owner durable worker claims. Unknown submissions are never replayed.
use super::promax_worker_protocol::{Envelope, safe_id};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub struct Ledger {
    root: PathBuf,
    _lock: File,
}
pub enum Claim {
    New,
    Replay(Value),
    Unknown,
}
#[derive(Serialize, Deserialize)]
struct Record {
    command: String,
    run: String,
    step: String,
    idempotency: String,
    attempt: u64,
    fence: u64,
}
fn key(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
fn read(path: &Path) -> Result<Value, String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "LEDGER_READ_FAILED")?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("LEDGER_INVALID".into());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "LEDGER_READ_FAILED")?
        .take(65537)
        .read_to_end(&mut bytes)
        .map_err(|_| "LEDGER_READ_FAILED")?;
    if bytes.len() > 65536 {
        return Err("LEDGER_TOO_LARGE".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "LEDGER_INVALID".into())
}
fn create(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|_| "LEDGER_INVALID")?;
    if bytes.len() > 65536 {
        return Err("LEDGER_TOO_LARGE".into());
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "LEDGER_CREATE_FAILED")?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "LEDGER_WRITE_FAILED".into())
}
impl Ledger {
    pub fn open(root: &Path) -> Result<Self, String> {
        fs::create_dir_all(root).map_err(|_| "LEDGER_UNAVAILABLE")?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("owner.lock"))
            .map_err(|_| "LEDGER_UNAVAILABLE")?;
        lock.try_lock().map_err(|_| "WORKER_ALREADY_RUNNING")?;
        Ok(Self {
            root: root.canonicalize().map_err(|_| "LEDGER_UNAVAILABLE")?,
            _lock: lock,
        })
    }
    pub fn claim(&self, envelope: &Envelope) -> Result<Claim, String> {
        let step = envelope.payload.command["stepId"]
            .as_str()
            .ok_or("LEDGER_INVALID")?;
        let wanted = Record {
            command: envelope.command_id.clone(),
            run: envelope.task_id.clone(),
            step: step.into(),
            idempotency: envelope.idempotency_key.clone(),
            attempt: envelope.attempt_id,
            fence: envelope.fencing_token,
        };
        if [
            &wanted.command,
            &wanted.run,
            &wanted.step,
            &wanted.idempotency,
        ]
        .iter()
        .any(|id| !safe_id(id))
            || wanted.attempt == 0
            || wanted.fence == 0
        {
            return Err("LEDGER_INVALID".into());
        }
        let entries = fs::read_dir(&self.root).map_err(|_| "LEDGER_READ_FAILED")?;
        let mut count = 0;
        let mut scanned = 0;
        for entry in entries {
            scanned += 1;
            if scanned > 4001 {
                return Err("LEDGER_CAPACITY".into());
            }
            let path = entry.map_err(|_| "LEDGER_READ_FAILED")?.path();
            if !path
                .file_name()
                .and_then(|v| v.to_str())
                .is_some_and(|v| v.ends_with(".claim.json"))
            {
                continue;
            }
            count += 1;
            if count > 1000 {
                return Err("LEDGER_CAPACITY".into());
            }
            let old: Record = serde_json::from_value(read(&path)?).map_err(|_| "LEDGER_INVALID")?;
            if old.command == wanted.command {
                if serde_json::to_value(&old).ok() != serde_json::to_value(&wanted).ok() {
                    return Err("COMMAND_ID_REUSED".into());
                }
                let result = self
                    .root
                    .join(format!("{}.result.json", key(&wanted.command)));
                return if result.exists() {
                    let result = read(&result)?;
                    validate_terminal(envelope, &result)?;
                    Ok(Claim::Replay(result))
                } else {
                    Ok(Claim::Unknown)
                };
            }
            if old.idempotency == wanted.idempotency
                || (old.run == wanted.run && old.step == wanted.step && old.fence >= wanted.fence)
            {
                return Err("STALE_FENCE_OR_IDEMPOTENCY".into());
            }
            // Unknown prior provider mutation blocks even a higher attempt/fence.
            if old.run == wanted.run
                && old.step == wanted.step
                && !self
                    .root
                    .join(format!("{}.result.json", key(&old.command)))
                    .exists()
            {
                return Err("PRIOR_OUTCOME_UNKNOWN".into());
            }
        }
        if count >= 1000 {
            return Err("LEDGER_CAPACITY".into());
        }
        create(
            &self
                .root
                .join(format!("{}.claim.json", key(&wanted.command))),
            &wanted,
        )?;
        Ok(Claim::New)
    }
    pub fn finish(&self, envelope: &Envelope, result: &Value) -> Result<(), String> {
        validate_terminal(envelope, result)?;
        let claim_path = self
            .root
            .join(format!("{}.claim.json", key(&envelope.command_id)));
        let claim: Record =
            serde_json::from_value(read(&claim_path)?).map_err(|_| "LEDGER_INVALID")?;
        if claim.command != envelope.command_id
            || claim.run != envelope.task_id
            || claim.step != envelope.payload.command["stepId"].as_str().unwrap_or("")
            || claim.attempt != envelope.attempt_id
            || claim.fence != envelope.fencing_token
            || claim.idempotency != envelope.idempotency_key
        {
            return Err("LEDGER_RESULT_MISMATCH".into());
        }
        create(
            &self
                .root
                .join(format!("{}.result.json", key(&envelope.command_id))),
            result,
        )
    }
    pub fn acknowledged(&self, command: &str) -> Result<(), String> {
        if !safe_id(command) {
            return Err("ACK_INVALID".into());
        }
        let claim = self.root.join(format!("{}.claim.json", key(command)));
        let result = self.root.join(format!("{}.result.json", key(command)));
        if !claim.is_file() || !result.is_file() {
            return Err("ACK_UNKNOWN".into());
        }
        let path = self.root.join(format!("{}.ack.json", key(command)));
        if path.exists() {
            return Ok(());
        }
        create(&path, &serde_json::json!({"acknowledged":true}))
    }
}
fn validate_terminal(envelope: &Envelope, result: &Value) -> Result<(), String> {
    validate_receipt(result)?;
    if result["commandId"] != envelope.command_id
        || result["runId"] != envelope.task_id
        || result["stepId"] != envelope.payload.command["stepId"]
        || result["attemptId"].as_u64() != Some(envelope.attempt_id)
        || result["fencingToken"].as_u64() != Some(envelope.fencing_token)
        || !matches!(
            result["status"].as_str(),
            Some("completed" | "cancelled" | "failed")
        )
    {
        return Err("LEDGER_RESULT_MISMATCH".into());
    }
    Ok(())
}
/// Only bounded opaque artifact/checkpoint metadata may reach a durable result.
pub(crate) fn validate_receipt(value: &Value) -> Result<(), String> {
    fn walk(value: &Value, key: &str, depth: usize) -> bool {
        if depth > 8 {
            return false;
        }
        match value {
            Value::Null | Value::Bool(_) | Value::Number(_) => true,
            Value::String(s) => {
                s.len() <= 512
                    && !s.contains("..")
                    && !s.contains('\\')
                    && !s.contains("http")
                    && !s.contains("data:")
                    && !s.contains('@')
                    && !s.chars().any(char::is_control)
                    && (if key == "completedAt" {
                        chrono::DateTime::parse_from_rfc3339(s).is_ok()
                    } else if key == "error" {
                        s == "Local stage cancellation was confirmed." || s == "Local stage failed."
                    } else if matches!(key, "mimeType" | "mime_type") {
                        matches!(
                            s.as_str(),
                            "image/png" | "image/jpeg" | "image/webp" | "video/mp4"
                        )
                    } else {
                        s.bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b))
                            && !s.starts_with('/')
                            && !s.contains(":/")
                    })
            }
            Value::Array(items) => {
                items.len() <= 16 && items.iter().all(|v| walk(v, key, depth + 1))
            }
            Value::Object(items) => {
                items.len() <= 32
                    && items.iter().all(|(k, v)| {
                        [
                            "commandId",
                            "runId",
                            "stepId",
                            "attemptId",
                            "fencingToken",
                            "status",
                            "error",
                            "errorCode",
                            "output",
                            "checkpoint",
                            "completedAt",
                            "outputs",
                            "artifacts",
                            "artifactId",
                            "resourceId",
                            "storageScope",
                            "location",
                            "locationRef",
                            "kind",
                            "metadata",
                            "mimeType",
                            "mime_type",
                            "sha256",
                            "bytes",
                            "width",
                            "height",
                            "duration",
                            "durationSeconds",
                            "videoArtifact",
                            "imageArtifact",
                            "confirmed",
                        ]
                        .contains(&k.as_str())
                            && walk(v, k, depth + 1)
                    })
            }
        }
    }
    if walk(value, "", 0) && serde_json::to_vec(value).map_or(false, |v| v.len() <= 65536) {
        Ok(())
    } else {
        Err("UNSAFE_RESULT_METADATA".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::promax_worker_protocol::fixture;
    use serde_json::json;
    fn root() -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("concat-worker-ledger-{:x}", rand::random::<u128>()));
        fs::create_dir(&root).unwrap();
        root
    }
    #[test]
    fn duplicate_completed_command_replays_only_durable_result() {
        let root = root();
        let ledger = Ledger::open(&root).unwrap();
        let (_, envelope) = fixture();
        assert!(matches!(ledger.claim(&envelope).unwrap(), Claim::New));
        let result = envelope.terminal(
            "completed",
            json!({"output":{"resourceId":"asset-1","sha256":"abc"}}),
        );
        ledger.finish(&envelope, &result).unwrap();
        assert!(matches!(ledger.claim(&envelope).unwrap(),Claim::Replay(value)if value==result));
        ledger.acknowledged(&envelope.command_id).unwrap();
        assert!(matches!(ledger.claim(&envelope).unwrap(), Claim::Replay(_)));
        drop(ledger);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn unknown_claim_survives_restart_and_blocks_higher_attempt() {
        let root = root();
        let (_, envelope) = fixture();
        {
            let ledger = Ledger::open(&root).unwrap();
            assert!(matches!(ledger.claim(&envelope).unwrap(), Claim::New));
            assert!(Ledger::open(&root).is_err());
        }
        let ledger = Ledger::open(&root).unwrap();
        assert!(matches!(ledger.claim(&envelope).unwrap(), Claim::Unknown));
        let mut next = envelope.clone();
        next.command_id = "cmd-2".into();
        next.idempotency_key = "run-1:stage-1:2".into();
        next.attempt_id = 2;
        next.fencing_token = 2;
        assert!(ledger.claim(&next).is_err());
        drop(ledger);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn lower_fence_and_reused_command_identity_are_rejected() {
        let root = root();
        let ledger = Ledger::open(&root).unwrap();
        let (_, envelope) = fixture();
        ledger.claim(&envelope).unwrap();
        let result = envelope.terminal("completed", json!({"output":{"resourceId":"asset-1"}}));
        ledger.finish(&envelope, &result).unwrap();
        let mut other = envelope.clone();
        other.task_id = "other-run".into();
        assert!(ledger.claim(&other).is_err());
        other = envelope.clone();
        other.command_id = "cmd-2".into();
        other.idempotency_key = "other-key".into();
        assert!(ledger.claim(&other).is_err());
        drop(ledger);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn ledger_never_persists_commands_prompts_urls_or_media() {
        let root = root();
        let ledger = Ledger::open(&root).unwrap();
        let (_, mut envelope) = fixture();
        envelope.payload.command["inputPayload"]["prompt"] = json!("TOP_SECRET_PROMPT");
        envelope.payload.command["inputPayload"]["sourceArtifact"] =
            json!({"dataUrl":"data:image/png;base64,SECRET_MEDIA"});
        ledger.claim(&envelope).unwrap();
        for entry in fs::read_dir(&root).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                let content = fs::read_to_string(path).unwrap();
                assert!(
                    !content.contains("TOP_SECRET")
                        && !content.contains("SECRET_MEDIA")
                        && !content.contains("data:")
                );
            }
        }
        for value in [
            json!({"output":{"prompt":"secret"}}),
            json!({"output":{"resourceId":"https://provider.test/signed?token=secret"}}),
            json!({"output":{"location":"C:\\private\\video.mp4"}}),
        ] {
            assert!(validate_receipt(&value).is_err());
        }
        let cancelled = envelope.terminal("cancelled", json!({}));
        ledger.finish(&envelope, &cancelled).unwrap();
        drop(ledger);
        fs::remove_dir_all(root).unwrap();
    }
}
