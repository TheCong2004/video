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
static INITIALIZING: AtomicBool = AtomicBool::new(false);
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
    .map_err(|e| -> String { match e {
        concat_studio::kedi_live::ControlError::Policy => "Thiếu snapshot sản phẩm còn hiệu lực hoặc lời đọc chưa qua kiểm tra dữ liệu. Kiểm tra mục Sản phẩm.".into(),
        concat_studio::kedi_live::ControlError::State => "Thao tác không phù hợp trạng thái phiên. Kiểm tra Bắt đầu / Tạm dừng / Dừng.".into(),
        concat_studio::kedi_live::ControlError::Input => "Nội dung trống, quá dài hoặc sai định dạng.".into(),
        concat_studio::kedi_live::ControlError::Disabled => "Runtime chưa được bật. Chọn cách chạy ở mục Phiên.".into(),
        concat_studio::kedi_live::ControlError::Unauthorized => "Thao tác không được phép trong chế độ hiện tại.".into(),
        concat_studio::kedi_live::ControlError::Conflict => "Phiên hoặc phiên bản sản phẩm đã thay đổi. Kiểm tra lại trạng thái.".into(),
        concat_studio::kedi_live::ControlError::Unavailable => "Dịch vụ hoặc đầu ra chưa sẵn sàng. Kiểm tra mục Sức khỏe.".into(),
    }})?;
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
    let product = r.featured_snapshot(&utc_now()).map(|p| format!(
        "{} · SKU {} · phiên bản {}\nNguồn snapshot: {}\nGiá: {} · Tồn kho: {}\nCó hiệu lực tới {}",
        p.title, p.sku, p.revision, p.verification_source,
        p.price_vnd.map(|v| format!("{v} VND")).unwrap_or_else(|| "Chưa xác nhận".into()),
        p.stock_state, p.valid_until,
    )).unwrap_or_else(|| "Chưa có snapshot còn hiệu lực. AI sẽ không tự xác nhận giá hoặc tồn kho.".into());
    let health = format!(
        "Phiên: {}\nCửa hàng: {} / {}\nHàng đợi: {} tác vụ · {} ms lời đọc\nGiọng: {} · Cảnh: {}\nLỗi tác vụ: {} · Sự kiện loại bỏ: {}\nP95 lập kế hoạch: {}\nAnalytics chờ: {}\nSKU nổi bật: {} · SKU đã ghim: {}\nFPS / độ lệch hình–tiếng: chưa đo\nPhát public / ghim / gửi bình luận: chưa có transport được kiểm chứng",
        s.session_id,
        s.tenant_id,
        s.store_id,
        s.queued_tasks,
        s.queued_audio_ms,
        s.speech_state,
        s.scene,
        s.failed_tasks,
        s.dropped_events,
        s.p95_plan_ms
            .map(|v| format!("{v} ms"))
            .unwrap_or_else(|| "Chưa có mẫu".into()),
        s.analytics_pending,
        s.featured_sku.as_deref().unwrap_or("Chưa chọn"),
        s.confirmed_pinned_sku.as_deref().unwrap_or("Chưa xác nhận"),
    );
    drop(r);
    let _ = weak.upgrade_in_event_loop(move |app| {
        let ui = app.global::<StudioLive>();
        ui.set_kedi_state(format!("{:?}", s.state).into());
        ui.set_kedi_automatic(s.automatic);
        ui.set_kedi_mode(if dev { "simulation" } else { "local" }.into());
        ui.set_kedi_product(product.into());
        ui.set_kedi_health(health.into());
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
    // Use the existing lip-sync connection owner; process secrets are never
    // copied to logs, checkpoint files or model prompts.
    if let Ok(origin)=std::env::var("KEDI_LIPSYNC_URL"){app.global::<StudioLive>().set_lipsync_url(origin.into());}
    if let Ok(token)=std::env::var("KEDI_LIPSYNC_TOKEN"){app.global::<StudioLive>().set_lipsync_token(token.into());}
    let weak = app.as_weak();
    let enable_auto = legacy_auto.clone();
    app.global::<StudioLive>().on_kedi_enable(move |real_speech| {
        let Some(app)=weak.upgrade() else {return};
        if RUNTIME.get().is_some() {return;}
        let mut flags=LiveFeatureFlags::from_env();
        flags.live_enabled=true;
        flags.opentalking_enabled=real_speech;
        // UI activation never grants platform-write permission.
        flags.platform_write_enabled=false;
        if !real_speech {flags.commerce_enabled=false;flags.motion_enabled=false;}
        if real_speech && ["KEDI_TENANT_ID","KEDI_STORE_ID","KEDI_AVATAR_MANIFEST","KEDI_ASSET_ROOT"].iter().any(|key|std::env::var(key).unwrap_or_default().trim().is_empty()) {
            app.global::<StudioLive>().set_kedi_status("Thiếu cấu hình cửa hàng hoặc avatar manifest. Xem mục Sức khỏe; chưa bật lời đọc thật.".into());
            return;
        }
        initialize(&app, enable_auto.clone(), flags);
    });
    let flags = LiveFeatureFlags::from_env();
    if !flags.live_enabled {
        return;
    }
    initialize(app, legacy_auto, flags);
}

fn initialize(app: &App, legacy_auto: Arc<AtomicBool>, flags: LiveFeatureFlags) {
    if RUNTIME.get().is_some()
        || INITIALIZING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
    {
        return;
    }
    app.global::<StudioLive>()
        .set_kedi_status("Đang chuẩn bị runtime và kiểm tra phiên đã lưu…".into());
    let weak = app.as_weak();
    let spawned = std::thread::Builder::new()
        .name("kedi-live-initialize".into())
        .spawn(move || {
            let result = prepare_runtime(flags);
            let _ = weak.upgrade_in_event_loop(move |app| {
                match result {
                    Ok((runtime, path)) => {
                        finish_initialize(&app, legacy_auto, flags, runtime, path)
                    }
                    Err(message) => app.global::<StudioLive>().set_kedi_status(message.into()),
                }
                INITIALIZING.store(false, Ordering::Release);
            });
        });
    if spawned.is_err() {
        INITIALIZING.store(false, Ordering::Release);
        app.global::<StudioLive>()
            .set_kedi_status("Không mở được tác vụ chuẩn bị phiên".into());
    }
}

// Checkpoint files are read on the preparation worker, never in a UI callback.
fn prepare_runtime(flags: LiveFeatureFlags) -> Result<(KediLiveRuntime, Option<PathBuf>), String> {
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
    let runtime =
        created.map_err(|_| "Cấu hình tenant/store hoặc checkpoint không hợp lệ".to_string())?;
    Ok((runtime, state_path))
}

fn finish_initialize(
    app: &App,
    legacy_auto: Arc<AtomicBool>,
    flags: LiveFeatureFlags,
    runtime: KediLiveRuntime,
    state_path: Option<PathBuf>,
) {
    let shared = Arc::new(Mutex::new(runtime));
    if RUNTIME.set(shared.clone()).is_err() {
        return;
    }
    app.global::<StudioLive>().set_kedi_available(true);
    app.global::<StudioLive>().set_kedi_mode(
        if flags.opentalking_enabled {
            "local"
        } else {
            "simulation"
        }
        .into(),
    );
    if flags.opentalking_enabled {
        app.global::<StudioLive>().invoke_ensure_lipsync();
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
        app.global::<StudioLive>().set_kedi_action_error("".into());
        let action = match command.as_str() {
            "start" => {
                if flags.opentalking_enabled && (!app.global::<StudioLive>().get_lipsync_enabled() || crate::studio_live_video::selected_motion().is_none()) {
                    app.global::<StudioLive>().set_kedi_action_error("Chọn và xem chuyển động nhân vật, rồi kiểm tra khớp môi ở mục Âm thanh / Đầu ra trước khi bắt đầu lời đọc thật.".into());
                    return;
                }
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
                crate::studio_lipsync::cancel();
                crate::host::Shell::with(|shell,_|shell.studio.borrow().host.playback.stop_speech());
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
            // Stop only the speech clock; the existing idle/video playout stays on.
            crate::host::Shell::with(|shell,_|shell.studio.borrow().host.playback.stop_speech());
            app.global::<StudioLive>().set_is_speaking(false);
        }
        let error = apply(&controls, action).err();
        if let Some(ref message) = error {
            app.global::<StudioLive>()
                .set_kedi_action_error(message.clone().into());
        }
        render(&weak, &controls, error);
    });
    let weak = app.as_weak();
    let controls = shared.clone();
    app.global::<StudioLive>().on_kedi_simulate(move |text| {
        let error = apply(&controls, ControlAction::Simulate { text: text.into() }).err();
        if let Some(app) = weak.upgrade() {
            app.global::<StudioLive>()
                .set_kedi_action_error(error.clone().unwrap_or_default().into());
        }
        render(&weak, &controls, error);
    });
    let weak = app.as_weak();
    let controls = shared.clone();
    app.global::<StudioLive>().on_kedi_speak(move |text| {
        crate::studio_lipsync::cancel();
        let error = apply(&controls, ControlAction::Speak { text: text.into() }).err();
        if let Some(app) = weak.upgrade() {
            app.global::<StudioLive>()
                .set_kedi_action_error(error.clone().unwrap_or_default().into());
        }
        render(&weak, &controls, error);
    });

    let weak = app.as_weak();
    let products = shared.clone();
    app.global::<StudioLive>().on_kedi_import_product(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.global::<StudioLive>().get_kedi_product_busy() {
            return;
        }
        app.global::<StudioLive>().set_kedi_product_busy(true);
        let revision = products.lock().ok().map(|r| r.status().revision);
        let shared = products.clone();
        let weak = weak.clone();
        if let Some(app) = weak.upgrade() {
            app.global::<StudioLive>().set_kedi_action_error("".into());
        }
        let spawned = std::thread::Builder::new()
            .name("kedi-product-import".into())
            .spawn(move || {
                let Some(path) = rfd::FileDialog::new()
                    .add_filter("Product snapshot", &["json"])
                    .pick_file()
                else {
                    let _ = weak.upgrade_in_event_loop(|app| {
                        app.global::<StudioLive>().set_kedi_product_busy(false)
                    });
                    return;
                };
                let result = (|| -> Result<(), String> {
                    if std::fs::metadata(&path)
                        .map_err(|_| "Không đọc được snapshot")?
                        .len()
                        > 65536
                    {
                        return Err("Snapshot vượt giới hạn 64 KB".into());
                    }
                    use std::io::Read;
                    let mut bytes = Vec::new();
                    std::fs::File::open(path)
                        .map_err(|_| "Không đọc được snapshot")?
                        .take(65537)
                        .read_to_end(&mut bytes)
                        .map_err(|_| "Không đọc được snapshot")?;
                    if bytes.len() > 65536 {
                        return Err("Snapshot vượt giới hạn 64 KB".into());
                    }
                    let snapshot: ProductSnapshot = serde_json::from_slice(
                        bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes),
                    )
                    .map_err(|_| "Snapshot JSON không đúng hợp đồng sản phẩm")?;
                    let sku = snapshot.sku.clone();
                    let product_revision = snapshot.revision;
                    let mut r = shared.lock().map_err(|_| "Runtime không khả dụng")?;
                    if Some(r.status().revision) != revision {
                        return Err(
                            "Phiên đã thay đổi trong lúc chọn tệp. Chọn lại snapshot.".into()
                        );
                    }
                    r.install_snapshot(snapshot)
                        .map_err(|e| format!("Không nhận snapshot: {e:?}"))?;
                    let expected_revision = r.status().revision;
                    r.execute(
                        ControlRequest {
                            command_id: format!("import-{}", rand_id()),
                            expected_revision,
                            action: ControlAction::Feature {
                                sku,
                                revision: product_revision,
                            },
                        },
                        Role::Operator,
                        &utc_now(),
                        monotonic_ms(),
                    )
                    .map_err(|e| format!("Không chọn được sản phẩm: {e:?}"))?;
                    Ok(())
                })();
                let message = result.err().unwrap_or_default();
                let _ = weak.upgrade_in_event_loop(move |app| {
                    app.global::<StudioLive>().set_kedi_product_busy(false);
                    app.global::<StudioLive>()
                        .set_kedi_action_error(message.into());
                });
                render(&weak, &shared, None);
            });
        if spawned.is_err() {
            app.global::<StudioLive>().set_kedi_product_busy(false);
            app.global::<StudioLive>()
                .set_kedi_action_error("Không mở được tác vụ nhập snapshot".into());
        }
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
                let ready_now = ready.load(Ordering::Acquire);
                let _ = weak.upgrade_in_event_loop(move |app| {
                    app.global::<StudioLive>().set_kedi_ready(ready_now)
                });
                render(&weak, &shared, preflight_error.clone());
                std::thread::sleep(Duration::from_millis(80));
            }
        });
    if spawned.is_err() {
        app.global::<StudioLive>()
            .set_kedi_status("Không tạo được Kedi worker".into());
    }
}

