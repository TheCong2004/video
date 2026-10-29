// SPDX-License-Identifier: AGPL-3.0-or-later
//! Concat-local avatar UI. Private paths/credentials never enter UI models.
use crate::ui::{App, PromaxAvatarRow, StudioPromaxAvatars};
use concat_host::studio_promax_avatars::{AvatarClient, AvatarError, AvatarJob, Readiness};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{cell::RefCell, path::PathBuf, time::Duration};

thread_local! {
    static CHOSEN_VIDEO: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    static POLLER: RefCell<Option<slint::Timer>> = const { RefCell::new(None) };
}

fn client() -> Result<AvatarClient, String> {
    let dirs = concat_host::AppDirs::locate().map_err(|_| "Không mở được dữ liệu Concat.")?;
    concat_host::studio_local_avatars::client(&dirs.data.join("local-avatars")).map_err(message)
}

fn message(error: AvatarError) -> String {
    match error {
        AvatarError::Configuration => "Thiếu bộ quét local của Concat. Chạy scripts/install-slive-avatar-runtime.ps1 hoặc cài bản có kèm AvatarRuntime.",
        AvatarError::Authentication => "Phiên runtime local không hợp lệ. Đóng và mở lại Concat; không cần đăng nhập hoặc API key.",
        AvatarError::Input => "Video/tên bị từ chối: chọn MP4/WebM tối đa 25 MB, dài 1–30 giây; tên 1–90 ký tự. Kiểm tra giới hạn thư viện trên Concat local.",
        AvatarError::Unavailable => "Không khởi động được bộ quét local. Kiểm tra Python/model của Concat rồi bấm Tải lại. Không cần Promax hoặc ArtCraft.",
        AvatarError::NotReady => "Concat local chưa sẵn sàng, đang bận hoặc video chưa có. Bấm Kiểm tra Concat local / Tải lại, không gửi lặp video.",
        AvatarError::InvalidResponse => "Dữ liệu nhân vật local không hợp lệ. Kiểm tra bộ quét đã đi kèm đúng bản Concat.",
        AvatarError::UnknownOutcome => "Chưa xác nhận được kết quả gửi. Bấm Tải lại danh sách để kiểm tra job trước khi gửi lại; app không tự gửi lặp.",
    }.into()
}

enum Action {
    Refresh,
    Poll,
    Import(String, PathBuf),
    Preview(String),
    Live(String, String, u64),
    Delete(String),
}
enum Reply {
    Jobs(Option<Readiness>, Vec<AvatarJob>),
    Imported(AvatarJob),
    Preview(Option<image::RgbaImage>),
    Live(PathBuf, String, String, u64),
    Deleted(Vec<AvatarJob>),
}

fn selected_id(state: &StudioPromaxAvatars) -> Option<String> {
    usize::try_from(state.get_selected_index())
        .ok()
        .and_then(|index| state.get_ready_ids().row_data(index))
        .map(|id| id.to_string())
}

fn decode_preview(bytes: Vec<u8>) -> Option<image::RgbaImage> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    Some(reader.decode().ok()?.to_rgba8())
}

