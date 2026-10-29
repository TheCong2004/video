// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native Kedi operator controls and a single speech writer over existing A/V.
use crate::ui::{App, StudioLive};
use concat_studio::{
    kedi_live::{ControlAction, ControlRequest, KediLiveRuntime, Role, RuntimeConfig, utc_now},
    live_adapters::{CommerceGatewayClient, TikTokReadOnlyAdapter},
    live_brain::{LiveFeatureFlags, LiveSessionState, ProductSnapshot},
};
use slint::ComponentHandle;
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

type Shared = Arc<Mutex<KediLiveRuntime>>;
static RUNTIME: OnceLock<Shared> = OnceLock::new();
thread_local! {static SPEECH_GENERATION:Cell<Option<u64>>=const{Cell::new(None)};static SPEECH_MOTION:RefCell<Option<PathBuf>>=const{RefCell::new(None)};}
pub(crate) fn motion_source() -> Option<PathBuf> {
    SPEECH_MOTION.with(|m| m.borrow().clone())
}

fn load_scene() -> Result<concat_studio::live_scene::SceneController, String> {
    let file = std::env::var_os("KEDI_AVATAR_MANIFEST")
        .ok_or("Thiếu KEDI_AVATAR_MANIFEST có consent nhân vật/giọng")?;
    let root = std::env::var_os("KEDI_ASSET_ROOT").ok_or("Thiếu KEDI_ASSET_ROOT")?;
    let meta = std::fs::metadata(&file).map_err(|_| "Không đọc được avatar manifest")?;
    if meta.len() > 65536 {
        return Err("Avatar manifest quá lớn".into());
    }
    let bytes = std::fs::read(file).map_err(|_| "Không đọc được avatar manifest")?;
    let manifest: concat_studio::live_scene::AvatarManifest =
        serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes))
            .map_err(|_| "Avatar manifest không hợp lệ")?;
    let controller = concat_studio::live_scene::SceneController::load(
        std::path::Path::new(&root),
        manifest,
        &utc_now(),
    )
    .map_err(|e| format!("Avatar preflight: {e:?}"))?;
    for clip in &controller.manifest().motions.clips {
        crate::studio_live_director::validate_clip(&std::path::Path::new(&root).join(&clip.path))
            .map_err(str::to_string)?;
    }
    Ok(controller)
}

pub(crate) fn active() -> bool {
    RUNTIME
        .get()
        .and_then(|r| r.lock().ok())
        .is_some_and(|r| r.status().state == LiveSessionState::Live)
}

/// Called only by the existing validated connector monitor, never from chat text.
pub(crate) fn receive_frame(frame: &concat_studio::live::LiveFrame) {
    let Some(shared) = RUNTIME.get() else {
        return;
    };
    let Ok(mut r) = shared.lock() else {
        return;
    };
    if r.status().state != LiveSessionState::Live {
        return;
    }
    let Ok(adapter) = TikTokReadOnlyAdapter::new(&r.config.tenant_id, &r.config.store_id) else {
        return;
    };
    if let Ok(Some(mut event)) = adapter.normalize_comment(frame) {
        event.session_id = r.config.session_id.clone();
        let previous = r.status().generation;
        let _ = r.ingest(event, &utc_now(), monotonic_ms());
        if r.status().generation != previous {
            crate::studio_lipsync::cancel();
        }
    }
}

fn monotonic_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// Returns false when a Kedi speech generation has been cancelled. Existing speech
/// paths outside a Kedi turn are unaffected. STARTED is observed at the audio clock.
pub(crate) fn observe_audio(advanced: bool) -> bool {
    SPEECH_GENERATION.with(|slot| {
        let Some(generation) = slot.get() else {
            return true;
        };
        let Some(shared) = RUNTIME.get() else {
            return false;
        };
        let Ok(mut r) = shared.lock() else {
            return false;
        };
        if !r.generation_current(generation) {
            return false;
        }
        if advanced && r.status().speech_state == "SUBMITTED" {
            return r.playback_started(generation, &utc_now());
        }
        true
    })
}