/// Runs real native callbacks against the isolated local runtime; never a platform.
#[cfg(debug_assertions)]
pub(crate) fn smoke_operator(app: &App) -> Result<(), String> {
    use std::{cell::RefCell, rc::Rc};
    app.global::<StudioLive>().invoke_kedi_enable(false);
    let result = Rc::new(RefCell::new(None));
    let completed = result.clone();
    let weak = app.as_weak();
    let deadline = Instant::now() + Duration::from_secs(15);
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(20),
        move || {
            if completed.borrow().is_some() {
                return;
            }
            if RUNTIME.get().is_some() {
                let outcome = weak
                    .upgrade()
                    .ok_or_else(|| "Test window disappeared".to_string())
                    .and_then(|app| smoke_operator_actions(&app));
                *completed.borrow_mut() = Some(outcome);
                let _ = slint::quit_event_loop();
            } else if Instant::now() >= deadline {
                *completed.borrow_mut() =
                    Some(Err("Local runtime initialization timed out".into()));
                let _ = slint::quit_event_loop();
            }
        },
    );
    slint::run_event_loop().map_err(|e| e.to_string())?;
    timer.stop();
    let output = result
        .borrow_mut()
        .take()
        .unwrap_or_else(|| Err("Operator smoke ended before initialization".into()));
    output
}

