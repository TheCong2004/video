// SPDX-License-Identifier: AGPL-3.0-or-later
//! Kedi Tracking Outbox
//! Durable, offline-first transaction event queue for Concat Desktop.
//! Ensures reliable, at-least-once telemetry, audit and usage recording with deduplication.
//! Defined per KEDI_CONCAT_CHROMEX_CODEX_PLAN.md (Gate G3: O01, O02, O03, O04, O05, O06)

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Event types tracked by the outbox (O04)
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutboxEventKind {
    AppOpen,
    ExtensionPaired,
    ProjectOpen,
    ImportStarted,
    ImportCompleted,
    ImportFailed,
    RenderStarted,
    RenderCompleted,
    RenderFailed,
    AiUsageMetered,
}

impl OutboxEventKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AppOpen => "app.open",
            Self::ExtensionPaired => "extension.paired",
            Self::ProjectOpen => "project.open",
            Self::ImportStarted => "import.started",
            Self::ImportCompleted => "import.completed",
            Self::ImportFailed => "import.failed",
            Self::RenderStarted => "render.started",
            Self::RenderCompleted => "render.completed",
            Self::RenderFailed => "render.failed",
            Self::AiUsageMetered => "ai.usage.metered",
        }
    }
}

/// Durable outbox record (O01)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutboxRecord {
    pub event_id: String,
    pub tenant_id: String,
    pub user_id: String,
    pub device_id: String,
    pub stream: String,
    pub sequence: u64,
    pub kind: OutboxEventKind,
    pub payload_redacted: serde_json::Value,
    pub created_at_unix: u64,
    pub acked_at_unix: Option<u64>,
}

/// Thread-safe local outbox journal
pub struct KediOutbox {
    storage_path: PathBuf,
    lock: Mutex<()>,
}

impl KediOutbox {
    /// Opens or creates the outbox journal at the specified path
    pub fn open(storage_path: impl Into<PathBuf>) -> std::io::Result<Self> {
        let path = storage_path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if !path.exists() {
            File::create(&path)?;
        }
        Ok(Self {
            storage_path: path,
            lock: Mutex::new(()),
        })
    }

    /// Enqueues an event locally before network dispatch (O02)
    pub fn enqueue(&self, record: OutboxRecord) -> std::io::Result<bool> {
        let _guard = self.lock.lock().unwrap();

        // Check deduplication
        let existing = self.read_all_unlocked()?;
        if existing.iter().any(|r| r.event_id == record.event_id) {
            return Ok(false); // Already exists (idempotent)
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.storage_path)?;

        let line = serde_json::to_string(&record)?;
        writeln!(file, "{line}")?;
        file.sync_data()?; // Guarantee disk durability (WAL equivalent)
        Ok(true)
    }

    /// Retrieves unacked pending events up to batch_limit (O02)
    pub fn pending_events(&self, batch_limit: usize) -> std::io::Result<Vec<OutboxRecord>> {
        let _guard = self.lock.lock().unwrap();
        let records = self.read_all_unlocked()?;
        let pending = records
            .into_iter()
            .filter(|r| r.acked_at_unix.is_none())
            .take(batch_limit)
            .collect();
        Ok(pending)
    }

    /// Marks a batch of events as acknowledged (ACK) after backend receipt (O02, O03)
    pub fn acknowledge(&self, event_ids: &HashSet<String>, acked_at_unix: u64) -> std::io::Result<usize> {
        let _guard = self.lock.lock().unwrap();
        let mut records = self.read_all_unlocked()?;
        let mut acked_count = 0;

        for record in records.iter_mut() {
            if event_ids.contains(&record.event_id) && record.acked_at_unix.is_none() {
                record.acked_at_unix = Some(acked_at_unix);
                acked_count += 1;
            }
        }

        if acked_count > 0 {
            self.rewrite_all_unlocked(&records)?;
        }

        Ok(acked_count)
    }

    /// Cleans up acknowledged records older than retention_seconds (O06)
    pub fn purge_acked(&self, older_than_unix: u64) -> std::io::Result<usize> {
        let _guard = self.lock.lock().unwrap();
        let records = self.read_all_unlocked()?;
        let initial_count = records.len();
        let retained: Vec<_> = records
            .into_iter()
            .filter(|r| match r.acked_at_unix {
                Some(acked) => acked >= older_than_unix,
                None => true, // Keep all unacked records indefinitely
            })
            .collect();

        let purged = initial_count - retained.len();
        if purged > 0 {
            self.rewrite_all_unlocked(&retained)?;
        }
        Ok(purged)
    }