fn apply(shared: &Shared, action: ControlAction) -> Result<(), String> {
    let mut r = shared
        .lock()
        .map_err(|_| "Runtime không khả dụng".to_string())?;
    if matches!(action, ControlAction::Start) && r.status().state == LiveSessionState::Stopped {
        r.renew_session().map_err(|e| format!("Kedi: {e:?}"))?;
    }
    let expected_revision = r.status().revision;
    r.execute(
        ControlRequest {
            command_id: format!("ui-{}-{}", monotonic_ms(), rand_id()),
            expected_revision,
            action,
        },
        Role::Operator,
        &utc_now(),
        monotonic_ms(),
    )
    .map_err(|e| format!("Kedi: {e:?}"))?;
    Ok(())
}
fn rand_id() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn start_commerce_worker(shared: Shared, weak: slint::Weak<App>) {
    let _ = std::thread::Builder::new()
        .name("kedi-live-commerce".into())
        .spawn(move || {
            let (Ok(origin), Ok(token), Ok(sku)) = (
                std::env::var("KEDI_COMMERCE_ORIGIN"),
                std::env::var("KEDI_COMMERCE_TOKEN"),
                std::env::var("KEDI_FEATURED_SKU"),
            ) else {
                return;
            };
            let Ok(client) = CommerceGatewayClient::new(&origin, &token, true) else {
                return;
            };
            let mut refresh = Instant::now() - Duration::from_secs(10);
            let mut failures = 0u8;
            let mut retry_at = Instant::now();
            loop {
                if weak.upgrade_in_event_loop(|_| {}).is_err() {
                    return;
                }
                if Instant::now() >= retry_at && refresh.elapsed() >= Duration::from_secs(3) {
                    refresh = Instant::now();
                    let scope = shared
                        .lock()
                        .ok()
                        .map(|r| (r.config.tenant_id.clone(), r.config.store_id.clone()));
                    if let Some((tenant, store)) = scope {
                        match client.snapshot(&tenant, &store, &sku) {
                            Ok(snapshot) => {
                                failures = 0;
                                let revision = snapshot.revision;
                                let needs_feature = if let Ok(mut r) = shared.lock() {
                                    let accepted = r.install_snapshot(snapshot).is_ok();
                                    accepted
                                        && r.status().featured_sku.as_deref() != Some(sku.as_str())
                                } else {
                                    false
                                };
                                if needs_feature {
                                    let _ = apply(
                                        &shared,
                                        ControlAction::Feature {
                                            sku: sku.clone(),
                                            revision,
                                        },
                                    );
                                }
                            }
                            Err(_) => {
                                failures = failures.saturating_add(1);
                                if failures >= 3 {
                                    retry_at = Instant::now() + Duration::from_secs(30);
                                    failures = 0;
                                }
                            }
                        }
                    }
                }
                // Best effort: a failed CRM write is dropped and never retried as a transaction.
                if Instant::now() >= retry_at {
                    let signal = shared.lock().ok().and_then(|mut r| r.pop_analytics());
                    if let Some(signal) = signal {
                        if client.publish_interest(&signal).is_err() {
                            retry_at = Instant::now() + Duration::from_secs(5);
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(150));
            }
        });
}

fn start_checkpoint_worker(shared: Shared, weak: slint::Weak<App>, path: PathBuf) {
    let _ = std::thread::Builder::new()
        .name("kedi-live-checkpoint".into())
        .spawn(move || {
            let mut previous = None;
            loop {
                if weak.upgrade_in_event_loop(|_| {}).is_err() {
                    return;
                }
                if let Some(status) = shared.lock().ok().map(|r| r.status()) {
                    let key = (
                        status.revision,
                        status.generation,
                        status.speech_state.clone(),
                        status.queued_tasks,
                    );
                    if previous.as_ref() != Some(&key) {
                        if KediLiveRuntime::save_status(&status, &path).is_ok() {
                            previous = Some(key);
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        });
}

fn render(weak: &slint::Weak<App>, shared: &Shared, error: Option<String>) {
    let Ok(r) = shared.lock() else {
        return;
    };
    let s = r.status();
    let dev = r.config.dev_mode;
    drop(r);
    let _ = weak.upgrade_in_event_loop(move |app| {
        let ui = app.global::<StudioLive>();
        ui.set_kedi_state(format!("{:?}", s.state).into());
        ui.set_kedi_automatic(s.automatic);
        ui.set_kedi_status(
            error
                .unwrap_or_else(|| {
                    format!(
                        "{} · {} tác vụ · {} ms chờ · {} lỗi",
                        if dev {
                            "Mô phỏng offline"
                        } else {
                            "TTS/Avatar local"
                        },
                        s.queued_tasks,
                        s.queued_audio_ms,
                        s.failed_tasks
                    )
                })
                .into(),
        );
        ui.set_kedi_reply(s.last_safe_reply.unwrap_or_default().into());
    });
}

pub(crate) fn bind(app: &App, legacy_auto: Arc<AtomicBool>) {
    let flags = LiveFeatureFlags::from_env();
    app.global::<StudioLive>()
        .set_kedi_available(flags.live_enabled);
    if !flags.live_enabled {
        return;
    }
    let mut config = RuntimeConfig::demo();
    config.dev_mode = !flags.opentalking_enabled;
    if let Ok(v) = std::env::var("KEDI_TENANT_ID") {
        config.tenant_id = v;
    }
    if let Ok(v) = std::env::var("KEDI_STORE_ID") {
        config.store_id = v;
    }
    let state_path = concat_host::AppDirs::locate().ok().and_then(|dirs| {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (&config.tenant_id, &config.store_id).hash(&mut h);
        let folder = dirs.data.join("kedi-live");
        std::fs::create_dir_all(&folder).ok()?;
        Some(folder.join(format!("session-{:016x}.json", h.finish())))
    });
    let saved = state_path.as_ref().and_then(|p| {
        if std::fs::metadata(p).ok()?.len() > 65536 {
            return None;
        }
        serde_json::from_slice::<concat_studio::kedi_live::RuntimeStatus>(&std::fs::read(p).ok()?)
            .ok()
    });
    let restore = saved.as_ref().is_some_and(|s| {
        s.tenant_id == config.tenant_id
            && s.store_id == config.store_id
            && s.state != LiveSessionState::Stopped
    });
    if restore {
        config.session_id = saved.unwrap().session_id;
    }
    let created = if restore {
        KediLiveRuntime::restore(config.clone(), flags, state_path.as_ref().unwrap())
    } else {
        KediLiveRuntime::new(config, flags)
    };
    let Ok(runtime) = created else {
        app.global::<StudioLive>()
            .set_kedi_status("Cấu hình tenant/store không hợp lệ".into());
        return;
    };
    let shared = Arc::new(Mutex::new(runtime));
    if RUNTIME.set(shared.clone()).is_err() {
        return;
    }
    if let Some(path) = state_path {
        start_checkpoint_worker(shared.clone(), app.as_weak(), path);
    }
    let playback_slot = Arc::new(Mutex::new(None));
    let selected_playback = playback_slot.clone();
    let ready = Arc::new(AtomicBool::new(!flags.opentalking_enabled));
    let start_ready = ready.clone();
    let weak = app.as_weak();
    let controls = shared.clone();
    app.global::<StudioLive>().on_kedi_control(move |command| {
        let Some(app) = weak.upgrade() else {
            return;
        };
        let action = match command.as_str() {
            "start" => {
                if !start_ready.load(Ordering::Acquire) {
                    app.global::<StudioLive>()
                        .set_kedi_status("Chưa qua preflight avatar/consent/codec".into());
                    return;
                }
                crate::host::Shell::with(|shell, _| {
                    if let Ok(mut slot) = selected_playback.lock() {
                        *slot = Some(Arc::clone(&shell.studio.borrow().host.playback));
                    }
                });
                legacy_auto.store(false, Ordering::Release);
                app.global::<StudioLive>().set_auto_reply(false);
                ControlAction::Start
            }
            "pause" => ControlAction::Pause,
            "resume" => ControlAction::Resume,
            "takeover" => ControlAction::Takeover,
            "introduce" => ControlAction::IntroduceProduct,
            "stop" => ControlAction::Stop,
            _ => return,
        };
        if matches!(
            action,
            ControlAction::Stop | ControlAction::Pause | ControlAction::Takeover
        ) {
            crate::studio_lipsync::cancel();
        }
        let error = apply(&controls, action).err();
        render(&weak, &controls, error);
    });
    let weak = app.as_weak();
    let controls = shared.clone();
    app.global::<StudioLive>().on_kedi_simulate(move |text| {
        let error = apply(&controls, ControlAction::Simulate { text: text.into() }).err();
        render(&weak, &controls, error);
    });
    let weak = app.as_weak();
    let controls = shared.clone();
    app.global::<StudioLive>().on_kedi_speak(move |text| {
        crate::studio_lipsync::cancel();
        let error = apply(&controls, ControlAction::Speak { text: text.into() }).err();
        render(&weak, &controls, error);
    });

    let weak = app.as_weak();
    if flags.commerce_enabled {
        start_commerce_worker(shared.clone(), weak.clone());
    }
    let spawned = std::thread::Builder::new()
        .name("kedi-live-owner".into())
        .spawn(move || {
            let (mut scenes, preflight_error) = if flags.opentalking_enabled {
                match load_scene() {
                    Ok(c) => {
                        ready.store(true, Ordering::Release);
                        (Some(c), None)
                    }
                    Err(e) => (None, Some(e)),
                }
            } else {
                (None, None)
            };
            // Demo prices are used only in the explicit offline profile.
            if let Ok(mut r) = shared.lock() {
                if r.config.dev_mode && !flags.commerce_enabled {
                    let tenant = r.config.tenant_id.clone();
                    let store = r.config.store_id.clone();
                    let _ = r.install_snapshot(ProductSnapshot {
                        schema_version: 1,
                        tenant_id: tenant,
                        store_id: store,
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
                    });
                    let revision = r.status().revision;
                    let _ = r.execute(
                        ControlRequest {
                            command_id: "demo-feature".into(),
                            expected_revision: revision,
                            action: ControlAction::Feature {
                                sku: "DEMO-01".into(),
                                revision: 1,
                            },
                        },
                        Role::Operator,
                        &utc_now(),
                        monotonic_ms(),
                    );
                }
            }
            loop {
                if weak.upgrade_in_event_loop(|_| {}).is_err() {
                    break;
                }
                let task = shared
                    .lock()
                    .ok()
                    .and_then(|mut r| r.dispatch(&utc_now(), monotonic_ms()));
                if let Some(task) = task {
                    if flags.opentalking_enabled {
                        if let Some(controller) = scenes.as_mut() {
                            if !controller.rights_current(&utc_now()) {
                                if let Ok(mut r) = shared.lock() {
                                    r.playback_finished(task.generation_id, false);
                                }
                                continue;
                            }
                            let idle = controller
                                .manifest()
                                .motions
                                .clips
                                .iter()
                                .find(|c| c.gesture == "idle" && c.loopable)
                                .map(|c| c.clip_id.clone());
                            match controller.prepare(
                                &task,
                                if flags.motion_enabled {
                                    None
                                } else {
                                    idle.as_deref()
                                },
                            ) {
                                Ok(plan) => {
                                    SPEECH_MOTION.with(|m| *m.borrow_mut() = Some(plan.path));
                                }
                                Err(_) => {
                                    if let Ok(mut r) = shared.lock() {
                                        r.playback_finished(task.generation_id, false);
                                    }
                                    continue;
                                }
                            }
                        }
                        // Synthesis and GPU work execute without holding the controller mutex.
                        SPEECH_GENERATION.with(|slot| slot.set(Some(task.generation_id)));
                        let result = (|| {
                            let playback = playback_slot
                                .lock()
                                .ok()
                                .and_then(|p| p.clone())
                                .ok_or("Playback chưa sẵn sàng")?;
                            crate::studio_live::synthesize_and_play(
                                &playback,
                                &task.speech.text,
                                &std::env::var("KEDI_VOICE_ID")
                                    .unwrap_or_else(|_| "edge:vi-VN-HoaiMyNeural".into()),
                                &weak,
                            )
                        })();
                        SPEECH_GENERATION.with(|slot| slot.set(None));
                        SPEECH_MOTION.with(|m| *m.borrow_mut() = None);
                        if let Ok(mut r) = shared.lock() {
                            r.playback_finished(task.generation_id, result.is_ok());
                        }
                    } else if let Ok(mut r) = shared.lock() {
                        if r.playback_started(task.generation_id, &utc_now()) {
                            r.playback_finished(task.generation_id, true);
                        }
                    }
                }
                render(&weak, &shared, preflight_error.clone());
                std::thread::sleep(Duration::from_millis(80));
            }
        });
    if spawned.is_err() {
        app.global::<StudioLive>()
            .set_kedi_status("Không tạo được Kedi worker".into());
    }
}
