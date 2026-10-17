// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native session-owned OmniRoute supervisor. Never sends Promax tokens locally.
use concat_host::studio_workflows::{WorkflowClient, WorkflowError};
use concat_studio::local_ai::LocalAiRuntime;
use sha2::{Digest, Sha256};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;

#[derive(Clone)]
struct Owner {
    generation: u64,
    profile: String,
    cancel: Arc<AtomicBool>,
}
static NEXT: AtomicU64 = AtomicU64::new(1);
static OWNER: Mutex<Option<Owner>> = Mutex::new(None);
static RUNTIME: Mutex<Option<(u64, LocalAiRuntime)>> = Mutex::new(None);

pub(crate) fn generation() -> Option<u64> {
    OWNER
        .lock()
        .ok()
        .and_then(|owner| owner.as_ref().map(|owner| owner.generation))
}

pub(crate) fn origin() -> Option<String> {
    RUNTIME
        .lock()
        .ok()
        .and_then(|slot| slot.as_ref().map(|(_, r)| r.origin().to_string()))
}

pub(crate) fn set_owner(identity: &str) {
    stop();
    let profile = format!("{:x}", Sha256::digest(identity.as_bytes()));
    if let Ok(mut owner) = OWNER.lock() {
        *owner = Some(Owner {
            generation: NEXT.fetch_add(1, Ordering::Relaxed),
            profile,
            cancel: Arc::new(AtomicBool::new(false)),
        });
    }
}
pub(crate) fn stop() {
    let previous = OWNER.lock().ok().and_then(|mut owner| owner.take());
    if let Some(owner) = previous {
        owner.cancel.store(true, Ordering::Release);
        let _ = std::thread::Builder::new()
            .name("local-ai-stop".into())
            .spawn(move || {
                if let Ok(mut runtime) = RUNTIME.lock() {
                    if runtime
                        .as_ref()
                        .is_some_and(|(generation, _)| *generation == owner.generation)
                    {
                        runtime.take();
                    }
                }
            });
    }
}
pub(crate) fn shutdown() {
    stop();
    if let Ok(mut runtime) = RUNTIME.lock() {
        runtime.take();
    }
}
pub(crate) fn client() -> Result<WorkflowClient, WorkflowError> {
    let owner = OWNER
        .lock()
        .map_err(|_| WorkflowError::Unavailable)?
        .clone()
        .ok_or(WorkflowError::Authentication)?;
    let mut slot = RUNTIME.lock().map_err(|_| WorkflowError::Unavailable)?;
    if owner.cancel.load(Ordering::Acquire) {
        return Err(WorkflowError::Authentication);
    }
    if slot
        .as_ref()
        .is_some_and(|(generation, _)| *generation != owner.generation)
    {
        slot.take();
    }
    // Recover a terminated owned child before a new action. Never replay an
    // action whose HTTP outcome is unknown.
    if let Some((_, runtime)) = slot.as_mut() {
        match runtime.client() {
            Ok(client) => return Ok(client),
            Err(_) => {
                slot.take();
            }
        }
    }
    if slot.is_none() {
        let dirs = concat_host::AppDirs::locate().map_err(|_| WorkflowError::Configuration)?;
        let resources = LocalAiRuntime::resources()?;
        let runtime = LocalAiRuntime::start(
            &resources,
            &dirs.data.join("local-ai").join(&owner.profile),
            &owner.cancel,
            Duration::from_secs(180),
        )?;
        if owner.cancel.load(Ordering::Acquire) {
            return Err(WorkflowError::Authentication);
        }
        *slot = Some((owner.generation, runtime));
    }
    slot.as_mut().ok_or(WorkflowError::Unavailable)?.1.client()
}
