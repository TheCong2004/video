// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native workflow execution. No automatic submission or retry of mutations.
use crate::i18n::t;
use crate::ui::{App, ProductionStudio, StudioRunRow};
use concat_host::studio_workflows::{
    ProductionPlan as Plan, RunError, RunSummary, WorkflowClient, WorkflowError,
};
use slint::{ComponentHandle, Model, ModelRc, VecModel};

enum Action {
    List,
    Source(String),
    Check(Plan),
    Start(Plan),
    Cancel(String),
    Resume(String),
    Definitions,
}
enum Reply {
    Runs(Vec<RunSummary>),
    Source(String, String),
    Checked(Plan),
    Started(RunSummary),
    Cancelled(RunSummary),
    Resumed(RunSummary),
    Definitions(Vec<concat_host::studio_workflows::WorkflowDefinition>),
}

fn client() -> Result<WorkflowClient, RunError> {
    crate::studio_session::production_client().map_err(RunError::Remote)
}

pub(crate) fn plan(app: &App) -> Result<Plan, RunError> {
    let state = app.global::<ProductionStudio>();
    let index = usize::try_from(state.get_workflow_index()).map_err(|_| RunError::Input)?;
    let workflow = state
        .get_workflow_ids()
        .row_data(index)
        .ok_or(RunError::Input)?
        .to_string();
    let page = crate::studio_production_pages::selected().ok_or(RunError::Input)?;
    if page.workflow_id().is_some_and(|id| {
        !id.is_empty() && id != workflow && workflow != concat_studio::native_pipeline::WORKFLOW
    }) {
        return Err(RunError::NotReady);
    }
    // A local reference is not an uploaded artifact. Never send a machine path
    // as if the remote worker could read it or treat the image as source video.
    if !state.get_anchor_label().is_empty() && workflow != concat_studio::native_pipeline::WORKFLOW
    {
        return Err(RunError::NotReady);
    }
    let mut extra: serde_json::Value =
        serde_json::from_str(&state.get_extra_inputs()).map_err(|_| RunError::Input)?;
    let fields = extra.as_object_mut().ok_or(RunError::Input)?;
    if fields.contains_key("production_settings") {
        return Err(RunError::Input);
    }
    let mut config = settings(app);
    config["browserProfileId"] = serde_json::json!(page.profile.id);
    config["pageSnapshot"] = serde_json::to_value(page).map_err(|_| RunError::Input)?;
    if workflow == concat_studio::native_pipeline::WORKFLOW {
        crate::studio_native_production::augment(&mut config);
        // These are properties of the selected graph, not stale controls/presets
        // retained from another workflow. The native UI displays them read-only.
        config["visualEngine"] = serde_json::json!("grok_web");
        config["editingPreset"]["aspect"] = serde_json::json!("9:16");
        config["editingPreset"]["transition_style"] = serde_json::json!("cut");
        config["editingPreset"]["video_speed"] = serde_json::json!(1);
        config["editingPreset"]["music_volume"] = serde_json::json!(0.18);
    }
    let brief = if workflow == concat_studio::native_pipeline::WORKFLOW
        && state.get_brief().trim().is_empty()
        && app.global::<crate::ui::ProductionScenes>().get_count() > 0
    {
        format!(
            "Dựng phim từ {} cảnh đã biên soạn.",
            app.global::<crate::ui::ProductionScenes>().get_count()
        )
    } else if workflow == concat_studio::native_pipeline::WORKFLOW
        && state.get_brief().trim().is_empty()
    {
        state.get_script().to_string()
    } else {
        state.get_brief().to_string()
    };
    fields.insert("production_settings".into(), config);
    Plan::new(
        workflow,
        &brief,
        &state.get_sources(),
        state.get_scenes(),
        state.get_duration(),
        &extra.to_string(),
    )
}