fn apply_jobs(app: &App, jobs: Vec<AvatarJob>) {
    let state = app.global::<StudioPromaxAvatars>();
    let previous = selected_id(&state);
    let preferred = state.get_last_import_id().to_string();
    let mut rows = Vec::new();
    let mut ids: Vec<slint::SharedString> = Vec::new();
    let mut options: Vec<slint::SharedString> = Vec::new();
    state.set_scanning(jobs.iter().any(|job| job.status == "processing"));
    for job in jobs {
        if job.id == preferred && job.status != "processing" {
            state.set_last_import_id("".into());
            state.set_error(job.status != "ready");
            state.set_status(if job.status == "ready" {
                format!("Đã quét xong {} · {} khung hình. Chọn Phát trên LIVE để sử dụng.", job.name, job.frames).into()
            } else { "Quét không thành công. Chọn video 1–30 giây, một người và khuôn mặt rõ rồi thử lại.".into() });
        }
        if job.status == "ready" {
            ids.push(job.id.clone().into());
            options.push(job.name.clone().into());
        }
        rows.push(PromaxAvatarRow {
            deletable: job.status != "processing",
            id: job.id.into(), name: job.name.into(),
            state: match job.status.as_str() { "ready" => "Sẵn sàng", "processing" => "Đang quét…", _ => "Không thành công" }.into(),
            details: if job.status == "ready" { format!("{} khung hình · {:.1} giây · Chuyển động gốc, tiếng gốc đã tắt", job.frames, job.duration_seconds) }
                else if job.status == "processing" { "Đang tách khung hình và nhận diện khuôn mặt. Kết quả tự cập nhật, không cần gửi lại.".into() }
                else if job.error_code.as_deref() == Some("PROCESS_INTERRUPTED") { "Backend đã khởi động lại khi đang quét. Video này chưa sẵn sàng.".into() }
                else { "Không tạo được dữ liệu khuôn mặt. Kiểm tra video và runtime Concat local.".into() }.into(),
        });
    }
    let index = ids
        .iter()
        .position(|id| !preferred.is_empty() && id.as_str() == preferred)
        .or_else(|| {
            ids.iter()
                .position(|id| Some(id.as_str()) == previous.as_deref())
        })
        .unwrap_or(0);
    state.set_jobs(ModelRc::new(VecModel::from(rows)));
    state.set_ready_ids(ModelRc::new(VecModel::from(ids)));
    state.set_ready_options(ModelRc::new(VecModel::from(options)));
    state.set_selected_index(index as i32);
    let new_id = selected_id(&state);
    if new_id.is_none() {
        state.set_preview(slint::Image::default());
    } else if new_id != previous || state.get_preview().size().width == 0 {
        start(app, Action::Preview(new_id.unwrap()));
    }
}

