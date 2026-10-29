// SPDX-License-Identifier: AGPL-3.0-or-later
//! A standalone coordinator process; no Window or Slint dependency.
use super::{engine::Engine, store::now, *};
use std::{
    fs::OpenOptions,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
pub fn run(store: CampaignStore, id: &str, engine: Engine) -> Result<(), String> {
    let directory = store.directory(id)?;
    let runner = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join("coordinator.lock"))
        .map_err(|e| e.to_string())?;
    if runner.try_lock().is_err() {
        return Ok(());
    }
    let record = store.read(id)?;
    engine.preflight(record.plan())?;
    // Nexora registers the browser worker through this adapter. Start/retain it
    // before claiming content jobs, rather than only upon reaching Media.
    let _media_runtime = if record.plan().providers.uses_grok() {
        let runtime = crate::production_runtime::BrowserRuntime::ensure()?;
        let browser = crate::nexora::NexoraSettings::load(&engine.settings.config_root)
            .and_then(|s|s.client()).map_err(|_|"Kết nối Nexora không hợp lệ")?;
        let deadline=Instant::now()+Duration::from_secs(65);
        loop {
            if browser.production_browser_ready(&record.plan().providers.grok_profile).unwrap_or(false){break;}
            if Instant::now()>=deadline{return Err("Cầu nối đã mở nhưng worker Grok chưa xác nhận tiện ích và đăng nhập; chưa gửi yêu cầu AI mới".into());}
            std::thread::sleep(Duration::from_secs(2));
        }
        Some(runtime)
    }else{None};
    let engine = Arc::new(engine);
    let stop = Arc::new(AtomicBool::new(false));
    let mut threads = vec![];
    // The store enforces the approved worker count on every claim. Fixed upper
    // bound lets a paused, reapproved plan change its limit without a new owner.
    for index in 0..8 {
        let store = store.clone();
        let id = id.to_owned();
        let engine = engine.clone();
        let stop = stop.clone();
        threads.push(std::thread::spawn(move || {
            let worker = format!("{}-{index}", std::process::id());
            while !stop.load(Ordering::Acquire) {
                let job = match store.claim(&id, &worker, now()) {
                    Ok(Some(job)) => job,
                    Ok(None) => {
                        std::thread::sleep(Duration::from_secs(2));
                        continue;
                    }
                    Err(_) => {
                        std::thread::sleep(Duration::from_secs(3));
                        continue;
                    }
                };
                let cancel = Arc::new(AtomicBool::new(false));
                let done = Arc::new(AtomicBool::new(false));
                let heartbeat = {
                    let (store, id, video, token, cancel, done) = (
                        store.clone(),
                        id.clone(),
                        job.id.clone(),
                        job.lease.as_ref().unwrap().token.clone(),
                        cancel.clone(),
                        done.clone(),
                    );
                    let seconds = job.lease.as_ref().unwrap().reserved_seconds;
                    std::thread::spawn(move || {
                        let start = Instant::now();
                        while !done.load(Ordering::Acquire) {
                            if store.heartbeat(&id, &video, &token, now()).ok() != Some(true)
                                || seconds > 0 && start.elapsed().as_secs() > seconds
                            {
                                cancel.store(true, Ordering::Release);
                            }
                            for _ in 0..20 {
                                if done.load(Ordering::Acquire) {
                                    return;
                                }
                                std::thread::sleep(Duration::from_millis(250));
                            }
                        }
                    })
                };
                // Resource locks are held THROUGH settlement. Concurrent content
                // tasks therefore compare against the latest committed scripts.
                let locks = store.root().join("resource-locks");
                let _ = std::fs::create_dir_all(&locks);
                let mut resource = None;
                let mut resource_failed = false;
                let resource_key = match job.stage {
                    Stage::Content => Some(format!("content-{id}")),
                    Stage::Media => store
                        .read(&id)
                        .ok()
                        .filter(|r| r.plan().providers.uses_grok())
                        .map(|r| {
                            format!(
                                "grok-{}",
                                crate::ai_control::digest(&serde_json::json!(
                                    r.plan().providers.grok_profile
                                ))
                            )
                        }),
                    Stage::Publish => job.page_id.as_ref().map(|p| format!("page-{p}")),
                    _ => None,
                };
                if let Some(key) = resource_key {
                    match OpenOptions::new()
                        .create(true)
                        .truncate(false)
                        .read(true)
                        .write(true)
                        .open(locks.join(format!("{key}.lock")))
                    {
                        Ok(file) => {
                            while file.try_lock().is_err() {
                                if cancel.load(Ordering::Acquire) {
                                    break;
                                }
                                std::thread::sleep(Duration::from_millis(250));
                            }
                            resource = Some(file);
                        }
                        Err(_) => resource_failed = true,
                    }
                }
                if job.stage == Stage::Render {
                    while resource.is_none() && !cancel.load(Ordering::Acquire) {
                        for slot in 0..2 {
                            if let Ok(file) = OpenOptions::new()
                                .create(true)
                                .truncate(false)
                                .read(true)
                                .write(true)
                                .open(locks.join(format!("render-{slot}.lock")))
                            {
                                if file.try_lock().is_ok() {
                                    resource = Some(file);
                                    break;
                                }
                            } else {
                                resource_failed = true;
                                break;
                            }
                        }
                        if resource_failed {
                            break;
                        }
                        if resource.is_none() {
                            std::thread::sleep(Duration::from_millis(250));
                        }
                    }
                }
                let started = Instant::now();
                let result = if resource_failed {
                    Err(super::store::TaskFailure::input(
                        "Không giữ được khóa tài nguyên; chưa thực thi",
                    ))
                } else if cancel.load(Ordering::Acquire) {
                    Err(super::store::TaskFailure::known(
                        "Đã tạm dừng trước khi thực thi",
                    ))
                } else {
                    store
                        .read(&id)
                        .map_err(super::store::TaskFailure::input)
                        .and_then(|record| engine.execute(&store, &record, &job, cancel.clone()))
                }
                .map_err(|mut failure| {
                    if job.stage == Stage::Render {
                        failure.render_seconds = started.elapsed().as_secs();
                    }
                    failure
                });
                let _ = store.settle(
                    &id,
                    &job.id,
                    &job.lease.as_ref().unwrap().token,
                    result,
                    now(),
                );
                drop(resource);
                done.store(true, Ordering::Release);
                let _ = heartbeat.join();
            }
        }));
    }
    let mut last_reconcile = Instant::now();
    let mut reconcile_cursor = 0usize;
    loop {
        let record = store.read(id)?;
        if matches!(
            record.state,
            CampaignState::Cancelled | CampaignState::Completed
        ) {
            stop.store(true, Ordering::Release);
            break;
        }
        if record.state == CampaignState::Running {
            // A lost finish reply is read-only recovery, never a second upload
            // or publish request. Spread polling across jobs and cap API traffic.
            if last_reconcile.elapsed() >= Duration::from_secs(30) {
                let waiting: Vec<_> = record
                    .jobs
                    .iter()
                    .filter(|j| {
                        j.stage == Stage::Publish
                            && j.state == JobState::Unknown
                            && j.lease.is_none()
                            && j.publish_dispatched
                            && j.upload_id.is_some()
                            && j.page_id.is_some()
                    })
                    .collect();
                if !waiting.is_empty() {
                    let job = waiting[reconcile_cursor % waiting.len()];
                    reconcile_cursor = reconcile_cursor.wrapping_add(1);
                    let upload = job.upload_id.as_deref().unwrap();
                    let api = super::facebook::Facebook::new(&engine.settings.config_root);
                    if let Ok(status) = api.status(job.page_id.as_deref().unwrap(), upload) {
                        if let Some(url) = super::facebook::Facebook::published(&status, upload) {
                            let _ = store.reconcile_published(
                                id,
                                &job.id,
                                job.revision,
                                upload,
                                &url,
                                now(),
                            );
                        }
                    }
                }
                last_reconcile = Instant::now();
            }
            if record
                .plan()
                .completion_deadline_utc
                .is_some_and(|deadline| now() > deadline)
            {
                let _ = store.control(id, "pause");
                let _ = store.transact(id, |r| {
                    r.audit(
                        "worker",
                        "deadline_reached",
                        None,
                        "Không tự kéo dài thời hạn đã duyệt",
                        now(),
                    );
                    Ok(())
                });
            }
            if record.plan().publish
                && !record
                    .jobs
                    .iter()
                    .any(|j| j.stage == Stage::Publish && j.lease.is_some())
                && record
                    .jobs
                    .iter()
                    .any(|j| j.stage == Stage::Publish && j.slot_utc.is_none() && j.lease.is_none())
            {
                if let Err(error) = store.allocate(id) {
                    let _ = store.transact(id, |r| {
                        r.state = CampaignState::Paused;
                        r.audit("scheduler", "schedule_blocked", None, error, now());
                        Ok(())
                    });
                }
            }
        }
        std::thread::sleep(Duration::from_secs(3));
    }
    for thread in threads {
        let _ = thread.join();
    }
    store.release_cancelled_slots(id)?;
    Ok(())
}