/// Versioned configuration, not a completion result or provider capability claim.
fn settings(app: &App) -> serde_json::Value {
    let s = app.global::<ProductionStudio>();
    fn choice(index: i32, choices: &[&str]) -> String {
        usize::try_from(index)
            .ok()
            .and_then(|i| choices.get(i))
            .unwrap_or(&choices[0])
            .to_string()
    }
    serde_json::json!({
        "version": 1,
        "storyboard": crate::studio_scenes::workflow_shots(),
        "researchEnabled": s.get_research_enabled(),
        "script": s.get_script().as_str(),
        "assetUrls": s.get_asset_urls().lines().map(str::trim).filter(|v| !v.is_empty()).collect::<Vec<_>>(),
        "characterDescription": s.get_character_anchor().as_str(),
        "visualStyle": s.get_visual_style().as_str(),
        "visualEngine": choice(s.get_visual_engine_index(), &["omniroute", "grok_web", "fal"]),
        "imagePrompt": s.get_image_prompt().as_str(),
        "expand916Prompt": s.get_expand_prompt().as_str(),
        "videoPrompt": s.get_motion_prompt().as_str(),
        "voiceStyle": choice(s.get_voice_style_index(), &["storytelling_vietnamese", "narration", "conversation"]),
        "voiceId": s.get_voice_id().as_str(),
        "editingPreset": {
            "aspect": choice(s.get_aspect_index(), &["9:16", "16:9", "1:1"]),
            "subtitle_style": choice(s.get_subtitle_index(), &["dynamic", "simple", "none"]),
            "transition_style": choice(s.get_transition_index(), &["smooth", "cut", "fade"]),
            "video_speed": s.get_video_speed(),
            "music_volume": s.get_music_volume() / 100.0
        }
    })
}

fn row(run: RunSummary) -> StudioRunRow {
    StudioRunRow {
        id: run.id.into(),
        workflow: run.workflow_id.into(),
        status: run.status.into(),
        node: run.node.into(),
        created: run.created_at.into(),
        completed: run.completed_steps as i32,
        total: run.total_steps as i32,
    }
}

fn message(error: RunError) -> String {
    match error {
        RunError::Input => t("nativeStudio.invalidPlan"),
        RunError::NotReady => t("nativeStudio.workerMissing"),
        RunError::UnknownOutcome => t("nativeStudio.unknownOutcome"),
        RunError::Remote(WorkflowError::Authentication) => "Kết nối thiết bị Nexora chưa được BE xác nhận. Không cần đăng nhập Concat; kiểm tra kết nối Nexora với Promax.".into(),
        RunError::Remote(WorkflowError::Forbidden) => t("productionStudio.accessDenied"),
        RunError::Remote(WorkflowError::RateLimited) => t("productionStudio.rateLimited"),
        RunError::Remote(WorkflowError::Configuration) => {
            t("productionStudio.workflowConfiguration")
        }
        RunError::Remote(WorkflowError::InvalidResponse) => t("productionStudio.workflowInvalid"),
        RunError::Remote(WorkflowError::Unavailable) => "Chưa gọi được quy trình qua Nexora. Nếu Page đã hiện, cần kiểm tra/cập nhật kết nối Nexora–Promax; chưa gửi chạy thành công.".into(),
    }
}

