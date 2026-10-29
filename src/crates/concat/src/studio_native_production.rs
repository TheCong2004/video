//! Native executor behind Promax scheduling; no direct whole-pipeline submit.
use crate::ui::{App, ProductionScenes, ProductionStudio};
use concat_studio::{
    native_pipeline::{Pipeline, Services, WORKFLOW},
    pages::ContentPage,
    production_runtime::BrowserRuntime,
    promax_worker::CentralWorker,
    storyboard::Storyboard,
    storyboard_render::{RenderSize, render_board},
    workflows::{ProductionPlan as Plan, WorkflowClient},
};
use serde_json::{Value, json};
use slint::{ComponentHandle, Model};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
struct Runtime {
    worker: CentralWorker,
    pipeline: Arc<Pipeline>,
    _browser: BrowserRuntime,
    submitted: bool,
}
static RUNTIME: Mutex<Option<Runtime>> = Mutex::new(None);
static OUTPUT: Mutex<Option<PathBuf>> = Mutex::new(None);
pub(crate) fn shutdown() {
    if let Ok(mut s) = RUNTIME.lock() {
        s.take();
    }
}
pub(crate) fn submitted() {
    if let Ok(mut s) = RUNTIME.lock() {
        if let Some(r) = s.as_mut() {
            r.submitted = true;
        }
    }
}
/// A definitively rejected submission may be prepared again; ambiguous outcomes
/// retain their reservation and cannot be cleared by this path.
pub(crate) fn submission_rejected(reservation: &str) -> bool {
    if let Ok(mut slot) = RUNTIME.lock() {
        if let Some(runtime) = slot.as_mut() {
            if runtime.pipeline.id() == reservation && runtime.worker.status() != "outcome_unknown"
            {
                runtime.submitted = false;
                return true;
            }
        }
    }
    false
}
pub(crate) fn augment(config: &mut Value) {
    if let Ok(slot) = RUNTIME.lock() {
        if let Some(r) = slot.as_ref() {
            config["nativeReservationId"] = json!(r.pipeline.id());
            config["workerTarget"] = r.pipeline.target();
        }
    }
}
fn message(error: &str) -> String {
    if error.contains("PROMAX_TRANSPORT_NOT_CONFIGURED") {"BE Promax chưa cấu hình dịch vụ nhận worker. Cần cấu hình máy chủ Node/Nest trước khi tạo video; không phải lỗi đăng nhập hay dữ liệu Page."}
    else if error.contains("NEXORA_WORKER") {"Cần cập nhật Nexora có kết nối worker native rồi mở lại Nexora. Không cần đăng nhập Concat."}
    else if error.contains("SOURCE") {"Chọn lại ảnh tham chiếu nhân vật ở bước 4 trước khi tạo video."}
    else if error.contains("MODEL") {"Chọn model đã kết nối trong bước 2, hoặc nhập kịch bản có sẵn; chưa gửi chạy AI."}
    else if error.contains("PLAYWRIGHT") {"Bộ xử lý Grok chưa khởi động được. Kiểm tra bộ cài có Playwright runtime và Node."}
    else if error.contains("REVISION") {"Định nghĩa workflow native trên BE đã khác bản đi kèm. Không chạy graph không khớp."}
    else if error.contains("PENDING") {"Đang có tác vụ hoặc kết quả chưa rõ. Kiểm tra tác vụ trước, không tạo lại để tránh trùng."}
    else if error.contains("GROK_OPEN_UNKNOWN") {"Chưa xác nhận được tab Grok sau yêu cầu mở. Không tự mở lại để tránh trùng; kiểm tra đúng hồ sơ Nexora rồi thử lại."}
    else if error.contains("GROK_OPEN") {"Không chuẩn bị được tab Grok trong hồ sơ đã chọn. Kiểm tra Nexora; chưa gửi tạo nội dung."}
    else if error.contains("GROK_WORKER") {"Đã yêu cầu mở hoặc dùng lại Grok trong hồ sơ đã chọn, nhưng Grok chưa sẵn sàng. Đăng nhập Grok nếu được yêu cầu, kiểm tra tiện ích rồi bấm Tạo video lại; chưa gửi tạo nội dung."}
    else if error.contains("READY") {"Worker chưa được BE xác nhận sẵn sàng. Kiểm tra Nexora và kết nối mạng rồi bấm kiểm tra lại."}
    else if error.contains("UNKNOWN") || error.contains("UNCONFIRMED") {"Kết quả gửi chưa rõ; không tự gửi lại. Kiểm tra tác vụ và hồ sơ Grok trên Nexora."}
    else {"Không hoàn tất công đoạn sản xuất. Kiểm tra thông báo worker, giọng đọc và phiên Grok trong đúng hồ sơ Nexora; chưa xác nhận video hoàn thành."}.into()
}
#[derive(Clone)]
struct Target {
    epoch: u64,
    path: String,
}
fn target() -> Option<Target> {
    let mut t = None;
    crate::host::Shell::with(|shell, _| {
        t = shell.studio.borrow().session.as_ref().map(|s| Target {
            epoch: crate::host::project_epoch(),
            path: s.path().into(),
        });
    });
    t
}
fn import(app: &App, path: PathBuf, expected: Option<&Target>) {
    if verify(&path).is_err() {
        app.global::<ProductionStudio>()
            .set_status("Video đầu ra chưa qua kiểm tra; không nhập.".into());
        return;
    }
    crate::host::Shell::with(|shell, app| {
        let matches = expected.is_some_and(|t| {
            t.epoch == crate::host::project_epoch()
                && shell
                    .studio
                    .borrow()
                    .session
                    .as_ref()
                    .is_some_and(|s| s.path() == t.path)
        });
        if matches {
            shell.studio.borrow_mut().handle(crate::panes::Msg::Media(
                crate::panes::media_bin::MediaMsg::Import(vec![path]),
            ));
            shell.studio.borrow_mut().refresh_art();
            shell.studio.borrow().publish(&app, &shell.models);
        } else {
            app.global::<ProductionStudio>().set_status("Video đã lưu. Mở đúng dự án rồi bấm Nhập video vào Concat; không tự nhập vào dự án khác.".into());
        }
    });
}
fn verify(path: &Path) -> Result<(), String> {
    use concat_media::FrameSource;
    let info = concat_media::probe::probe(path).map_err(|_| "VIDEO_PROBE_FAILED")?;
    info.require_video().map_err(|_| "VIDEO_STREAM_MISSING")?;
    let options = concat_media::DecodeOptions::default()
        .limited_to(1)
        .threaded(1)
        .in_software();
    let mut decoder =
        concat_media::Decoder::open(path, &options).map_err(|_| "VIDEO_DECODE_FAILED")?;
    if decoder
        .next_frame()
        .map_err(|_| "VIDEO_DECODE_FAILED")?
        .is_none()
    {
        return Err("VIDEO_FRAME_MISSING".into());
    }
    Ok(())
}
struct NativeServices {
    weak: slint::Weak<App>,
    session_generation: u64,
    project: Option<Target>,
    board: Storyboard,
    script: String,
    brief: String,
    model: String,
    sources: String,
    duration: f64,
    scenes: usize,
    voice: String,
    ai_generation: Option<u64>,
    voice_generation: u64,
    cloud: WorkflowClient,
}
impl Services for NativeServices {
    fn sources(&self) -> Result<(), String> {
        // This stage verifies source metadata only. It never invents the contents
        // of a URL or claims that model knowledge is verified web research.
        for source in self
            .sources
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            self.cloud
                .source_title(source)
                .map_err(|_| "SOURCE_METADATA_UNAVAILABLE")?;
        }
        Ok(())
    }
    fn script(&self) -> Result<Storyboard, String> {
        if !self.board.shots.is_empty() {
            return Ok(self.board.clone());
        }
        if !self.script.trim().is_empty() {
            return Storyboard::from_script(&self.script, self.duration);
        }
        if crate::studio_local_ai::generation() != self.ai_generation {
            return Err("MODEL_CONNECTION_CHANGED".into());
        }
        let client = crate::studio_local_ai::client().map_err(|_| "MODEL_CONNECTION_REQUIRED")?;
        let prompt = format!(
            "Viết kịch bản tiếng Việt từ ý tưởng bên dưới, {} cảnh, tổng khoảng {} giây. Chỉ trả JSON không markdown: {{\"title\":\"...\",\"shots\":[{{\"title\":\"...\",\"narration\":\"...\",\"duration\":5,\"direction\":{{\"setting\":\"...\",\"prompt\":\"...\"}}}}]}}. Không đường dẫn, không đánh dấu đã tạo ảnh/video. Không bịa nội dung nguồn URL chưa đọc. Ý tưởng: {}",
            self.scenes, self.duration, self.brief
        );
        let answer = client
            .ai_ask(&self.model, &prompt)
            .map_err(|_| "SCRIPT_SUBMISSION_UNKNOWN")?;
        Storyboard::from_ai(&answer.text)
    }
    fn voice(
        &self,
        board: &mut Storyboard,
        root: &Path,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        for (i, shot) in board.shots.iter_mut().enumerate() {
            if cancel.load(Ordering::Acquire) {
                return Err("CANCELLED".into());
            }
            if shot.narration.trim().is_empty() {
                continue;
            }
            if crate::studio_live_director::voice_revision() != self.voice_generation {
                return Err("VOICE_CONFIGURATION_CHANGED".into());
            }
            if shot.voice.is_none() {
                let _turn = crate::studio_live_director::turn().map_err(str::to_owned)?;
                let audio = crate::studio_live_director::synthesize(&shot.narration, &self.voice)
                    .map_err(|_| "VOICE_GENERATION_FAILED")?;
                let path = root.join(format!(
                    "voice-{i}.{}",
                    if audio.media_type.contains("wav") {
                        "wav"
                    } else {
                        "mp3"
                    }
                ));
                std::fs::write(&path, audio.bytes).map_err(|_| "VOICE_SAVE_FAILED")?;
                shot.voice = Some(path);
            }
            let duration = crate::studio_scenes::audio_duration(
                shot.voice.as_deref().ok_or("VOICE_MISSING")?,
            )?;
            if duration > shot.duration {
                shot.duration = duration;
            }
        }
        Ok(())
    }
    fn render(
        &self,
        board: &Storyboard,
        root: &Path,
        cancel: &AtomicBool,
    ) -> Result<PathBuf, String> {
        render_board(
            &crate::studio_scenes::ffmpeg()?,
            &crate::studio_scenes::font()?,
            board,
            &root.join("assembly"),
            RenderSize {
                width: 1080,
                height: 1920,
            },
            cancel,
            |i, total| self.progress(6, i, total),
        )
    }
    fn verify_video(&self, path: &Path) -> Result<(), String> {
        verify(path)
    }
    fn progress(&self, stage: usize, scene: usize, total: usize) {
        let generation = self.session_generation;
        let text = format!(
            "Promax · công đoạn {}/7 · cảnh {}/{}",
            stage + 1,
            scene + 1,
            total
        );
        let _ = self.weak.upgrade_in_event_loop(move |app| {
            if crate::studio_session::session_generation() != generation {
                return;
            }
            app.global::<ProductionStudio>()
                .set_status(text.clone().into());
            app.global::<ProductionStudio>()
                .set_local_progress(text.into());
        });
    }
    fn completed(&self, board: Storyboard, video: PathBuf) {
        if let Err(error) = crate::studio_video_library::record(&video, "Xưởng phim AI") {
            log::warn!("Cannot catalog completed video: {error}");
        }
        if let Ok(mut out) = OUTPUT.lock() {
            *out = Some(video.clone())
        }
        let project = self.project.clone();
        let generation = self.session_generation;
        let _ = self.weak.upgrade_in_event_loop(move |app| {
            if crate::studio_session::session_generation() != generation {
                return;
            }
            let s = app.global::<ProductionStudio>();
            s.set_local_has_output(true);
            s.set_local_running(false);
            s.set_status("Đã dựng và kiểm tra video thật. Đang đưa vào dự án Concat.".into());
            app.global::<crate::ui::StudioBulk>().invoke_refresh_videos();
            crate::studio_scenes::adopt_production(
                &app,
                board,
                project.as_ref().is_some_and(|t| {
                    target().is_some_and(|now| now.epoch == t.epoch && now.path == t.path)
                }),
            );
            import(&app, video, project.as_ref());
        });
    }
    fn failed(&self, error: &str) {
        let safe = message(error);
        let generation = self.session_generation;
        log::warn!("Native production stage failed: {}", error);
        let _ = self.weak.upgrade_in_event_loop(move |app| {
            if crate::studio_session::session_generation() != generation {
                return;
            }
            let s = app.global::<ProductionStudio>();
            s.set_ready(false);
            s.set_local_running(false);
            s.set_status(safe.into());
        });
    }
}