fn start(app: &App, action: Action) {
    let state = app.global::<StudioPromaxAvatars>();
    if state.get_busy() {
        return;
    }
    state.set_busy(true);
    match &action {
        Action::Refresh => {
            state.set_error(false);
            state.set_status("Đang kiểm tra runtime và tải danh sách từ Concat local…".into());
        }
        Action::Import(_, _) => {
            state.set_error(false);
            state.set_status("Đang gửi video đã chọn tới Concat local…".into());
        }
        Action::Live(_, _, _) => {
            state.set_error(false);
            state.set_status("Đang tải đúng video nhân vật từ Concat local để phát LIVE…".into());
        }
        Action::Delete(_) => {
            state.set_error(false);
            state.set_status("Đang chuyển bản đã chọn vào thư mục khôi phục…".into());
        }
        _ => {}
    }
    let motion_epoch = if let Action::Live(_, _, epoch) = &action {
        Some(*epoch)
    } else {
        None
    };
    let weak = app.as_weak();
    let spawned = std::thread::Builder::new().name("promax-avatar-ui".into()).spawn(move || {
        let result: Result<Reply, String> = (|| {
            let client = client()?;
            match action {
                Action::Refresh => Ok(Reply::Jobs(Some(client.readiness().map_err(message)?), client.list().map_err(message)?)),
                Action::Poll => Ok(Reply::Jobs(None, client.list().map_err(message)?)),
                Action::Delete(id) => {
                    client.delete(&id).map_err(message)?;
                    Ok(Reply::Deleted(client.list().map_err(message)?))
                }
                Action::Import(name, path) => {
                    if !client.readiness().map_err(message)?.ready { return Err("Runtime Concat local chưa sẵn sàng. Bấm Kiểm tra Concat local để xem phần còn thiếu.".into()); }
                    Ok(Reply::Imported(client.import_video(&name, &path).map_err(message)?))
                }
                Action::Preview(id) => Ok(Reply::Preview(decode_preview(client.media(&id, true).map_err(message)?))),
                Action::Live(id, name, epoch) => {
                    let bytes = client.media(&id, false).map_err(message)?;
                    use sha2::{Digest, Sha256};
                    let digest = format!("{:x}", Sha256::digest(&bytes));
                    let dirs = concat_host::AppDirs::locate().map_err(|_| "Không mở được thư mục dữ liệu Concat.")?;
                    let folder = dirs.data.join("promax-avatar-video");
                    std::fs::create_dir_all(&folder).map_err(|_| "Không tạo được thư mục video.")?;
                    let target = folder.join(format!("{digest}.mp4"));
                    // Re-authorize/download before using any local cached artifact.
                    if !target.is_file() {
                        use std::io::Write;
                        let partial = folder.join(format!("{digest}-{}-{epoch}.part", std::process::id()));
                        let saved = (|| {
                            let mut file = std::fs::OpenOptions::new().create_new(true).write(true).open(&partial)?;
                            file.write_all(&bytes)?;
                            file.flush()?;
                            drop(file);
                            std::fs::rename(&partial, &target)
                        })();
                        if saved.is_err() { let _ = std::fs::remove_file(&partial); return Err("Không lưu được video Concat local trên máy.".into()); }
                    }
                    Ok(Reply::Live(target, id, name, epoch))
                }
            }
        })();
        let _ = weak.upgrade_in_event_loop(move |app| {
            let state = app.global::<StudioPromaxAvatars>();
            state.set_busy(false);
            match result {
                Ok(Reply::Jobs(readiness, jobs)) => {
                    state.set_loaded(true);
                    if let Some(readiness) = readiness {
                        state.set_ready(readiness.ready);
                        state.set_status(if readiness.ready { "Concat local đã sẵn sàng quét video. Chọn video và đặt tên để bắt đầu.".into() }
                            else { format!("Bộ quét chưa sẵn sàng: {}. Chạy scripts/install-slive-avatar-runtime.ps1 trong Concat.", readiness.missing.join(", ")).into() });
                    }
                    apply_jobs(&app, jobs);
                }
                Ok(Reply::Imported(job)) => {
                    state.set_last_import_id(job.id.into());
                    state.set_scanning(true);
                    state.set_status("Concat local đã nhận video, đang quét. Bạn có thể chuyển mục; app tự tải kết quả, không tự phát LIVE.".into());
                    start(&app, Action::Poll);
                }
                Ok(Reply::Deleted(jobs)) => {
                    state.set_delete_id("".into());
                    state.set_error(false);
                    state.set_status("Đã xóa bản đã chọn khỏi thư viện. Video gốc không đổi; bản đã xóa nằm trong local-avatars/library/.trash để khôi phục.".into());
                    apply_jobs(&app, jobs);
                }
                Ok(Reply::Preview(pixels)) => {
                    state.set_preview(pixels.map(|pixels| slint::Image::from_rgba8(slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(pixels.as_raw(), pixels.width(), pixels.height()))).unwrap_or_default());
                }
                Ok(Reply::Live(path, id, name, epoch)) => {
                    if crate::studio_live_video::finish_promax_motion(&app, epoch, Ok(path), &id, &name) {
                        state.set_status(format!("Đã chuyển {} sang đầu ra LIVE · phát chuyển động gốc, không khớp môi theo câu AI mới.", name).into());
                    }
                }
                Err(error) => {
                    state.set_error(true);
                    state.set_status(error.clone().into());
                    state.set_scanning(false);
                    state.set_loaded(false);
                    if let Some(epoch) = motion_epoch { crate::studio_live_video::finish_promax_motion(&app, epoch, Err(error), "", ""); }
                }
            }
        });
    });
    if spawned.is_err() {
        state.set_busy(false);
        state.set_error(true);
        state.set_status("Không khởi động được tác vụ Concat local. Thử tải lại.".into());
        if let Some(epoch) = motion_epoch {
            crate::studio_live_video::finish_promax_motion(
                app,
                epoch,
                Err("Không khởi động được tác vụ tải video.".into()),
                "",
                "",
            );
        }
    }
}