pub(crate) fn bind(app: &App) {
    let anchor = std::sync::Arc::new(std::sync::Mutex::new(None::<std::path::PathBuf>));
    let selected = anchor.clone();
    let weak = app.as_weak();
    app.global::<ProductionStudio>().on_choose_anchor(move || {
        let Some(app) = weak.upgrade() else { return };
        let state = app.global::<ProductionStudio>();
        if state.get_busy() { return; }
        state.set_busy(true);
        let selected = selected.clone();
        let weak = app.as_weak();
        let spawned = std::thread::Builder::new().name("production-anchor".into()).spawn(move || {
            let result = (|| -> Result<_, ()> {
                let Some(path) = rfd::FileDialog::new().set_title("Ảnh tham chiếu nhân vật · PNG/JPEG")
                    .add_filter("Ảnh", &["png", "jpg", "jpeg"]).pick_file() else { return Ok(None) };
                let mut reader = std::fs::File::open(&path).map_err(|_| ())?;
                // Bound both encoded bytes and decoded memory before allocation.
                use std::io::Read;
                let mut bytes = Vec::new();
                std::io::Read::by_ref(&mut reader).take(10 * 1024 * 1024 + 1).read_to_end(&mut bytes).map_err(|_| ())?;
                if bytes.len() > 10 * 1024 * 1024 { return Err(()) }
                let mut decoder = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|_| ())?;
                let mut limits = image::Limits::default();
                limits.max_image_width = Some(8192);
                limits.max_image_height = Some(8192);
                limits.max_alloc = Some(128 * 1024 * 1024);
                decoder.limits(limits);
                let preview = decoder.decode().map_err(|_| ())?.thumbnail(480, 480).to_rgba8();
                Ok(Some((path, preview)))
            })();
            let _ = weak.upgrade_in_event_loop(move |app| {
                let state = app.global::<ProductionStudio>();
                state.set_busy(false);
                match result {
                    Ok(Some((path, pixels))) => {
                        state.set_anchor_label(path.file_name().unwrap_or_default().to_string_lossy().into_owned().into());
                        state.set_anchor_preview(slint::Image::from_rgba8(slint::SharedPixelBuffer::clone_from_slice(pixels.as_raw(), pixels.width(), pixels.height())));
                        *selected.lock().unwrap() = Some(path);
                        state.set_ready(false); state.set_confirm_start(false);
                        state.set_status("Đã chọn ảnh trên máy. Chưa tải lên hoặc tạo video.".into());
                    }
                    Ok(None) => {}
                    Err(()) => state.set_status("Không đọc được ảnh. Chọn PNG/JPEG tối đa 10 MB, kích thước tối đa 8192 px.".into()),
                }
            });
        });
        if spawned.is_err() { state.set_busy(false); state.set_status(t("nativeStudio.threadError").into()); }
    });
    let selected = anchor.clone();
    let weak = app.as_weak();
    app.global::<ProductionStudio>().on_clear_anchor(move || {
        let Some(app) = weak.upgrade() else { return };
        let state = app.global::<ProductionStudio>();
        if state.get_busy() {
            return;
        }
        *selected.lock().unwrap() = None;
        state.set_anchor_label("".into());
        state.set_anchor_preview(Default::default());
        state.set_ready(false);
        state.set_confirm_start(false);
    });
    let weak = app.as_weak();
    let export_anchor = anchor.clone();
    app.global::<ProductionStudio>().on_export_settings(move || {
        let Some(app) = weak.upgrade() else { return };
        let state = app.global::<ProductionStudio>();
        if state.get_busy() { return; }
        // Snapshot on the UI thread; file dialog and all disk IO stay off it.
        let document = serde_json::json!({
            "format": "concat-production-config", "version": 1,
            "brief": state.get_brief().as_str(), "sources": state.get_sources().as_str(),
            "sceneCount": state.get_scenes(), "durationSeconds": state.get_duration(),
            "advancedInputs": state.get_extra_inputs().as_str(),
            "settings": settings(&app),
            "localAnchorPath": export_anchor.lock().unwrap().clone(),
            "execution": "not-submitted"
        });
        state.set_busy(true);
        let weak = app.as_weak();
        let spawned = std::thread::Builder::new().name("production-export".into()).spawn(move || {
            let result = (|| -> Result<bool, ()> {
                let Some(path) = rfd::FileDialog::new().set_title("Lưu cấu hình 7 bước · chưa tạo video")
                    .set_file_name("xuong-phim-config.json").add_filter("JSON", &["json"]).save_file() else { return Ok(false) };
                let bytes = serde_json::to_vec_pretty(&document).map_err(|_| ())?;
                std::fs::write(path, bytes).map_err(|_| ())?;
                Ok(true)
            })();
            let _ = weak.upgrade_in_event_loop(move |app| {
                let state = app.global::<ProductionStudio>(); state.set_busy(false);
                match result {
                    Ok(true) => state.set_status("Đã lưu cấu hình 7 bước ra JSON. Đây không phải video đã xuất; chưa gửi lên dịch vụ.".into()),
                    Ok(false) => {},
                    Err(()) => state.set_status("Không lưu được cấu hình. Chọn thư mục có quyền ghi rồi thử lại.".into()),
                }
            });
        });
        if spawned.is_err() { state.set_busy(false); state.set_status(t("nativeStudio.threadError").into()); }
    });
    let weak = app.as_weak();
    app.global::<ProductionStudio>().on_check_source(move || {
        let Some(app) = weak.upgrade() else { return };
        let state = app.global::<ProductionStudio>();
        let sources = state.get_sources().to_string();
        if sources
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count()
            != 1
        {
            state.set_status(
                "Nhập một URL YouTube để kiểm tra nguồn. Chưa tải file hoặc tạo job.".into(),
            );
            return;
        }
        run(&app, Action::Source(sources.trim().to_owned()), None);
    });
    let prepared = std::sync::Arc::new(std::sync::Mutex::new(None::<Plan>));
    let weak = app.as_weak();
    app.global::<ProductionStudio>().on_load_runs(move || {
        if let Some(app) = weak.upgrade() {
            run(&app, Action::List, None);
        }
    });
    let weak = app.as_weak();
    let gate = prepared.clone();
    let worker_anchor = anchor.clone();
    app.global::<ProductionStudio>().on_check_plan(move || {
        let Some(app) = weak.upgrade() else {
            return;
        };
        if app.global::<ProductionStudio>().get_busy() {
            return;
        }
        *gate.lock().unwrap() = None;
        let ui = app.global::<ProductionStudio>();
        if ui
            .get_workflow_ids()
            .row_data(ui.get_workflow_index().max(0) as usize)
            .is_some_and(|id| id == concat_studio::native_pipeline::WORKFLOW)
        {
            crate::studio_native_production::prepare(
                &app,
                worker_anchor.clone(),
                gate.clone(),
                false,
            );
            return;
        }
        match plan(&app) {
            Ok(plan) => run(&app, Action::Check(plan), Some(gate.clone())),
            Err(error) => app
                .global::<ProductionStudio>()
                .set_status(message(error).into()),
        }
    });
    let weak = app.as_weak();
    let gate = prepared;
    let start_anchor = anchor.clone();
    app.global::<ProductionStudio>().on_start_run(move || {
        let Some(app) = weak.upgrade() else {
            return;
        };
        let state = app.global::<ProductionStudio>();
        log::info!(
            "Production Create: native={}, busy={}, uncertain={}, running={}, confirmed={}",
            state.get_native_workflow(),
            state.get_busy(),
            state.get_uncertain(),
            state.get_local_running(),
            state.get_confirm_start()
        );
        if state.get_native_workflow() {
            if state.get_busy()
                || state.get_uncertain()
                || state.get_local_running()
                || !state.get_confirm_start()
            {
                return;
            }
            state.set_confirm_start(false);
            crate::studio_native_production::prepare(
                &app,
                start_anchor.clone(),
                gate.clone(),
                true,
            );
            return;
        }
        if state.get_busy()
            || state.get_uncertain()
            || !state.get_confirm_start()
            || !state.get_ready()
        {
            return;
        }
        let current = plan(&app);
        let prepared = gate.lock().unwrap().take();
        state.set_ready(false);
        state.set_confirm_start(false);
        match (current, prepared) {
            (Ok(current), Some(prepared)) if current == prepared => {
                if current.workflow == concat_studio::native_pipeline::WORKFLOW {
                    crate::studio_native_production::submitted();
                    state.set_local_running(true);
                    state.set_local_has_output(false);
                }
                run(&app, Action::Start(current), None)
            }
            _ => state.set_status(t("nativeStudio.preflightAgain").into()),
        }
    });
    let weak = app.as_weak();
    app.global::<ProductionStudio>().on_cancel_run(move |id| {
        if let Some(app) = weak.upgrade() {
            let state = app.global::<ProductionStudio>();
            if id.is_empty()
                || state.get_cancel_id() != id
                || state.get_busy()
                || state.get_uncertain()
            {
                return;
            }
            run(&app, Action::Cancel(id.to_string()), None);
        }
    });
    let weak = app.as_weak();
    app.global::<ProductionStudio>().on_resume_run(move |id| {
        let Some(app) = weak.upgrade() else {
            return;
        };
        let state = app.global::<ProductionStudio>();
        if id.is_empty()
            || id != state.get_resume_id().as_str()
            || state.get_busy()
            || state.get_uncertain()
        {
            return;
        }
        run(&app, Action::Resume(id.to_string()), None);
    });
    let weak = app.as_weak();
    app.global::<ProductionStudio>().on_load_workflows(move || {
        if let Some(app) = weak.upgrade() {
            run(&app, Action::Definitions, None);
        }
    });
    // This binding changes Page configuration only; it cannot override submission.
    crate::studio_production_pages::bind(app);
}

