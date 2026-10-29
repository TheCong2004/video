// SPDX-License-Identifier: AGPL-3.0-or-later
//! Page-owned native port of the existing Grok worker path, not cloud scheduling.
use crate::ui::{App, ProductionStudio};
use concat_studio::{
    pages::{ContentPage, ContentPageStore},
    production::{ProductionClient, ProductionOutput, ProductionPlan},
};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

type Anchor = Arc<Mutex<Option<PathBuf>>>;
#[derive(Clone, PartialEq)]
struct Inputs {
    page: ContentPage,
    anchor: PathBuf,
    image: String,
    expand: String,
    video: String,
}
#[derive(Default)]
struct State {
    pages: Vec<ContentPage>,
    selected: Option<ContentPage>,
    prepared: Option<(Inputs, ProductionPlan)>,
    cancel: Option<Arc<AtomicBool>>,
    output: Option<Arc<ProductionOutput>>,
}
type Shared = Arc<Mutex<State>>;

fn client() -> Result<ProductionClient, String> {
    let dirs =
        concat_host::AppDirs::locate().map_err(|_| "PRODUCTION_RECOVERY_STORAGE_UNAVAILABLE")?;
    Ok(ProductionClient::new(
        crate::studio_browser::client().map_err(|_| "NEXORA_UNAVAILABLE".to_string())?,
    )
    .with_recovery_root(dirs.config.join("studio/production-recovery")))
}
fn failure(error: &str) -> String {
    if error.contains("CANCELLED") { "Đã dừng tác vụ; chưa nhập video." }
    else if error.contains("UNKNOWN") || error.contains("UNCONFIRMED") { "Chưa xác định tác vụ đã dừng hay đã gửi. Không gửi lại để tránh tạo trùng; kiểm tra tác vụ trên Nexora." }
    else if error.contains("AUTH") || error.contains("LOGIN") { "Hồ sơ Nexora chưa đăng nhập hoặc chưa được phép tạo video. Mở đúng hồ sơ để kiểm tra." }
    else if error.contains("PROFILE") { "Hồ sơ Nexora của Page không tồn tại hoặc không sẵn sàng. Kiểm tra hồ sơ đã gán cho kênh." }
    else if error.contains("SOURCE") || error.contains("ANCHOR") { "Ảnh nguồn đã thay đổi hoặc không hợp lệ. Chọn lại ảnh rồi kiểm tra cấu hình." }
    else { "Không hoàn thành được tác vụ. Kiểm tra Nexora, worker Grok và dịch vụ tải kết quả; chưa xác nhận có video để nhập." }.into()
}
fn inputs(app: &App, shared: &Shared, anchor: &Anchor) -> Result<Inputs, String> {
    let s = app.global::<ProductionStudio>();
    if s.get_visual_engine_index() != 1 {
        return Err("Chọn Grok Web Automation cho luồng chuyển từ ArtCraft. Các engine khác chưa có điều phối native ở đây.".into());
    }
    if s.get_aspect_index() != 0 {
        return Err("Luồng Grok cũ tạo ảnh dọc 9:16; không tự đổi tỷ lệ bạn đã chọn.".into());
    }
    let page = shared
        .lock()
        .map_err(|_| "Không đọc được Page.")?
        .selected
        .clone()
        .ok_or("Chọn Page trước khi chạy.")?;
    if page
        .default_workflow_id
        .as_deref()
        .is_some_and(|id| !id.trim().is_empty() && id != "grok_content_pipeline")
    {
        return Err("Workflow mặc định của Page chưa được chuyển sang nhánh Grok này. Không tự đổi sang workflow khác; kiểm tra cấu hình kênh.".into());
    }
    if page
        .browser_profile_id
        .as_deref()
        .is_none_or(|id| id.trim().is_empty())
    {
        return Err(
            "Page chưa được gán hồ sơ Nexora. Thiết lập hồ sơ trong Kênh của bạn trước khi chạy."
                .into(),
        );
    }
    if !s.get_sources().trim().is_empty() || !s.get_asset_urls().trim().is_empty() {
        return Err("Luồng Grok này dùng ảnh tham chiếu trên máy, chưa nhập URL tư liệu. Không gửi chạy khi các nguồn URL chưa được xử lý.".into());
    }
    let anchor = anchor
        .lock()
        .map_err(|_| "Không đọc được ảnh.")?
        .clone()
        .ok_or("Chọn ảnh nhân vật ở bước 4 trước khi chạy.")?;
    Ok(Inputs {
        page,
        anchor,
        image: s.get_image_prompt().to_string(),
        expand: s.get_expand_prompt().to_string(),
        video: s.get_motion_prompt().to_string(),
    })
}
struct ProjectTarget {
    path: String,
    epoch: u64,
}
fn current_project() -> Option<ProjectTarget> {
    let mut path = None;
    crate::host::Shell::with(|shell, _| {
        path = shell
            .studio
            .borrow()
            .session
            .as_ref()
            .map(|s| ProjectTarget {
                path: s.path().to_string(),
                epoch: crate::host::project_epoch(),
            });
    });
    path
}
fn import(app: &App, output: &ProductionOutput, expected_project: &ProjectTarget) -> bool {
    let mut imported = false;
    crate::host::Shell::with(|shell, app| {
        let studio = shell.studio.borrow();
        if crate::host::project_epoch() != expected_project.epoch
            || studio
                .session
                .as_ref()
                .is_none_or(|s| s.path() != expected_project.path)
        {
            return;
        }
        drop(studio);
        shell.studio.borrow_mut().handle(crate::panes::Msg::Media(
            crate::panes::media_bin::MediaMsg::Import(vec![output.path.clone()]),
        ));
        shell.studio.borrow_mut().refresh_art();
        shell.studio.borrow().publish(&app, &shell.models);
        imported = true;
    });
    if !imported {
        app.global::<ProductionStudio>().set_status(
            "Video đã lưu. Mở đúng dự án rồi bấm Nhập video; không nhập vào dự án khác tự động."
                .into(),
        );
    }
    imported
}
fn verify_video(output: &ProductionOutput) -> Result<(), String> {
    use concat_media::FrameSource;
    output.verify_bytes()?;
    let info = concat_media::probe::probe(&output.path).map_err(|_| "OUTPUT_PROBE_FAILED")?;
    let video = info.require_video().map_err(|_| "OUTPUT_NO_VIDEO")?;
    if video.width > 4096
        || video.height > 8192
        || u64::from(video.width) * u64::from(video.height) > 16_777_216
        || video.width == 0
        || video.height == 0
        || ((video.width as f64 / video.height as f64) - (9.0 / 16.0)).abs() > 0.02
    {
        return Err("OUTPUT_INVALID_DIMENSIONS".into());
    }
    if info
        .duration
        .is_none_or(|d| !d.as_f64().is_finite() || d.as_f64() <= 0.0)
    {
        return Err("OUTPUT_INVALID_DURATION".into());
    }
    let options = concat_media::DecodeOptions::default()
        .limited_to(1)
        .threaded(1)
        .in_software();
    let mut decoder =
        concat_media::Decoder::open(&output.path, &options).map_err(|_| "OUTPUT_DECODE_FAILED")?;
    if decoder
        .next_frame()
        .map_err(|_| "OUTPUT_DECODE_FAILED")?
        .is_none()
    {
        return Err("OUTPUT_EMPTY_VIDEO".into());
    }
    Ok(())
}
pub(crate) fn bind(app: &App, anchor: Anchor) {
    let shared = Arc::new(Mutex::new(State::default()));
    let weak = app.as_weak();
    let data = shared.clone();
    app.global::<ProductionStudio>().on_load_pages(move || {
        let Some(app)=weak.upgrade() else {return}; let s=app.global::<ProductionStudio>();
        if s.get_busy() {return} s.set_busy(true); s.invoke_invalidate();
        let weak=app.as_weak(); let data=data.clone();
        let task=std::thread::Builder::new().name("production-pages".into()).spawn(move || {
            let result=concat_host::AppDirs::locate().and_then(|dirs| ContentPageStore::new(&dirs.config).list(false));
            let _=weak.upgrade_in_event_loop(move |app| {
                let s=app.global::<ProductionStudio>(); s.set_busy(false);
                match result {
                    Ok(pages)=>{
                        let mut names=vec!["Chọn Page để sản xuất…".into()];
                        names.extend(pages.iter().map(|p| p.name.clone().into()));
                        s.set_page_names(ModelRc::new(VecModel::from(names))); s.set_page_index(0);
                        let mut state=data.lock().unwrap(); state.pages=pages; state.selected=None; state.prepared=None;
                        s.set_page_summary("Chọn Page; cấu hình được chụp riêng cho từng tác vụ.".into());
                    },
                    Err(_)=>s.set_status("Không đọc được danh sách Page. Kiểm tra Kênh của bạn; không ghi đè dữ liệu cũ.".into())
                }
            });
        });
        if task.is_err() {s.set_busy(false); s.set_status("Không mở được tác vụ tải Page.".into());}
    });
    let weak = app.as_weak();
    let data = shared.clone();
    app.global::<ProductionStudio>()
        .on_apply_page(move |index| {
            let Some(app) = weak.upgrade() else { return };
            let s = app.global::<ProductionStudio>();
            if s.get_busy() {
                return;
            }
            s.invoke_invalidate();
            let mut state = data.lock().unwrap();
            state.prepared = None;
            state.selected = usize::try_from(index - 1)
                .ok()
                .and_then(|i| state.pages.get(i).cloned());
            if let Some(page) = &state.selected {
                s.set_image_prompt(page.default_image_prompt.clone().unwrap_or_default().into());
                s.set_expand_prompt(
                    page.default_expand_9_16_prompt
                        .clone()
                        .unwrap_or_default()
                        .into(),
                );
                s.set_motion_prompt(page.default_video_prompt.clone().unwrap_or_default().into());
                s.set_visual_engine_index(1);
                s.set_aspect_index(match page.default_aspect_ratio.as_deref() {
                    Some("16:9") => 1,
                    Some("1:1") => 2,
                    _ => 0,
                });
                s.set_page_summary(
                    format!(
                        "{} · Hồ sơ Nexora: {}\nLưu tại: {}",
                        page.name,
                        page.browser_profile_id.as_deref().unwrap_or("chưa gán"),
                        page.output_root
                    )
                    .into(),
                );
                s.set_workflow_ids(ModelRc::new(VecModel::from(vec![
                    "grok_content_pipeline".into(),
                ])));
                s.set_workflow_names(ModelRc::new(VecModel::from(vec![
                    "Grok · tạo ảnh → 9:16 → video".into(),
                ])));
                s.set_workflow_index(0);
            } else {
                s.set_page_summary("Chưa chọn Page. Không thể chạy tác vụ.".into());
            }
        });
    let weak = app.as_weak();
    let data = shared.clone();
    let selected = anchor.clone();
    app.global::<ProductionStudio>().on_check_plan(move || {
        let Some(app)=weak.upgrade() else {return}; let s=app.global::<ProductionStudio>(); if s.get_busy() || s.get_uncertain() {return}
        s.invoke_invalidate(); data.lock().unwrap().prepared=None;
        let input=match inputs(&app,&data,&selected) {Ok(v)=>v,Err(e)=>{s.set_status(e.into());return}};
        s.set_busy(true); let weak=app.as_weak(); let data=data.clone(); let selected=selected.clone();
        let task=std::thread::Builder::new().name("production-preflight".into()).spawn(move || {
            let result=(||{
                let plan=ProductionPlan::new(input.page.clone(),input.anchor.clone(),&input.image,&input.expand,&input.video)?;
                client()?.preflight(&plan)?; Ok::<_,String>(plan)
            })();
            let _=weak.upgrade_in_event_loop(move |app| {
                let s=app.global::<ProductionStudio>();s.set_busy(false);
                match result {
                    Ok(plan) if inputs(&app,&data,&selected).ok().as_ref()==Some(&input)=>{
                        data.lock().unwrap().prepared=Some((input,plan)); s.set_ready(true);
                        s.set_status("Đã kiểm tra Page, ảnh và hồ sơ Nexora. Chưa gửi tạo nội dung; bấm Tạo video để xác nhận chạy.".into());
                    },
                    Ok(_)=>s.set_status("Cấu hình đã đổi; kiểm tra lại trước khi chạy.".into()),
                    Err(e)=>s.set_status(failure(&e).into())
                }
            });
        });
        if task.is_err() {s.set_busy(false);s.set_status("Không mở được tác vụ kiểm tra.".into());}
    });
    let weak = app.as_weak();
    let data = shared.clone();
    let selected = anchor;
    app.global::<ProductionStudio>().on_start_run(move || {
        let Some(app) = weak.upgrade() else { return };
        let s = app.global::<ProductionStudio>();
        if s.get_busy() || s.get_uncertain() || !s.get_ready() || !s.get_confirm_start() {
            return;
        }
        let current = inputs(&app, &data, &selected).ok();
        let prepared = data.lock().unwrap().prepared.take();
        s.invoke_invalidate();
        let Some((input, plan)) = prepared.filter(|(input, _)| Some(input) == current.as_ref())
        else {
            s.set_status("Kiểm tra lại cấu hình trước khi chạy.".into());
            return;
        };
        let expected_project = current_project();
        s.set_active_run(plan.job_id().into());
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut state = data.lock().unwrap();
            state.cancel = Some(cancel.clone());
            state.output = None;
        }
        s.set_busy(true);
        s.set_local_running(true);
        s.set_local_has_output(false);
        s.set_local_image_status("Chưa chạy".into());
        s.set_local_expand_status("Chưa chạy".into());
        s.set_local_video_status("Chưa chạy".into());
        s.set_local_progress("Đang chuẩn bị tác vụ Nexora…".into());
        let weak = app.as_weak();
        let progress = app.as_weak();
        let data_for_task = data.clone();
        let task = std::thread::Builder::new()
            .name("production-native-run".into())
            .spawn(move || {
                let data = data_for_task;
                let result = (|| {
                    let output = client()?.run(plan, cancel.clone(), move |event| {
                        let stage = event.stage;
                        let status = if event.state == "SUCCEEDED" { "Đã nhận và kiểm tra file" } else { "Đang chạy" };
                        use concat_studio::production::ProductionStage;
                        let label=match stage {
                            ProductionStage::ImageEdit=>"Tạo ảnh",
                            ProductionStage::Expand916=>"Mở rộng khung hình",
                            ProductionStage::VideoGenerate=>"Tạo video",
                            ProductionStage::LocalSave=>"Lưu video"
                        };
                        let text = format!("{label} · {status}");
                        let _ = progress.upgrade_in_event_loop(move |app| {
                            let s=app.global::<ProductionStudio>();
                            use concat_studio::production::ProductionStage;
                            match stage {
                                ProductionStage::ImageEdit=>s.set_local_image_status(status.into()),
                                ProductionStage::Expand916=>s.set_local_expand_status(status.into()),
                                ProductionStage::VideoGenerate=>s.set_local_video_status(status.into()),
                                ProductionStage::LocalSave=>{}
                            }
                            s.set_local_progress(text.into());
                        });
                    })?;
                    verify_video(&output)?;
                    if cancel.load(Ordering::Acquire) {return Err("CANCELLED".into());}
                    Ok::<_, String>(output)
                })();
                let _ = weak.upgrade_in_event_loop(move |app| {
                    let s = app.global::<ProductionStudio>();
                    s.set_busy(false);
                    s.set_local_running(false);
                    let cancelled=data.lock().unwrap().cancel.take().is_some_and(|c|c.load(Ordering::Acquire));
                    let result=if cancelled && result.is_ok(){Err("CANCELLED".to_string())}else{result};
                    match result {
                        Ok(output) => {
                            s.set_active_run(output.job_id.clone().into());
                            s.set_local_has_output(true);
                            s.set_local_progress(
                                format!(
                                    "Đã nhận và kiểm tra video · Page {}\n{}",
                                    input.page.name,
                                    output.path.display()
                                )
                                .into(),
                            );
                            let output = Arc::new(output);
                            data.lock().unwrap().output = Some(output.clone());
                            if let Some(expected_project) = expected_project {
                                if import(&app, &output, &expected_project) {
                                    s.set_status("Đã gửi video vào hàng nhập của dự án. Concat sẽ thông báo khi nhập xong.".into());
                                }
                            } else {
                                s.set_status(
                                    "Video đã lưu. Mở dự án rồi chọn Nhập video để đưa vào Concat."
                                        .into(),
                                );
                            }
                        }
                        Err(e) => {
                            if s.get_local_image_status() == "Đang chạy" {
                                s.set_local_image_status("Đã dừng / có lỗi".into());
                            }
                            if s.get_local_expand_status() == "Đang chạy" {
                                s.set_local_expand_status("Đã dừng / có lỗi".into());
                            }
                            if s.get_local_video_status() == "Đang chạy" {
                                s.set_local_video_status("Đã dừng / có lỗi".into());
                            }
                            let text = failure(&e);
                            s.set_local_progress(text.clone().into());
                            s.set_status(text.into());
                            if e.contains("UNKNOWN") || e.contains("UNCONFIRMED") {
                                s.set_uncertain(true);
                            }
                        }
                    }
                });
            });
        if task.is_err() {
            s.set_busy(false);
            s.set_local_running(false);
            data.lock().unwrap().cancel = None;
            s.set_status("Không mở được tác vụ sản xuất; chưa gửi yêu cầu.".into());
        }
    });
    let data = shared.clone();
    app.global::<ProductionStudio>().on_cancel_local(move || {
        if let Some(cancel) = &data.lock().unwrap().cancel {
            cancel.store(true, Ordering::Release);
        }
    });
    let weak = app.as_weak();
    let data = shared;
    app.global::<ProductionStudio>().on_import_output(move || {
        let Some(app) = weak.upgrade() else { return };
        let s = app.global::<ProductionStudio>();
        if s.get_busy() {
            return;
        }
        let Some(expected_project) = current_project() else {
            s.set_status("Mở dự án Concat trước khi nhập video.".into());
            return;
        };
        let output = data.lock().unwrap().output.clone();
        let Some(output) = output else { return };
        s.set_busy(true);
        let weak = app.as_weak();
        let task = std::thread::Builder::new()
            .name("production-import-check".into())
            .spawn(move || {
                let result = verify_video(&output);
                let _ = weak.upgrade_in_event_loop(move |app| {
                    let s = app.global::<ProductionStudio>();
                    s.set_busy(false);
                    if result.is_ok() {
                        if import(&app, &output, &expected_project) {
                            s.set_status("Đã gửi video vào hàng nhập của dự án. Concat sẽ thông báo khi nhập xong.".into());
                        }
                    } else {
                        s.set_local_has_output(false);
                        s.set_status(
                            "Video đã thay đổi hoặc không còn hợp lệ; không nhập vào dự án.".into(),
                        );
                    }
                });
            });
        if task.is_err() {
            s.set_busy(false);
            s.set_status("Không mở được tác vụ xác minh video.".into());
        }
    });
}