pub(crate) fn bind(app: &App) {
    let state = app.global::<StudioPromaxAvatars>();
    state.set_endpoint("Concat local · tự khởi động · video không gửi lên cloud".into());
    let weak = app.as_weak();
    state.on_delete_avatar(move |id| {
        let Some(app) = weak.upgrade() else { return };
        let state = app.global::<StudioPromaxAvatars>();
        if state.get_busy() || state.get_delete_id() != id { return; }
        let allowed = state.get_jobs().iter().any(|job| job.id == id && job.deletable);
        if !allowed || state.get_scanning() || state.get_live_active_id() == id {
            state.set_status("Dừng video đang LIVE hoặc chờ quét xong trước khi xóa bản này.".into());
            return;
        }
        start(&app, Action::Delete(id.to_string()));
    });
    let weak = app.as_weak();
    state.on_refresh(move || {
        if let Some(app) = weak.upgrade() {
            start(&app, Action::Refresh);
        }
    });
    let weak = app.as_weak();
    state.on_poll(move || {
        if let Some(app) = weak.upgrade() {
            start(&app, Action::Poll);
        }
    });
    let weak = app.as_weak();
    state.on_import_video(move || {
        let Some(app) = weak.upgrade() else { return };
        let state = app.global::<StudioPromaxAvatars>();
        let name = state.get_name().trim().to_owned();
        if name.is_empty() || name.chars().count() > 90 || name.chars().any(char::is_control) {
            state.set_status("Tên nhân vật phải có 1–90 ký tự.".into());
            return;
        }
        if let Some(path) = CHOSEN_VIDEO.with(|slot| slot.borrow().clone()) {
            start(&app, Action::Import(name, path));
        }
    });
    let weak = app.as_weak();
    state.on_select_avatar(move |index| {
        let Some(app) = weak.upgrade() else { return };
        let state = app.global::<StudioPromaxAvatars>();
        if state.get_busy() {
            return;
        }
        state.set_selected_index(index);
        state.set_preview(slint::Image::default());
        if let Some(id) = selected_id(&state) {
            start(&app, Action::Preview(id));
        }
    });
    let weak = app.as_weak();
    state.on_use_live(move || {
        let Some(app) = weak.upgrade() else { return };
        let state = app.global::<StudioPromaxAvatars>();
        if state.get_busy() {
            return;
        }
        let Some(id) = selected_id(&state) else {
            return;
        };
        let Some(name) = state
            .get_ready_options()
            .row_data(state.get_selected_index() as usize)
        else {
            return;
        };
        let Some(epoch) = crate::studio_live_video::begin_promax_motion(&app) else {
            return;
        };
        start(&app, Action::Live(id, name.to_string(), epoch));
    });
    let weak = app.as_weak();
    state.on_choose_video(move || {
        let Some(app) = weak.upgrade() else { return };
        let state = app.global::<StudioPromaxAvatars>();
        if state.get_busy() { return; }
        state.set_busy(true);
        let weak = app.as_weak();
        let spawned = std::thread::Builder::new().name("promax-avatar-file".into()).spawn(move || {
            let path = rfd::FileDialog::new().set_title("Video người thật · MP4/WebM tối đa 25 MB, 1–30 giây").add_filter("Video", &["mp4", "webm"]).pick_file();
            let _ = weak.upgrade_in_event_loop(move |app| {
                let state = app.global::<StudioPromaxAvatars>();
                state.set_busy(false);
                if let Some(path) = path {
                    state.set_file_label(path.file_name().unwrap_or_default().to_string_lossy().into_owned().into());
                    if state.get_name().is_empty() { state.set_name(path.file_stem().unwrap_or_default().to_string_lossy().chars().filter(|c| !c.is_control()).take(90).collect::<String>().into()); }
                    CHOSEN_VIDEO.with(|slot| *slot.borrow_mut() = Some(path));
                    state.set_status("Video mới chỉ được chọn trên máy, chưa gửi. Bấm Gửi video & quét để tiếp tục.".into());
                }
            });
        });
        if spawned.is_err() { state.set_busy(false); }
    });
    let timer = slint::Timer::default();
    let weak = app.as_weak();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_secs(4),
        move || {
            let Some(app) = weak.upgrade() else { return };
            let state = app.global::<StudioPromaxAvatars>();
            if state.get_scanning() && !state.get_busy() {
                start(&app, Action::Poll);
            }
        },
    );
    POLLER.with(|slot| *slot.borrow_mut() = Some(timer));
}