/// Called only after the user's Create confirmation and exact native preflight.
pub(crate) fn submit_native(app: &App, checked: Plan) {
    let state = app.global::<ProductionStudio>();
    if app.global::<crate::ui::StudioSession>().get_busy() || state.get_busy() {
        state.set_status(
            "Kết nối đang bận. Chờ xong rồi bấm Tạo video lại; chưa gửi tác vụ.".into(),
        );
        return;
    }
    if plan(app).ok().as_ref() != Some(&checked) {
        state.set_ready(false);
        state.set_status(t("nativeStudio.preflightAgain").into());
        return;
    }
    crate::studio_native_production::submitted();
    state.set_ready(false);
    state.set_local_running(true);
    state.set_local_has_output(false);
    run(app, Action::Start(checked), None);
}

fn run(app: &App, action: Action, gate: Option<std::sync::Arc<std::sync::Mutex<Option<Plan>>>>) {
    let native_reservation = match &action {
        Action::Start(plan) if plan.workflow == concat_studio::native_pipeline::WORKFLOW => plan
            .inputs["production_settings"]["nativeReservationId"]
            .as_str()
            .map(str::to_owned),
        _ => None,
    };
    let failed_spawn_reservation = native_reservation.clone();
    let session = app.global::<crate::ui::StudioSession>();
    if session.get_busy() {
        return;
    }
    let state = app.global::<ProductionStudio>();
    if state.get_busy() {
        return;
    }
    state.set_busy(true);
    state.set_ready(false);
    state.set_status(t("nativeStudio.working").into());
    let session_generation = crate::studio_session::session_generation();
    let weak = app.as_weak();
    let spawned = std::thread::Builder::new()
        .name("studio-production".into())
        .spawn(move || {
            let result = (|| {
                let client = client()?;
                if crate::studio_session::session_generation() != session_generation {
                    return Err(RunError::NotReady);
                }
                match action {
                    Action::Definitions => client.definitions().map(Reply::Definitions).map_err(RunError::Remote),
                    Action::List => client.runs().map(Reply::Runs).map_err(RunError::Remote),
                    Action::Source(source) => {
                        let title = client.source_title(&source).map_err(RunError::Remote)?;
                        Ok(Reply::Source(source, title))
                    }
                    Action::Check(plan) => {
                        let result = client.preflight(&plan.workflow, &plan.inputs)?;
                        if !result.can_start || (result.requires_worker && !result.worker_available)
                        {
                            return Err(RunError::NotReady);
                        }
                        Ok(Reply::Checked(plan))
                    }
                    Action::Start(plan) => client
                        .start(&plan.workflow, &plan.inputs)
                        .map(Reply::Started),
                    Action::Cancel(id) => client.cancel_run(&id).map(Reply::Cancelled),
                    Action::Resume(id) => client.resume_run(&id).map(Reply::Resumed),
                }
            })();
            let _ = weak.upgrade_in_event_loop(move |app| {
                let state = app.global::<ProductionStudio>();
                state.set_busy(false);
                if crate::studio_session::session_generation() != session_generation {
                    state.set_ready(false);
                    state.set_confirm_start(false);
                    // An ambiguous mutation must remain blocked even if its old
                    // account is no longer the active UI identity.
                    if matches!(result, Err(RunError::UnknownOutcome)) {
                        state.set_uncertain(true);
                    }
                    state.set_status("Phiên Promax đã đổi; không áp dụng kết quả vào phiên mới. Kiểm tra tác vụ ở đúng tài khoản trước khi gửi lại.".into());
                    return;
                }
                match result {
                    Ok(Reply::Definitions(mut definitions)) => {
                        if !definitions.iter().any(|d|d.workflow_id==concat_studio::native_pipeline::WORKFLOW){definitions.insert(0,concat_studio::workflows::WorkflowDefinition{workflow_id:concat_studio::native_pipeline::WORKFLOW.into(),name:"Concat · Sản xuất video native (7 công đoạn)".into(),version:"1.0.0".into(),description:"Nexora Grok → giọng đọc → dựng MP4; BE điều phối từng công đoạn".into(),node_count:7,edge_count:6});}
                        let preferred = crate::studio_production_pages::selected().and_then(|p| p.workflow_id().map(str::to_owned));
                        let index = preferred.as_deref().and_then(|id| definitions.iter().position(|d| d.workflow_id == id)).or_else(||definitions.iter().position(|d|d.workflow_id==concat_studio::native_pipeline::WORKFLOW));
                        state.set_workflow_ids(ModelRc::new(VecModel::from(definitions.iter().map(|d| d.workflow_id.clone().into()).collect::<Vec<slint::SharedString>>())));
                        state.set_workflow_names(ModelRc::new(VecModel::from(definitions.iter().map(|d| d.name.clone().into()).collect::<Vec<slint::SharedString>>())));
                        state.set_workflow_index(index.map_or(-1, |i| i as i32));
                        state.set_status("Đã tải workflow native: Grok qua hồ sơ Nexora, video dọc 9:16. Chọn ảnh nhân vật và hoàn thiện nội dung, rồi bấm Tạo video để mở hoặc dùng lại tab Grok. Chưa gửi chạy nội dung.".into());
                    }
                    Ok(Reply::Source(source, title)) => {
                        if state.get_sources().trim() == source {
                            state.set_status(format!("Đã lấy thông tin nguồn qua Promax: {title}. Chưa tải file hoặc tạo video.").into());
                        }
                    }
                    Ok(Reply::Runs(runs)) => {
                        state.set_history_runs(ModelRc::new(VecModel::from(
                            runs.iter()
                                .filter(|run| {
                                    matches!(
                                        run.status.as_str(),
                                        "completed" | "failed" | "cancelled"
                                    )
                                })
                                .cloned()
                                .map(row)
                                .collect::<Vec<_>>(),
                        )));
                        state.set_runs(ModelRc::new(VecModel::from(
                            runs.into_iter().map(row).collect::<Vec<_>>(),
                        )));
                        state.set_runs_loaded(true);
                        state.set_status(if state.get_uncertain() {
                            t("nativeStudio.unknownOutcome").into()
                        } else {
                            t("nativeStudio.runsLoaded").into()
                        });
                        // Unknown submission outcome is deliberately NOT cleared by
                        // loading a page: absence on page 1 proves nothing.
                    }
                    Ok(Reply::Checked(checked)) => {
                        if plan(&app).ok().as_ref() == Some(&checked) {
                            if let Some(gate) = gate {
                                *gate.lock().unwrap() = Some(checked);
                            }
                            state.set_ready(true);
                            state.set_status(t("nativeStudio.preflightReady").into());
                        } else {
                            state.set_status(t("nativeStudio.preflightAgain").into());
                        }
                    }
                    Ok(Reply::Started(run)) => {
                        state.set_active_run(run.id.clone().into());
                        state.set_runs(ModelRc::new(VecModel::from(vec![row(run)])));
                        state.set_status(t("nativeStudio.submitted").into());
                    }
                    Ok(Reply::Cancelled(run)) => {
                        state.set_cancel_id("".into());
                        state.set_runs(ModelRc::new(VecModel::from(vec![row(run)])));
                        state.set_status(t("nativeStudio.cancelRequested").into());
                    }
                    Ok(Reply::Resumed(run)) => {
                        state.set_resume_id("".into());
                        state.set_runs(ModelRc::new(VecModel::from(vec![row(run)])));
                        state.set_status(t("nativeStudio.resumeRequested").into());
                    }
                    Err(error) => {
                        if error == RunError::UnknownOutcome {
                            state.set_uncertain(true);
                        } else if native_reservation.as_deref().is_some_and(crate::studio_native_production::submission_rejected) {
                            state.set_local_running(false);
                        }
                        state.set_status(message(error).into());
                    }
                }
            });
        });
    if spawned.is_err() {
        state.set_busy(false);
        if failed_spawn_reservation
            .as_deref()
            .is_some_and(crate::studio_native_production::submission_rejected)
        {
            state.set_local_running(false);
        }
        state.set_status(t("nativeStudio.threadError").into());
    }
}