    fn read_all_unlocked(&self) -> std::io::Result<Vec<OutboxRecord>> {
        let file = File::open(&self.storage_path)?;
        let reader = BufReader::new(file);
        let mut records = Vec::new();

        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(rec) = serde_json::from_str::<OutboxRecord>(trimmed) {
                records.push(rec);
            }
        }
        Ok(records)
    }

    fn rewrite_all_unlocked(&self, records: &[OutboxRecord]) -> std::io::Result<()> {
        let tmp_path = self.storage_path.with_extension("tmp");
        {
            let mut file = File::create(&tmp_path)?;
            for record in records {
                let line = serde_json::to_string(record)?;
                writeln!(file, "{line}")?;
            }
            file.sync_data()?;
        }
        fs::rename(tmp_path, &self.storage_path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_outbox_path(test_name: &str) -> PathBuf {
        let rand_id = rand::random::<u32>();
        std::env::temp_dir().join(format!("kedi_outbox_test_{test_name}_{rand_id}.jsonl"))
    }

    #[test]
    fn enqueue_and_fetch_pending() {
        let path = temp_outbox_path("enqueue");
        let outbox = KediOutbox::open(&path).unwrap();

        let rec = OutboxRecord {
            event_id: "evt_1".to_string(),
            tenant_id: "t1".to_string(),
            user_id: "u1".to_string(),
            device_id: "d1".to_string(),
            stream: "user_actions".to_string(),
            sequence: 1,
            kind: OutboxEventKind::AppOpen,
            payload_redacted: serde_json::json!({"version": "0.2.7"}),
            created_at_unix: 1000,
            acked_at_unix: None,
        };

        assert!(outbox.enqueue(rec).unwrap());

        let pending = outbox.pending_events(10).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].event_id, "evt_1");

        let _ = fs::remove_file(path);
    }

    #[test]
    fn idempotent_deduplication() {
        let path = temp_outbox_path("dedup");
        let outbox = KediOutbox::open(&path).unwrap();

        let rec = OutboxRecord {
            event_id: "evt_dup".to_string(),
            tenant_id: "t1".to_string(),
            user_id: "u1".to_string(),
            device_id: "d1".to_string(),
            stream: "user_actions".to_string(),
            sequence: 1,
            kind: OutboxEventKind::ProjectOpen,
            payload_redacted: serde_json::json!({}),
            created_at_unix: 1000,
            acked_at_unix: None,
        };

        assert!(outbox.enqueue(rec.clone()).unwrap());
        // Second enqueue with identical event_id is rejected (idempotent)
        assert!(!outbox.enqueue(rec).unwrap());

        let pending = outbox.pending_events(10).unwrap();
        assert_eq!(pending.len(), 1);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn offline_accumulation_and_crash_recovery() {
        let path = temp_outbox_path("recovery");
        {
            // Simulate session 1: 3 offline events enqueued, then app killed
            let outbox1 = KediOutbox::open(&path).unwrap();
            for i in 1..=3 {
                outbox1
                    .enqueue(OutboxRecord {
                        event_id: format!("evt_offline_{i}"),
                        tenant_id: "t1".to_string(),
                        user_id: "u1".to_string(),
                        device_id: "d1".to_string(),
                        stream: "offline_queue".to_string(),
                        sequence: i as u64,
                        kind: OutboxEventKind::ImportStarted,
                        payload_redacted: serde_json::json!({"step": i}),
                        created_at_unix: 1000 + i,
                        acked_at_unix: None,
                    })
                    .unwrap();
            }
        } // Drop outbox1 simulating process exit / crash

        // Session 2: App re-opens online
        let outbox2 = KediOutbox::open(&path).unwrap();
        let pending = outbox2.pending_events(10).unwrap();
        assert_eq!(pending.len(), 3);

        // Acknowledge first 2 events
        let mut ack_set = HashSet::new();
        ack_set.insert("evt_offline_1".to_string());
        ack_set.insert("evt_offline_2".to_string());

        let acked = outbox2.acknowledge(&ack_set, 2000).unwrap();
        assert_eq!(acked, 2);

        // Only event 3 remains pending
        let remaining = outbox2.pending_events(10).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].event_id, "evt_offline_3");

        let _ = fs::remove_file(path);
    }
}