/// Explicit local smoke: real UI callbacks -> Promax -> prepared media -> both
/// LIVE components. No TikTok connection, AI request or social publishing.
pub(crate) fn smoke(app: &App, video: PathBuf) -> Result<(), slint::PlatformError> {
    use crate::ui::StudioLive;
    use std::{cell::Cell, collections::HashSet, rc::Rc};
    if !video.is_file() {
        return Err(slint::PlatformError::Other(
            "PROMAX_SMOKE_VIDEO_MISSING".into(),
        ));
    }
    CHOSEN_VIDEO.with(|slot| *slot.borrow_mut() = Some(video));
    let state = app.global::<StudioPromaxAvatars>();
    state.set_name("Concat local native smoke".into());
    state.set_file_label("Explicit smoke fixture.mp4".into());
    state.invoke_refresh();
    let phase = Rc::new(Cell::new(0u8));
    let finished = Rc::new(Cell::new(false));
    let success = finished.clone();
    let hashes = Rc::new(RefCell::new(HashSet::new()));
    let weak = app.as_weak();
    let deadline = std::time::Instant::now() + Duration::from_secs(600);
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(50),
        move || {
            let Some(app) = weak.upgrade() else { return };
            let state = app.global::<StudioPromaxAvatars>();
            if state.get_error() || std::time::Instant::now() >= deadline {
                eprintln!(
                    "PROMAX_AVATAR_UI_SMOKE_FAILED phase={}: {}",
                    phase.get(),
                    state.get_status()
                );
                let _ = slint::quit_event_loop();
                return;
            }
            match phase.get() {
                0 if !state.get_busy() && state.get_loaded() && state.get_ready() => {
                    phase.set(1);
                    state.invoke_import_video();
                }
                1 if !state.get_busy()
                    && !state.get_scanning()
                    && state.get_last_import_id().is_empty()
                    && state.get_ready_ids().row_count() > 0
                    && state.get_preview().size().width > 0 =>
                {
                    phase.set(2);
                    app.global::<StudioLive>().invoke_toggle_output_window();
                    state.invoke_use_live();
                }
                2 => {
                    let live = app.global::<StudioLive>();
                    let frame = live.get_video_frame();
                    if live.get_motion_playing() && frame.size().width > 0 {
                        if crate::studio_live::output_frame().as_ref() != Some(&frame) {
                            return;
                        }
                        if let Some(pixels) = frame.to_rgba8() {
                            use std::hash::{Hash, Hasher};
                            let mut hash = std::collections::hash_map::DefaultHasher::new();
                            for byte in pixels.as_bytes().iter().step_by(97) {
                                byte.hash(&mut hash);
                            }
                            hashes.borrow_mut().insert(hash.finish());
                        }
                        if hashes.borrow().len() >= 3 && !state.get_live_active_id().is_empty() {
                            let id = state.get_live_active_id();
                            state.set_delete_id(id.clone());
                            state.invoke_delete_avatar(id);
                            if state.get_busy() {
                                eprintln!("LOCAL_AVATAR_DELETE_GUARD_FAILED");
                                let _ = slint::quit_event_loop();
                                return;
                            }
                            live.invoke_stop_motion();
                            live.invoke_toggle_output_window();
                            phase.set(3);
                            state.invoke_delete_avatar(state.get_delete_id());
                        }
                    }
                }
                3 if !state.get_busy() && state.get_jobs().row_count() == 0
                    && state.get_ready_ids().row_count() == 0
                    && state.get_preview().size().width == 0
                    && state.get_delete_id().is_empty() => {
                    success.set(true);
                    let _ = slint::quit_event_loop();
                }
                _ => {}
            }
        },
    );
    slint::run_event_loop_until_quit()?;
    drop(timer);
    if finished.get() {
        println!(
            "PROMAX_AVATAR_UI_SMOKE=PASS upload=real preparation=real preview=real live_motion=real clean_output=matched archive=real live_guard=blocked"
        );
        Ok(())
    } else {
        Err(slint::PlatformError::Other(
            "PROMAX_AVATAR_UI_SMOKE_FAILED".into(),
        ))
    }
}