pub(crate) fn prepare(
    app: &App,
    anchor: Arc<Mutex<Option<PathBuf>>>,
    gate: Arc<Mutex<Option<Plan>>>,
    start_after_prepare: bool,
) {
    let capture = (|| -> Result<_, String> {
        if RUNTIME
            .lock()
            .map_err(|_| "WORKER_UNAVAILABLE")?
            .as_ref()
            .is_some_and(|r| {
                (r.submitted && !r.pipeline.finished()) || r.worker.status() == "outcome_unknown"
            })
        {
            return Err("WORKER_PENDING".into());
        }
        let ui = app.global::<ProductionStudio>();
        let source = anchor
            .lock()
            .map_err(|_| "SOURCE_IMAGE_MISSING")?
            .clone()
            .ok_or("SOURCE_IMAGE_MISSING")?;
        let page =
            crate::studio_production_pages::selected().ok_or("Chọn Page Nexora trước khi chạy.")?;
        let board = crate::studio_scenes::production_snapshot(app)?;
        let scenes = app.global::<ProductionScenes>();
        let model = scenes
            .get_models()
            .row_data(scenes.get_model_index().max(0) as usize)
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                page.preset
                    .as_ref()
                    .and_then(|p| p.default_model_id.clone())
            })
            .unwrap_or_default();
        if board.shots.is_empty() && ui.get_script().trim().is_empty() && model.is_empty() {
            return Err("MODEL_REQUIRED".into());
        }
        let plan = super::studio_production::plan(app)
            .map_err(|_| "Kiểm tra ý tưởng, URL, số cảnh và JSON; cấu hình chưa hợp lệ.")?;
        let dirs = concat_host::AppDirs::locate()?;
        let browser = concat_studio::nexora::NexoraSettings::load(&dirs.config)
            .and_then(|s| s.client())
            .map_err(|_| "NEXORA_WORKER_BRIDGE_REQUIRED")?;
        let cloud = WorkflowClient::via_nexora(browser.clone(), &page.profile.id)
            .map_err(|_| "NEXORA_WORKER_BRIDGE_REQUIRED")?;
        let root = page
            .preset
            .as_ref()
            .map(|p| PathBuf::from(&p.output_root))
            .unwrap_or_else(|| dirs.data.join("studio/production").join(&page.profile.id));
        let preset:ContentPage=page.preset.clone().unwrap_or(serde_json::from_value(json!({"id":page.profile.id,"name":page.profile.name,"slug":page.profile.id,"output_root":root,"browser_profile_id":page.profile.id,"default_aspect_ratio":"9:16","is_archived":false,"created_at":1,"updated_at":1,"revision":1})).map_err(|_|"PAGE_INVALID")?);
        // The selected native graph owns its effective output ratio. Do not
        // carry a generic channel's 16:9 preference into its 9:16 browser plan.
        let mut preset = preset;
        preset.default_aspect_ratio = Some("9:16".into());
        let services = Arc::new(NativeServices {
            weak: app.as_weak(),
            session_generation: crate::studio_session::session_generation(),
            project: target(),
            board,
            script: ui.get_script().into(),
            brief: ui.get_brief().into(),
            model,
            sources: ui.get_sources().into(),
            duration: ui.get_duration() as f64,
            scenes: ui.get_scenes() as usize,
            voice: if ui.get_voice_id().is_empty() {
                "edge-tts:vi-VN-HoaiMyNeural".into()
            } else {
                ui.get_voice_id().into()
            },
            ai_generation: crate::studio_local_ai::generation(),
            voice_generation: crate::studio_live_director::voice_revision(),
            cloud,
        });
        let image = if ui.get_image_prompt().trim().is_empty() {
            let shared = format!("{}\n{}", ui.get_brief(), ui.get_character_anchor());
            if shared.trim().is_empty() {
                // Per-scene instructions/narration are appended by the native
                // pipeline. This is an identity constraint, not invented scene content.
                "Giữ nhận diện nhân vật theo ảnh tham chiếu; thực hiện chỉ đạo riêng của từng cảnh."
                    .into()
            } else {
                shared
            }
        } else {
            ui.get_image_prompt().to_string()
        };
        Ok((
            source,
            page,
            preset,
            root,
            browser,
            services,
            plan,
            image,
            ui.get_expand_prompt().to_string(),
            ui.get_motion_prompt().to_string(),
        ))
    })();
    let (source, page, preset, root, browser, services, mut plan, image, expand, video) =
        match capture {
            Ok(v) => v,
            Err(e) => {
                // Keep required input reachable even after scenes have been added.
                if e.contains("SOURCE_IMAGE") {
                    app.global::<ProductionStudio>().set_current_phase(4);
                }
                log::warn!(
                    "Native production input rejected: {}",
                    if e.contains("SOURCE_IMAGE") {
                        "reference_image"
                    } else if e.contains("MODEL") {
                        "model_or_script"
                    } else {
                        "configuration"
                    }
                );
                app.global::<ProductionStudio>().set_status(
                    if e.chars().any(char::is_whitespace) {
                        e.into()
                    } else {
                        message(&e).into()
                    },
                );
                return;
            }
        };
    let state = app.global::<ProductionStudio>();
    if !start_after_prepare {
        state.set_ready(false);
        state.set_status("Cấu hình đầu vào hợp lệ. Bấm Tạo video để tự mở hoặc dùng lại Grok và kiểm tra worker; chưa mở trình duyệt hay gửi tạo.".into());
        return;
    }
    state.set_busy(true);
    state.set_ready(false);
    state.set_status(
        "Đang mở hoặc dùng lại tab Grok trong hồ sơ đã chọn… Chưa gửi tạo nội dung.".into(),
    );
    let weak = app.as_weak();
    let generation = crate::studio_session::session_generation();
    let spawned = std::thread::Builder::new()
        .name("native-production-prepare".into())
        .spawn(move || {
            let result = (|| -> Result<Plan, String> {
                if crate::studio_session::session_generation() != generation {
                    return Err("PAGE_CHANGED".into());
                }
                let live_profile = browser
                    .profiles()
                    .map_err(|_| "GROK_OPEN_FAILED")?
                    .into_iter()
                    .find(|p| p.id == page.profile.id)
                    .ok_or("PAGE_CHANGED")?;
                browser
                    .ensure_production_grok(&live_profile)
                    .map_err(|e| match e {
                        concat_studio::browser::BrowserError::UnknownOutcome => "GROK_OPEN_UNKNOWN",
                        _ => "GROK_OPEN_FAILED",
                    })?;
                let cloud = WorkflowClient::via_nexora(browser.clone(), &page.profile.id)
                    .map_err(|_| "NEXORA_WORKER_BRIDGE_REQUIRED")?;
                let graph = cloud
                    .ensure_native_definition()
                    .map_err(|_| "NATIVE_WORKFLOW_REGISTER_FAILED")?;
                let identity = browser.production_worker_identity(&page.profile.id)?;
                if crate::studio_session::session_generation() != generation {
                    return Err("PAGE_CHANGED".into());
                }
                if let Ok(mut slot) = RUNTIME.lock() {
                    slot.take();
                }
                let sidecar = BrowserRuntime::ensure()?;
                let grok_until = Instant::now() + Duration::from_secs(45);
                while !browser.production_browser_ready(&page.profile.id)? {
                    if crate::studio_session::session_generation() != generation {
                        return Err("PAGE_CHANGED".into());
                    }
                    if Instant::now() >= grok_until {
                        return Err("GROK_WORKER_NOT_READY".into());
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
                let recovery = concat_host::AppDirs::locate()?
                    .config
                    .join("studio/production-recovery");
                let pipeline = Arc::new(Pipeline::reserve(
                    identity,
                    graph,
                    &root,
                    &recovery,
                    preset,
                    source,
                    image,
                    expand,
                    video,
                    browser.clone(),
                    services,
                )?);
                plan.inputs["production_settings"]["nativeReservationId"] = json!(pipeline.id());
                plan.inputs["production_settings"]["workerTarget"] = pipeline.target();
                let worker = CentralWorker::via_nexora(
                    browser,
                    page.profile.id,
                    root.join("worker-ledger"),
                    pipeline.clone(),
                )?;
                let until = Instant::now() + Duration::from_secs(20);
                while Instant::now() < until && worker.status() != "ready" {
                    if worker.status() == "PROMAX_TRANSPORT_NOT_CONFIGURED" {
                        return Err("PROMAX_TRANSPORT_NOT_CONFIGURED".into());
                    }
                    if crate::studio_session::session_generation() != generation {
                        return Err("PAGE_CHANGED".into());
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
                if worker.status() != "ready" {
                    return Err("WORKER_NOT_READY".into());
                }
                let checked = cloud
                    .preflight(WORKFLOW, &plan.inputs)
                    .map_err(|_| "WORKER_NOT_READY")?;
                if !checked.can_start || !checked.worker_available {
                    return Err("WORKER_NOT_READY".into());
                }
                *RUNTIME.lock().map_err(|_| "WORKER_UNAVAILABLE")? = Some(Runtime {
                    worker,
                    pipeline,
                    _browser: sidecar,
                    submitted: false,
                });
                Ok(plan)
            })();
            let _ = weak.upgrade_in_event_loop(move |app| {
                let s = app.global::<ProductionStudio>();
                s.set_busy(false);
                if crate::studio_session::session_generation() != generation {
                    s.set_ready(false);
                    return;
                }
                match result {
                    Ok(checked)
                        if super::studio_production::plan(&app).ok().as_ref() == Some(&checked) =>
                    {
                        if let Ok(mut gate) = gate.lock() {
                            *gate = Some(checked.clone());
                        }
                        super::studio_production::submit_native(&app, checked);
                    }
                    Ok(_) => {
                        shutdown();
                        s.set_status("Cấu hình đã thay đổi; kiểm tra lại trước khi chạy.".into());
                    }
                    Err(e) => {
                        log::warn!("Native production preflight: {e}");
                        s.set_status(message(&e).into());
                    }
                }
            });
        });
    if spawned.is_err() {
        state.set_busy(false);
        state.set_status("Không mở được tác vụ chuẩn bị worker.".into());
    }
}
pub(crate) fn bind(app: &App) {
    let weak = app.as_weak();
    app.global::<ProductionStudio>().on_import_output(move || {
        if let Some(app) = weak.upgrade() {
            if let Some(path) = OUTPUT.lock().ok().and_then(|s| s.clone()) {
                import(&app, path, target().as_ref());
            }
        }
    });
}
