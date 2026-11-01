// SPDX-License-Identifier: AGPL-3.0-or-later
//! Selected-video queue, provenance deduplication and project-safe import.
use crate::ui::{App, LuxChoice, StudioLux};
use concat_studio::{
    lux::{self, LuxDownloadOptions, LuxProgressEvent, LuxVideoInfo},
    video_library,
};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

struct Item {
    video: LuxVideoInfo,
    selected: bool,
    exists: bool,
    detail: String,
    preview: crate::studio_video_preview::Prepared,
    preview_ready: bool,
}
#[derive(Default)]
struct State {
    video: Option<LuxVideoInfo>,
    items: Vec<Item>,
    cancel: Arc<AtomicBool>,
    generation: u64,
    preview_epoch: u64,
}
type Shared = Arc<Mutex<State>>;
fn active(app: &App) -> bool {
    let ui = app.global::<StudioLux>();
    ui.get_busy() || ui.get_downloading()
}
fn clear_info(app: &App, state: &mut State) {
    state.video = None;
    state.items.clear();
    state.generation += 1;
    state.preview_epoch += 1;
    let ui = app.global::<StudioLux>();
    ui.set_has_info(false);
    ui.set_video_title("".into());
    ui.set_video_site("".into());
    ui.set_stream_options(ModelRc::new(VecModel::default()));
    ui.set_stream_index(0);
    ui.set_choices(ModelRc::new(VecModel::default()));
    ui.set_selected_count(0);
    ui.set_total_count(0);
    ui.set_existing_count(0);
    ui.set_choice_page(0);
    ui.set_choice_pages(1);
    ui.set_done(false);
    ui.set_progress(0.0);
    ui.set_error("".into());
    ui.set_status("".into());
}
fn publish(app: &App, state: &State) {
    let ui = app.global::<StudioLux>();
    ui.set_total_count(state.items.len() as i32);
    ui.set_existing_count(state.items.iter().filter(|i| i.exists).count() as i32);
    let visible = state
        .items
        .iter()
        .filter(|i| !ui.get_hide_existing() || !i.exists)
        .count();
    let pages = visible.div_ceil(20).max(1);
    let page = (ui.get_choice_page().max(0) as usize).min(pages - 1);
    ui.set_choice_page(page as i32);
    ui.set_choice_pages(pages as i32);
    ui.set_selected_count(
        state
            .items
            .iter()
            .filter(|i| i.selected && !i.exists)
            .count() as i32,
    );
    ui.set_choices(ModelRc::new(VecModel::from(
        state
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| !ui.get_hide_existing() || !item.exists)
            .skip(page * 20)
            .take(20)
            .map(|(index, item)| LuxChoice {
                index: index as i32,
                title: item.video.title.clone().into(),
                detail: if item.exists {
                    "Đã có trên máy · bỏ qua tải trùng".into()
                } else if item.detail.is_empty() {
                    item.video.url.clone().into()
                } else {
                    item.detail.clone().into()
                },
                selected: item.selected,
                exists: item.exists,
                thumbnail: item.preview.image(),
                duration: if item.preview.duration.is_empty() {
                    crate::studio_video_preview::duration(item.video.duration_seconds as f64).into()
                } else {
                    item.preview.duration.clone().into()
                },
            })
            .collect::<Vec<_>>(),
    )));
}
fn prepare_choices(app: &App, shared: Shared) {
    let ui = app.global::<StudioLux>();
    let visible: Vec<_> = ui
        .get_choices()
        .iter()
        .map(|row| row.index.max(0) as usize)
        .collect();
    let (generation, epoch, work) = {
        let Ok(mut s) = shared.lock() else { return };
        s.preview_epoch += 1;
        let work: Vec<_> = visible
            .into_iter()
            .filter_map(|index| {
                s.items
                    .get(index)
                    .filter(|i| !i.preview_ready)
                    .map(|i| (index, i.video.clone()))
            })
            .collect();
        (s.generation, s.preview_epoch, work)
    };
    if work.is_empty() {
        return;
    }
    let reply = app.as_weak();
    std::thread::spawn(move || {
        let Ok(dirs) = concat_host::AppDirs::locate() else {
            return;
        };
        for (index, video) in work {
            if !shared
                .lock()
                .is_ok_and(|s| s.generation == generation && s.preview_epoch == epoch)
            {
                break;
            }
            let preview = crate::studio_video_preview::remote(
                &video.thumbnail,
                &video.url,
                video.duration_seconds,
                &dirs.data.join("studio/video-thumbnails/online"),
            );
            let state = shared.clone();
            let _ = reply.upgrade_in_event_loop(move |app| {
                let Ok(mut s) = state.lock() else { return };
                if s.generation != generation || s.preview_epoch != epoch {
                    return;
                }
                if let Some(item) = s.items.get_mut(index).filter(|i| i.video.url == video.url) {
                    item.preview = preview;
                    item.preview_ready = true;
                }
                // Update only this visible tile, never rebuild the whole gallery per image.
                let model = app.global::<StudioLux>().get_choices();
                for row in 0..model.row_count() {
                    if let Some(mut view) = model.row_data(row).filter(|v| v.index == index as i32)
                    {
                        if let Some(item) = s.items.get(index) {
                            view.thumbnail = item.preview.image();
                            view.duration = item.preview.duration.clone().into();
                            model.set_row_data(row, view);
                        }
                        break;
                    }
                }
            });
        }
    });
}
fn is_account_url(text: &str) -> bool {
    let (_, is_account): (String, bool) = concat_studio::lux::normalize_and_classify_url(text);
    is_account
}
fn fetch(app: &App, shared: Shared, browser: bool) {
    if active(app) {
        return;
    }
    let ui = app.global::<StudioLux>();
    let (clean_url, is_account): (String, bool) = concat_studio::lux::normalize_and_classify_url(&ui.get_url());
    let playlist = if is_account {
        ui.set_is_playlist(true);
        ui.set_can_browser(true);
        true
    } else {
        ui.set_is_playlist(false);
        false
    };
    let url = match lux::validate_video_request(&clean_url, playlist) {
        Ok(u) => u,
        Err(e) => {
            ui.set_error(e.into());
            return;
        }
    };
    let (generation, cancel) = {
        let Ok(mut s) = shared.lock() else { return };
        clear_info(app, &mut s);
        s.cancel = Arc::new(AtomicBool::new(false));
        (s.generation, s.cancel.clone())
    };
    let limit = [10, 100, 500][ui.get_scan_limit_index().clamp(0, 2) as usize];
    ui.set_busy(true);
    ui.set_status(if browser { "Đang mở trình duyệt riêng để đọc tài khoản. Hoàn tất xác minh nếu được yêu cầu; bấm Hủy để dừng quét." } else { "Đang lấy danh sách/thông tin video…" }.into());
    let reply = app.as_weak();
    std::thread::spawn(move || {
        let result = (|| {
            let dirs = concat_host::AppDirs::locate()?;
            let rows = if browser {
                concat_studio::download_browser::list_tiktok(
                    &url,
                    limit,
                    &dirs.data.join("studio/download-browser"),
                    cancel.clone(),
                )?
            } else if playlist {
                lux::list_account_videos(&url, limit, cancel.clone())?
            } else {
                lux::probe_video_info_cancellable(&url, false, cancel.clone())?
            };
            Ok::<_, String>((
                rows,
                video_library::downloaded_sources(&dirs.data.join("studio/video-library")),
            ))
        })();
        let _ = reply.upgrade_in_event_loop(move |app| {
            let ui = app.global::<StudioLux>(); let Ok(mut s) = shared.lock() else { return };
            if s.generation != generation { return; } ui.set_busy(false);
            if cancel.load(Ordering::Relaxed) { ui.set_status("Đã dừng lấy danh sách; không tải video.".into()); return; }
            match result {
                Ok((videos, existing)) if playlist => {
                    s.items = videos.into_iter().map(|video| { let exists = video_library::source_key(&video.url).is_some_and(|k| existing.contains(&k)); Item { video, exists, selected: false, detail: String::new(), preview: Default::default(), preview_ready: false } }).collect();
                    ui.set_has_info(true); publish(&app, &s);
                    ui.set_status(format!("Đọc được {} video trong giới hạn {limit}; {} đã có trên máy. Chọn video cần tải. Đây là danh sách đọc được, không cam kết toàn bộ tài khoản.", s.items.len(), s.items.iter().filter(|i| i.exists).count()).into());
                }
                Ok((videos, existing)) => {
                    if let Some(video) = videos.into_iter().next().filter(|v| !v.streams.is_empty()) {
                        if video_library::source_key(&video.url).is_some_and(|k| existing.contains(&k)) { ui.set_error("Video này đã có trong Danh sách video; không tải lại.".into()); }
                        ui.set_video_title(video.title.clone().into()); ui.set_video_site(video.site.clone().into()); ui.set_video_type(video.media_type.clone().into());
                        ui.set_stream_options(ModelRc::new(VecModel::from(video.streams.iter().map(|s| format!("{} · {} ({})", s.quality, s.size_display, s.ext).into()).collect::<Vec<_>>())));
                        ui.set_has_info(true); ui.set_status("Đã lấy thông tin video.".into()); s.video = Some(video);
                    } else { ui.set_error("Không tìm thấy video có thể tải.".into()); }
                }
                Err(e) => { ui.set_error(lux::friendly_error(&e).into()); ui.set_status(if is_account_url(&url) { "Chưa đọc được tài khoản. Hoàn tất xác minh trong trình duyệt nếu có, rồi bấm Lấy danh sách lại." } else { "Không đọc được video. Kiểm tra quyền truy cập và thử lại." }.into()); }
            }
            drop(s);
            if playlist { prepare_choices(&app, shared.clone()); }
        });
    });
}
pub(crate) fn bind(app: &App) {
    let state: Shared = Arc::new(Mutex::new(State::default()));
    let weak = app.as_weak();
    app.global::<StudioLux>().on_open_dialog(move || {
        if let Some(app) = weak.upgrade() {
            app.global::<StudioLux>().set_open(true);
        }
    });
    let weak = app.as_weak();
    app.global::<StudioLux>().on_close_dialog(move || {
        if let Some(app) = weak.upgrade() {
            if !active(&app) {
                app.global::<StudioLux>().set_open(false);
            }
        }
    });
    let weak = app.as_weak();
    let shared = state.clone();
    app.global::<StudioLux>().on_url_edited(move |text| {
        if let Some(app) = weak.upgrade() {
            if active(&app) {
                return;
            }
            let ui = app.global::<StudioLux>();
            if ui.get_url().trim() == text.trim() {
                return;
            }
            let (normalized, is_account): (String, bool) = concat_studio::lux::normalize_and_classify_url(&text);
            ui.set_can_browser(is_account);
            ui.set_is_playlist(is_account);
            if normalized != text.as_str() && !normalized.is_empty() {
                ui.set_url(normalized.into());
            } else {
                ui.set_url(text);
            }
            if let Ok(mut s) = shared.lock() {
                clear_info(&app, &mut s);
            }
        }
    });
    let weak = app.as_weak();
    let shared = state.clone();
    app.global::<StudioLux>().on_toggle_playlist(move |on| {
        if let Some(app) = weak.upgrade() {
            if active(&app) {
                return;
            }
            if app.global::<StudioLux>().get_is_playlist() == on {
                return;
            }
            app.global::<StudioLux>().set_is_playlist(on);
            if let Ok(mut s) = shared.lock() {
                clear_info(&app, &mut s);
            }
        }
    });
    let weak = app.as_weak();
    let shared = state.clone();
    app.global::<StudioLux>().on_limit_changed(move |index| {
        if let Some(app) = weak.upgrade() {
            if active(&app) {
                return;
            }
            let ui = app.global::<StudioLux>();
            let index = index.clamp(0, 2);
            if ui.get_scan_limit_index() == index { return; }
            ui.set_scan_limit_index(index);
            // A new enumeration limit does not invalidate already discovered URLs.
            // Keep the gallery/selection instead of leaving a blank dialog.
            if let Ok(s) = shared.lock() {
                if !s.items.is_empty() { ui.set_status(format!("Đang giữ {} video đã đọc và lựa chọn hiện tại. Bấm Lấy danh sách nếu muốn đọc lại với giới hạn mới.", s.items.len()).into()); }
                else { ui.set_status("Bấm Lấy danh sách để hiện video cần chọn.".into()); }
            }
        }
    });
    let weak = app.as_weak();
    let shared = state.clone();
    app.global::<StudioLux>().on_reset(move || {
        if let Some(app) = weak.upgrade() {
            if !active(&app) {
                if let Ok(mut s) = shared.lock() {
                    clear_info(&app, &mut s);
                }
            }
        }
    });
    let weak = app.as_weak();
    app.global::<StudioLux>().on_stream_changed(move |idx| {
        if let Some(app) = weak.upgrade() {
            if !active(&app) {
                app.global::<StudioLux>().set_stream_index(idx);
            }
        }
    });
    let weak = app.as_weak();
    let shared = state.clone();
    app.global::<StudioLux>().on_cancel_download(move || {
        if let Ok(s) = shared.lock() {
            s.cancel.store(true, Ordering::Relaxed);
        }
        if let Some(app) = weak.upgrade() {
            app.global::<StudioLux>()
                .set_status("Đang dừng; giữ lại các video đã hoàn tất…".into());
        }
    });
    let weak = app.as_weak();
    let shared = state.clone();
    app.global::<StudioLux>().on_fetch_info(move || {
        if let Some(app) = weak.upgrade() {
            let current = app.global::<StudioLux>().get_url();
            let (normalized, is_account): (String, bool) = concat_studio::lux::normalize_and_classify_url(&current);
            if normalized != current.as_str() && !normalized.is_empty() {
                app.global::<StudioLux>().set_url(normalized.into());
            }
            fetch(&app, shared.clone(), is_account);
        }
    });
    let weak = app.as_weak();
    let shared = state.clone();
    app.global::<StudioLux>().on_fetch_browser(move || {
        if let Some(app) = weak.upgrade() {
            fetch(&app, shared.clone(), true);
        }
    });
    let weak = app.as_weak();
    let shared = state.clone();
    app.global::<StudioLux>().on_toggle_choice(move |index| {
        if let Some(app) = weak.upgrade() {
            if active(&app) {
                return;
            }
            if let Ok(mut s) = shared.lock() {
                if let Some(item) = s.items.get_mut(index.max(0) as usize).filter(|i| !i.exists) {
                    item.selected = !item.selected;
                }
                publish(&app, &s);
            }
        }
    });
    let weak = app.as_weak();
    let shared = state.clone();
    app.global::<StudioLux>().on_choose_first(move |count| {
        if let Some(app) = weak.upgrade() {
            if active(&app) {
                return;
            }
            if let Ok(mut s) = shared.lock() {
                let flags = video_library::select_new(
                    s.items.iter().map(|i| i.exists),
                    count.max(0) as usize,
                );
                for (item, selected) in s.items.iter_mut().zip(flags) {
                    item.selected = selected;
                }
                publish(&app, &s);
            }
        }
    });
    let weak = app.as_weak();
    let shared = state.clone();
    app.global::<StudioLux>().on_filter_changed(move || {
        if let Some(app) = weak.upgrade() {
            if !active(&app) {
                app.global::<StudioLux>().set_choice_page(0);
                if let Ok(s) = shared.lock() {
                    publish(&app, &s);
                }
                prepare_choices(&app, shared.clone());
            }
        }
    });
    let weak = app.as_weak();
    let shared = state.clone();
    app.global::<StudioLux>()
        .on_choice_page_changed(move |page| {
            if let Some(app) = weak.upgrade() {
                if active(&app) {
                    return;
                }
                app.global::<StudioLux>().set_choice_page(page.max(0));
                if let Ok(s) = shared.lock() {
                    publish(&app, &s);
                }
                prepare_choices(&app, shared.clone());
            }
        });
    let weak = app.as_weak();
    let shared = state.clone();
    app.global::<StudioLux>().on_start_download(move || {
        if let Some(app) = weak.upgrade() {
            start(&app, shared.clone());
        }
    });
    if std::env::var_os("CONCAT_DOWNLOAD_SMOKE").is_some()
        && std::env::args().any(|arg| arg == "--smoke")
    {
        selection_smoke(app, &state);
    }
}

