// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat contributors

use concat_project::{Checkpoint, DraftError, Editor};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Entity that initiated or authorized an edit or operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EditActor {
    /// Human user in the GUI.
    User,
    /// Automated or interactive agent connected through an identified connection.
    Agent {
        /// Connection identifier for the agent.
        connection: String,
    },
}

/// Metadata describing an immutable timeline checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointInfo {
    /// Monotonic checkpoint identifier.
    pub id: u64,
    /// Human-readable label for the checkpoint.
    pub label: String,
    /// Actor that created the checkpoint.
    pub actor: EditActor,
    /// ISO timestamp of creation.
    pub created_at: String,
}

/// Audit log record of an edit, checkpoint or lease operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEntry {
    /// Actor performing the operation.
    pub actor: EditActor,
    /// Operation name (e.g. checkpoint, approvedBatch).
    pub operation: String,
    /// Additional details or idempotency key.
    pub details: Option<String>,
    /// ISO timestamp of execution.
    pub timestamp: String,
}

/// Active work session for a connected agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkSessionInfo {
    /// Unique work session ID.
    pub id: String,
    /// Agent connection ID owning this session.
    pub connection_id: String,
    /// Descriptive task title.
    pub title: String,
    /// ISO timestamp when work began.
    pub started_at: String,
}

/// Exclusive write lease held by an agent for a bounded duration.
#[derive(Clone)]
pub struct AgentLease {
    pub(crate) authority: Arc<()>,
    pub(crate) connection_id: String,
    pub(crate) expires_at: Instant,
}

struct StoredCheckpoint {
    info: CheckpointInfo,
    snapshot: Checkpoint,
}

struct ActiveLease {
    connection_id: String,
    expires_at: Instant,
    expired: bool,
}

pub struct SessionSafety {
    pub(crate) authority: Arc<()>,
    checkpoints: Vec<StoredCheckpoint>,
    next_checkpoint_id: u64,
    work_session: Option<WorkSessionInfo>,
    history_locks: HashMap<String, String>,
    active_lease: Option<ActiveLease>,
    committed_ops: HashSet<String>,
    activities: Vec<ActivityEntry>,
}

fn now_iso() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03}Z", now.as_secs(), now.subsec_millis())
}

impl SessionSafety {
    pub fn new() -> Self {
        Self {
            authority: Arc::new(()),
            checkpoints: Vec::new(),
            next_checkpoint_id: 1,
            work_session: None,
            history_locks: HashMap::new(),
            active_lease: None,
            committed_ops: HashSet::new(),
            activities: Vec::new(),
        }
    }

    pub fn create_checkpoint(
        &mut self,
        editor: &Editor,
        label: &str,
        actor: EditActor,
    ) -> Result<CheckpointInfo, String> {
        let id = self.next_checkpoint_id;
        self.next_checkpoint_id += 1;
        let info = CheckpointInfo {
            id,
            label: label.to_owned(),
            actor: actor.clone(),
            created_at: now_iso(),
        };
        let snapshot = editor.checkpoint();
        self.checkpoints.push(StoredCheckpoint {
            info: info.clone(),
            snapshot,
        });
        self.activities.push(ActivityEntry {
            actor,
            operation: "checkpoint".into(),
            details: Some(label.to_owned()),
            timestamp: now_iso(),
        });
        Ok(info)
    }

    pub fn restore_named_checkpoint(
        &mut self,
        editor: &mut Editor,
        id: u64,
        actor: EditActor,
    ) -> Result<bool, String> {
        if self.is_history_locked() {
            return Err("History is locked".into());
        }
        let stored = self
            .checkpoints
            .iter()
            .find(|c| c.info.id == id)
            .ok_or_else(|| format!("Checkpoint {id} not found"))?;
        let restored = editor
            .restore_checkpoint(&stored.snapshot)
            .map_err(|e| e.to_string())?;
        self.activities.push(ActivityEntry {
            actor,
            operation: "restoreCheckpoint".into(),
            details: Some(format!("checkpoint {id}")),
            timestamp: now_iso(),
        });
        Ok(restored)
    }

    pub fn delete_checkpoint(&mut self, id: u64) -> Result<(), String> {
        let initial_len = self.checkpoints.len();
        self.checkpoints.retain(|c| c.info.id != id);
        if self.checkpoints.len() == initial_len {
            return Err(format!("Checkpoint {id} not found"));
        }
        Ok(())
    }

    pub fn checkpoints(&self) -> Vec<CheckpointInfo> {
        self.checkpoints.iter().map(|c| c.info.clone()).collect()
    }