#[cfg(debug_assertions)]
fn smoke_operator_actions(app: &App) -> Result<(), String> {
    let ui = app.global::<StudioLive>();
    let shared = RUNTIME
        .get()
        .ok_or("Local runtime was not created by the UI callback")?;
    if !shared.lock().map_err(|_| "runtime lock")?.config.dev_mode {
        return Err("Operator smoke requires simulation mode".into());
    }
    ui.invoke_kedi_control("start".into());
    if shared.lock().map_err(|_| "runtime lock")?.status().state != LiveSessionState::Live {
        return Err("Start callback failed".into());
    }
    ui.invoke_kedi_simulate("xin chào".into());
    ui.invoke_kedi_control("pause".into());
    if shared.lock().map_err(|_| "runtime lock")?.status().state != LiveSessionState::Paused {
        return Err("Pause callback failed".into());
    }
    ui.invoke_kedi_control("resume".into());
    ui.invoke_kedi_control("takeover".into());
    if shared
        .lock()
        .map_err(|_| "runtime lock")?
        .status()
        .automatic
    {
        return Err("Takeover left automatic speech enabled".into());
    }
    ui.invoke_kedi_control("stop".into());
    let s = shared.lock().map_err(|_| "runtime lock")?.status();
    if s.state != LiveSessionState::Stopped || s.queued_tasks != 0 || s.active_task.is_some() {
        return Err("STOP did not drain and cancel the local runtime".into());
    }
    log::info!(
        "KEDI_OPERATOR_SMOKE=PASS enable/start/comment/pause/resume/takeover/stop; no platform writes"
    );
    Ok(())
}