// Exercise the real bound callbacks without network, downloads or user projects.
fn selection_smoke(app: &App, state: &Shared) {
    let ui = app.global::<StudioLux>();
    {
        let mut s = state.lock().expect("download smoke state");
        s.items = (0..150)
            .map(|index| Item {
                video: LuxVideoInfo {
                    url: format!("https://www.tiktok.com/@fixture/video/{index}"),
                    title: format!("Fixture {index}"),
                    site: "TikTok".into(),
                    media_type: "video".into(),
                    streams: Vec::new(),
                    thumbnail: String::new(),
                    duration_seconds: 0,
                },
                selected: false,
                exists: index < 20,
                detail: String::new(),
                preview: Default::default(),
                preview_ready: true,
            })
            .collect();
    }
    ui.set_is_playlist(true);
    ui.set_has_info(true);
    ui.set_hide_existing(true);
    ui.invoke_choose_first(100);
    assert_eq!(ui.get_selected_count(), 100);
    assert_eq!(ui.get_choices().row_count(), 20);
    assert_eq!(ui.get_total_count(), 150);
    assert_eq!(ui.get_choice_pages(), 7);
    ui.invoke_limit_changed(0);
    assert!(ui.get_has_info());
    assert_eq!(ui.get_selected_count(), 100);
    assert_eq!(ui.get_choices().row_count(), 20);
    ui.invoke_toggle_choice(0); // Previously downloaded entry, not selectable.
    assert_eq!(ui.get_selected_count(), 100);
    ui.invoke_toggle_choice(20);
    assert_eq!(ui.get_selected_count(), 99);
    ui.set_busy(true);
    ui.invoke_choose_first(0);
    assert_eq!(ui.get_selected_count(), 99); // Busy state freezes selection.
    ui.set_busy(false);
    ui.invoke_choose_first(10);
    assert_eq!(ui.get_selected_count(), 10);
    ui.set_hide_existing(false);
    ui.invoke_filter_changed();
    assert_eq!(ui.get_choices().row_count(), 20);
    assert_eq!(ui.get_choice_pages(), 8);
    ui.invoke_choose_first(0);
    assert_eq!(ui.get_selected_count(), 0);
    ui.invoke_url_edited("https://www.tiktok.com/@account".into());
    assert!(ui.get_can_browser());
    assert!(!ui.get_has_info());
    ui.invoke_url_edited("https://www.tiktok.com/@account/video/123".into());
    assert!(!ui.get_is_playlist());
    assert!(!ui.get_can_browser());
    ui.invoke_reset();
    ui.set_url("".into());
    ui.set_hide_existing(true);
    log::info!(
        "download selection smoke: PASS real callbacks select 10/100, hide-existing, duplicate guard, busy guard, reset; no network or project mutation"
    );
}
/// Explicit integration smoke through the real async fetch callback/event loop.
/// Does not download media, change a project, or register library entries.
pub(crate) fn live_smoke(app: &App, profile: &str) -> Result<(), slint::PlatformError> {
    use std::{
        cell::RefCell,
        rc::Rc,
        time::{Duration, Instant},
    };
    let ui = app.global::<StudioLux>();
    ui.invoke_url_edited(profile.into());
    ui.invoke_limit_changed(0);
    ui.set_open(true);
    ui.invoke_fetch_info();
    let result = Rc::new(RefCell::new(None::<Result<String, String>>));
    let result_slot = result.clone();
    let weak = app.as_weak();
    let started = Instant::now();
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(100), move || {
        let Some(app) = weak.upgrade() else { return }; let ui = app.global::<StudioLux>();
        let photos = ui.get_choices().iter().filter(|r| r.thumbnail.size().width > 0).count();
        let outcome = if ui.get_has_info() && ui.get_total_count() > 0 && photos > 0 {
            let total = ui.get_total_count(); let rows = ui.get_choices().row_count();
            ui.invoke_choose_first(10); let selected = ui.get_selected_count();
            ui.invoke_limit_changed(1);
            if !ui.get_has_info() || ui.get_total_count() != total || ui.get_selected_count() != selected || ui.get_choices().row_count() != rows {
                Some(Err("Changing 10 -> 100 blanked loaded gallery or selection".into()))
            } else { Some(Ok(format!("{total} real videos; {photos} decoded public covers; {selected} selected; limit change preserved gallery"))) }
        } else if !ui.get_busy() && !ui.get_has_info() && !ui.get_error().is_empty() { Some(Err(ui.get_error().to_string())) }
        else if started.elapsed() > Duration::from_secs(100) { ui.invoke_cancel_download(); Some(Err("Timed out waiting for real account cards and thumbnail".into())) }
        else { None };
        if let Some(outcome) = outcome { *result_slot.borrow_mut() = Some(outcome); let _ = slint::quit_event_loop(); }
    });
    app.show()?;
    slint::run_event_loop()?;
    timer.stop();
    app.hide()?;
    let outcome = result
        .borrow_mut()
        .take()
        .unwrap_or_else(|| Err("Live gallery check ended without a receipt".into()));
    ui.invoke_reset();
    ui.set_open(false);
    match outcome {
        Ok(message) => {
            log::info!(
                "download live gallery smoke: PASS {message}; no video downloads or project changes"
            );
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}
fn start(app: &App, shared: Shared) {
    if active(app) {
        return;
    }
    let ui = app.global::<StudioLux>();
    let queue: Vec<(Option<usize>, String, Option<String>)> = {
        let Ok(mut s) = shared.lock() else { return };
        let rows = if ui.get_is_playlist() {
            s.items
                .iter()
                .enumerate()
                .filter(|(_, i)| i.selected && !i.exists)
                .map(|(idx, i)| (Some(idx), i.video.url.clone(), Some("yt:auto".into())))
                .collect()
        } else {
            let url = match lux::validate_video_request(&ui.get_url(), false) {
                Ok(u) => u,
                Err(e) => {
                    ui.set_error(e.into());
                    return;
                }
            };
            vec![(
                None,
                s.video.as_ref().map(|v| v.url.clone()).unwrap_or(url),
                s.video
                    .as_ref()
                    .and_then(|v| v.streams.get(ui.get_stream_index().max(0) as usize))
                    .map(|s| s.id.clone()),
            )]
        };
        s.cancel = Arc::new(AtomicBool::new(false));
        rows
    };
    if queue.is_empty() {
        ui.set_error("Chọn ít nhất một video chưa có trên máy.".into());
        return;
    }
    let dirs = match concat_host::AppDirs::locate() {
        Ok(d) => d,
        Err(e) => {
            ui.set_error(e.into());
            return;
        }
    };
    let cancel = {
        let Ok(s) = shared.lock() else { return };
        s.cancel.clone()
    };
    let epoch = crate::host::project_epoch();
    let mut project = None;
    crate::host::Shell::with(|shell, _| {
        project = shell
            .studio
            .borrow()
            .session
            .as_ref()
            .map(|s| s.path().to_owned());
    });
    ui.set_downloading(true);
    ui.set_done(false);
    ui.set_progress(0.0);
    ui.set_error("".into());
    ui.set_speed("".into());
    ui.set_eta("".into());
    ui.set_status("Đang chuẩn bị hàng đợi…".into());
    let reply = app.as_weak();
    std::thread::spawn(move || {
        let total = queue.len();
        let mut saved = Vec::new();
        let mut failed = 0;
        let mut skipped = 0;
        let root = dirs.data.join("studio/video-library");
        let mut existing = video_library::downloaded_sources(&root);
        for (n, (index, url, stream_id)) in queue.into_iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let key = video_library::source_key(&url);
            let duplicate = key.as_ref().is_some_and(|k| existing.contains(k));
            let result = if duplicate {
                skipped += 1;
                Ok(Vec::new())
            } else {
                let progress = reply.clone();
                let flag = cancel.clone();
                lux::download_video(
                    &LuxDownloadOptions {
                        url: url.clone(),
                        stream_id,
                        playlist: false,
                        output_dir: Some(dirs.data.join("videos/downloads")),
                        audio_only: false,
                        cancel: cancel.clone(),
                    },
                    move |event| {
                        let flag = flag.clone();
                        let _ = progress.upgrade_in_event_loop(move |app| {
                            if flag.load(Ordering::Relaxed) {
                                return;
                            }
                            let ui = app.global::<StudioLux>();
                            match event {
                                LuxProgressEvent::Progress {
                                    percent,
                                    speed,
                                    eta,
                                    ..
                                } => {
                                    ui.set_progress((n as f32 + percent / 100.0) / total as f32);
                                    ui.set_speed(speed.into());
                                    ui.set_eta(eta.into());
                                    ui.set_status(
                                        format!("Video {}/{total} · {percent:.1}%", n + 1).into(),
                                    );
                                }
                                LuxProgressEvent::Merging { .. } => ui.set_status(
                                    format!("Video {}/{total} · đang ghép âm thanh", n + 1).into(),
                                ),
                                LuxProgressEvent::Started { .. } => ui.set_status(
                                    format!("Đang tải video {}/{total}…", n + 1).into(),
                                ),
                                _ => {}
                            }
                        });
                    },
                )
                .and_then(|paths| {
                    // Separate attempt per item: later cancellation never rolls
                    // back an earlier completed video.
                    for path in &paths {
                        concat_media::probe::probe(path)
                            .map_err(|_| "Tệp tải về không đọc được.")?
                            .require_video()
                            .map_err(|_| "Tệp tải về không có hình video.")?;
                        video_library::record_online(&root, path, "Tải trực tuyến", Some(&url))?;
                    }
                    Ok(paths)
                })
            };
            let (ok, detail) = match result {
                Ok(paths) => {
                    if let Some(key) = key {
                        existing.insert(key);
                    }
                    saved.extend(paths);
                    (true, String::new())
                }
                Err(e) if e == "DOWNLOAD_CANCELLED" => break,
                Err(e) => {
                    failed += 1;
                    (false, lux::friendly_error(&e))
                }
            };
            let state = shared.clone();
            let _ = reply.upgrade_in_event_loop(move |app| {
                if let Ok(mut s) = state.lock() {
                    if let Some(item) = index.and_then(|i| s.items.get_mut(i)) {
                        item.exists = ok;
                        item.selected = !ok;
                        item.detail = detail.clone();
                    }
                    publish(&app, &s);
                }
                let ui = app.global::<StudioLux>();
                ui.set_progress((n + 1) as f32 / total as f32);
                if !ok {
                    ui.set_error(detail.into());
                }
            });
        }
        let _ = reply.upgrade_in_event_loop(move |app| {
            let ui = app.global::<StudioLux>(); ui.set_downloading(false); ui.set_speed("".into()); ui.set_eta("".into()); let count = saved.len();
            // Never import after cancellation or switching projects; completed
            // files are already durable in the common video library.
            if !cancel.load(Ordering::Relaxed) && !saved.is_empty() && crate::host::project_epoch() == epoch {
                crate::host::Shell::with(|shell, app| { let mut studio = shell.studio.borrow_mut();
                    if project.as_ref().is_some_and(|p| studio.session.as_ref().is_some_and(|s| s.path() == p)) {
                        studio.handle(crate::panes::Msg::Media(crate::panes::media_bin::MediaMsg::Import(saved))); studio.refresh_art(); studio.publish(&app, &shell.models);
                    }
                });
            }
            ui.set_done(!cancel.load(Ordering::Relaxed) && failed == 0);
            ui.set_status(format!("{} Đã lưu {count} video vào Danh sách video · {skipped} bỏ qua vì đã có · {failed} lỗi.{}", if cancel.load(Ordering::Relaxed) { "Đã hủy hàng đợi." } else { "Đã xử lý hàng đợi." }, if failed > 0 { " Các video lỗi vẫn được chọn để thử lại." } else { "" }).into());
            if skipped > 0 && count == 0 { ui.set_error("Video đã có trên máy; mở Danh sách video để sử dụng, không cần tải lại.".into()); }
            app.global::<crate::ui::StudioBulk>().invoke_refresh_videos();
            prepare_choices(&app, shared.clone());
        });
    });
}