    pub fn begin_agent_work(
        &mut self,
        connection: &str,
        title: &str,
    ) -> Result<WorkSessionInfo, String> {
        if let Some(existing) = &self.work_session {
            if existing.connection_id == connection {
                return Ok(existing.clone());
            } else {
                return Err(format!(
                    "Another agent session is active: {}",
                    existing.connection_id
                ));
            }
        }
        let session = WorkSessionInfo {
            id: format!("ws-{}", self.activities.len() + 1),
            connection_id: connection.to_owned(),
            title: title.to_owned(),
            started_at: now_iso(),
        };
        self.work_session = Some(session.clone());
        Ok(session)
    }

    pub fn end_agent_work(&mut self, connection: &str) -> Result<(), String> {
        if let Some(existing) = &self.work_session {
            if existing.connection_id == connection {
                self.work_session = None;
            }
        }
        Ok(())
    }

    pub fn lock_history(&mut self, connection: &str, reason: &str) -> Result<(), String> {
        self.history_locks
            .insert(connection.to_owned(), reason.to_owned());
        Ok(())
    }

    pub fn is_history_locked(&self) -> bool {
        !self.history_locks.is_empty()
    }

    pub fn disconnect_agent(&mut self, connection: &str) {
        self.history_locks.remove(connection);
        if let Some(ws) = &self.work_session {
            if ws.connection_id == connection {
                self.work_session = None;
            }
        }
        if let Some(lease) = &self.active_lease {
            if lease.connection_id == connection {
                self.active_lease = None;
            }
        }
    }

    pub fn acquire_agent_write(
        &mut self,
        connection: &str,
        duration: Duration,
    ) -> Result<AgentLease, String> {
        if duration.is_zero() {
            return Err("Lease duration must be greater than zero".into());
        }
        if let Some(active) = &self.active_lease {
            if !active.expired && Instant::now() < active.expires_at {
                if active.connection_id != connection {
                    return Err(format!(
                        "Write lease currently held by connection {}",
                        active.connection_id
                    ));
                }
            }
        }
        let expires_at = Instant::now() + duration;
        self.active_lease = Some(ActiveLease {
            connection_id: connection.to_owned(),
            expires_at,
            expired: false,
        });
        Ok(AgentLease {
            authority: self.authority.clone(),
            connection_id: connection.to_owned(),
            expires_at,
        })
    }

    pub fn is_write_locked_for_user(&self) -> bool {
        !self.can_actor_edit(&EditActor::User)
    }

    pub fn can_actor_edit(&self, actor: &EditActor) -> bool {
        if self.is_history_locked() {
            return false;
        }
        if let Some(active) = &self.active_lease {
            if !active.expired && Instant::now() < active.expires_at {
                return match actor {
                    EditActor::Agent { connection } => connection == &active.connection_id,
                    EditActor::User => false,
                };
            }
        }
        true
    }

    pub fn active_lease_connection(&self) -> Option<String> {
        if let Some(active) = &self.active_lease {
            if !active.expired && Instant::now() < active.expires_at {
                return Some(active.connection_id.clone());
            }
        }
        None
    }

    pub fn expire_agent_write(&mut self) {
        if let Some(lease) = &mut self.active_lease {
            lease.expired = true;
        }
    }

    pub fn validate_commit(&mut self, lease: &AgentLease, op_id: &str) -> Result<(), DraftError> {
        if !Arc::ptr_eq(&self.authority, &lease.authority) {
            return Err(DraftError::Stale);
        }
        if Instant::now() >= lease.expires_at {
            return Err(DraftError::Stale);
        }
        if let Some(active) = &self.active_lease {
            if active.expired
                || active.connection_id != lease.connection_id
                || Instant::now() >= active.expires_at
            {
                return Err(DraftError::Stale);
            }
        } else {
            return Err(DraftError::Stale);
        }
        if self.committed_ops.contains(op_id) {
            return Err(DraftError::Stale);
        }
        Ok(())
    }

    pub fn record_agent_commit(&mut self, lease: &AgentLease, op_id: &str) {
        self.committed_ops.insert(op_id.to_owned());
        self.activities.push(ActivityEntry {
            actor: EditActor::Agent {
                connection: lease.connection_id.clone(),
            },
            operation: "approvedBatch".into(),
            details: Some(op_id.to_owned()),
            timestamp: now_iso(),
        });
    }

    pub fn activities(&self) -> &[ActivityEntry] {
        &self.activities
    }
}
