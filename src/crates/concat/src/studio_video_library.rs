// SPDX-License-Identifier: AGPL-3.0-or-later
//! Paged local-video gallery; bounded cached previews prepared off the UI thread.
use crate::studio_video_preview::Prepared;
use crate::ui::{App, StudioBulk, StudioVideoItem};
use concat_studio::video_library::Entry;
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

const PAGE_SIZE: usize = 24;

// Media paths are absolute. Compare Windows spellings without filesystem
// access on the UI thread (a project may reference an unplugged drive).
fn media_path_key(path: &str) -> String {
    #[cfg(windows)]
    {
        let path = path.replace('\\', "/");
        let path = path.strip_prefix("//?/").unwrap_or(&path);
        if let Some(unc) = path.strip_prefix("UNC/") {
            format!("//{unc}").to_lowercase()
        } else {
            path.to_lowercase()
        }
    }
    #[cfg(not(windows))]
    { path.to_owned() }
}
#[derive(Clone)]
struct Record {
    path: PathBuf,
    title: String,
    detail: String,
    preview: Prepared,
    ready: bool,
    is_generated: bool,
}
impl Record {
    fn view(&self) -> StudioVideoItem {
        StudioVideoItem {
            path: self.path.to_string_lossy().into_owned().into(),
            title: self.title.clone().into(),
            detail: self.detail.clone().into(),
            thumbnail: self.preview.image(),
            duration: if self.preview.duration.is_empty() {
                "Đang đọc...".into()
            } else {
                self.preview.duration.clone().into()
            },
            is_generated: self.is_generated,
        }
    }
}
#[derive(Default)]
struct State {
    rows: Vec<Record>,
    page: usize,
    epoch: u64,
    filter_mode: i32,
}
type Shared = Arc<Mutex<State>>;

fn publish(app: &App, state: &mut State) {
    let ui = app.global::<StudioBulk>();
    let mode = state.filter_mode;
    let filtered_indices: Vec<usize> = state
        .rows
        .iter()
        .enumerate()
        .filter(|(_, r)| match mode {
            1 => r.is_generated,
            2 => !r.is_generated,
            _ => true,
        })
        .map(|(idx, _)| idx)
        .collect();

    let total = filtered_indices.len();
    let pages = total.div_ceil(PAGE_SIZE).max(1);
    state.page = state.page.min(pages - 1);
    ui.set_filter_mode(mode);
    ui.set_library_page(state.page as i32);
    ui.set_library_pages(pages as i32);
    ui.set_library_total(total as i32);

    let start = state.page * PAGE_SIZE;
    let page_indices: Vec<usize> = filtered_indices
        .iter()
        .skip(start)
        .take(PAGE_SIZE)
        .copied()
        .collect();

    for (i, row) in state.rows.iter_mut().enumerate() {
        if !page_indices.contains(&i) {
            row.preview = Default::default();
            row.ready = false;
        }
    }

    let page_records: Vec<_> = page_indices
        .iter()
        .filter_map(|&idx| state.rows.get(idx))
        .map(Record::view)
        .collect();

    ui.set_videos(ModelRc::new(VecModel::from(page_records)));

    let selected = ui.get_selected_video().path;
    let selected_rec = page_indices
        .iter()
        .filter_map(|&idx| state.rows.get(idx))
        .find(|r| r.path.to_string_lossy() == selected.as_str())
        .or_else(|| page_indices.first().and_then(|&idx| state.rows.get(idx)));
    ui.set_selected_video(selected_rec.map(Record::view).unwrap_or_default());
}
fn prepare_page(app: &App, state: Shared) {
    let (epoch, work) = {
        let Ok(s) = state.lock() else { return };
        let mode = s.filter_mode;
        let filtered: Vec<&Record> = s
            .rows
            .iter()
            .filter(|r| match mode {
                1 => r.is_generated,
                2 => !r.is_generated,
                _ => true,
            })
            .collect();
        (
            s.epoch,
            filtered
                .iter()
                .skip(s.page * PAGE_SIZE)
                .take(PAGE_SIZE)
                .filter(|r| !r.ready)
                .map(|r| r.path.clone())
                .collect::<Vec<_>>(),
        )
    };
    if work.is_empty() {
        return;
    }
    let reply = app.as_weak();
    std::thread::spawn(move || {
        let Ok(dirs) = concat_host::AppDirs::locate() else {
            return;
        };
        for path in work {
            if !state.lock().is_ok_and(|s| s.epoch == epoch) {
                break;
            }
            let preview = crate::studio_video_preview::local(
                &path,
                &dirs.data.join("studio/video-thumbnails/local"),
            );
            let shared = state.clone();
            let _ = reply.upgrade_in_event_loop(move |app| {
                let Ok(mut s) = shared.lock() else { return };
                if s.epoch != epoch {
                    return;
                }
                let Some(row) = s.rows.iter_mut().find(|r| r.path == path) else {
                    return;
                };
                row.preview = preview;
                row.ready = true;
                let view = row.view();
                let ui = app.global::<StudioBulk>();
                let model = ui.get_videos();
                for i in 0..model.row_count() {
                    if model.row_data(i).is_some_and(|r| r.path == view.path) {
                        model.set_row_data(i, view.clone());
                        break;
                    }
                }
                if ui.get_selected_video().path == view.path {
                    ui.set_selected_video(view);
                }
            });
        }
    });
}
pub(crate) fn record(path: &Path, source: &str) -> Result<(), String> {
    let root = concat_host::AppDirs::locate()?
        .data
        .join("studio/video-library");
    concat_studio::video_library::record(&root, path, source)
}
// Bounded discovery of known completed production outputs, never entire drives/links.
fn discover(root: &Path, depth: usize, budget: &mut usize) {
    if depth == 0 || *budget == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_dir() {
            discover(&path, depth - 1, budget);
        } else if kind.is_file()
            && (path.file_name().is_some_and(|n| n == "film.mp4")
                || (path.extension().is_some_and(|e| e == "mp4")
                    && !path.file_name().unwrap().to_string_lossy().starts_with("shot_")
                    && !path.file_name().unwrap().to_string_lossy().starts_with("shot-")))
            && !root.join("shot-0000.mp4").exists()
            && concat_media::probe::probe(&path).is_ok_and(|i| i.require_video().is_ok())
        {
            let _ = record(&path, "Làm phim AI");
        }
    }
}
pub(crate) fn bind(app: &App) {
    let state: Shared = Arc::new(Mutex::new(State::default()));
    let weak = app.as_weak();
    let shared = state.clone();
    app.global::<StudioBulk>()
        .on_change_filter_mode(move |mode| {
            if let Some(app) = weak.upgrade() {
                if let Ok(mut s) = shared.lock() {
                    s.filter_mode = mode;
                    s.page = 0;
                    s.epoch += 1;
                    publish(&app, &mut s);
                }
                prepare_page(&app, shared.clone());
            }
        });
    let weak = app.as_weak();
    let shared = state.clone();
    app.global::<StudioBulk>()
        .on_change_library_page(move |page| {
            if let Some(app) = weak.upgrade() {
                if let Ok(mut s) = shared.lock() {
                    s.page = page.max(0) as usize;
                    s.epoch += 1;
                    publish(&app, &mut s);
                }
                prepare_page(&app, shared.clone());
            }
        });
    let weak = app.as_weak();
    let shared = state.clone();
    app.global::<StudioBulk>().on_select_video(move |path| {
        if let Some(app) = weak.upgrade() {
            if let Ok(s) = shared.lock() {
                if let Some(row) = s
                    .rows
                    .iter()
                    .find(|r| r.path.to_string_lossy() == path.as_str())
                {
                    app.global::<StudioBulk>().set_selected_video(row.view());
                }
            }
        }
    });
    let weak = app.as_weak();
    app.global::<StudioBulk>().on_request_delete(move |path| {
        if let Some(app) = weak.upgrade() {
            app.global::<StudioBulk>().set_delete_confirm_path(path);
        }
    });
    let weak = app.as_weak();
    app.global::<StudioBulk>().on_cancel_delete(move || {
        if let Some(app) = weak.upgrade() {
            app.global::<StudioBulk>().set_delete_confirm_path("".into());
        }
    });
    let weak = app.as_weak();
    app.global::<StudioBulk>().on_confirm_delete(move || {
        let Some(app) = weak.upgrade() else { return };
        let ui = app.global::<StudioBulk>();
        let path = PathBuf::from(ui.get_delete_confirm_path().as_str());
        if path.as_os_str().is_empty() { return; }
        let result = concat_host::AppDirs::locate()
            .map_err(|error| error.to_string())
            .and_then(|dirs| concat_studio::video_library::remove(&dirs.data.join("studio/video-library"), &path));
        ui.set_delete_confirm_path("".into());
        match result {
            Ok(()) => {
                ui.set_library_status("Đã xóa video khỏi máy và danh sách.".into());
                ui.invoke_refresh_videos();
            }
            Err(error) => ui.set_library_status(error.into()),
        }
    });
    let weak = app.as_weak();
    let shared = state.clone();
    app.global::<StudioBulk>().on_refresh_videos(move || {
        let Some(app) = weak.upgrade() else { return };
        let ui = app.global::<StudioBulk>();
        if ui.get_library_busy() {
            return;
        }
        ui.set_library_busy(true);
        let mut project_root = None;
        crate::host::Shell::with(|shell, _| {
            project_root = shell
                .studio
                .borrow()
                .session
                .as_ref()
                .map(|s| PathBuf::from(s.path()).join(".concat-storyboard"));
        });
        let reply = app.as_weak();
        let shared = shared.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<Vec<Record>, String> {
                let dirs = concat_host::AppDirs::locate()?;
                let mut budget = 10000;
                discover(&dirs.data.join("studio/production"), 5, &mut budget);
                discover(&dirs.data.join("studio/storyboard"), 4, &mut budget);
                discover(&PathBuf::from(r"D:\capcutpolot\concat\.runtime\films"), 4, &mut budget);
                if let Some(root) = project_root {
                    discover(&root, 3, &mut budget);
                }
                for page in concat_studio::pages::ContentPageStore::new(&dirs.config)
                    .list(true)
                    .unwrap_or_default()
                {
                    discover(Path::new(&page.output_root), 5, &mut budget);
                }
                let mut rows = Vec::new();
                if let Ok(entries) = std::fs::read_dir(dirs.data.join("studio/video-library")) {
                    for entry in entries.flatten().take(10000) {
                        if entry.path().extension().is_none_or(|e| e != "json")
                            || entry.metadata().is_ok_and(|m| m.len() > 65536)
                        {
                            continue;
                        }
                        let Ok(bytes) = std::fs::read(entry.path()) else {
                            continue;
                        };
                        let Ok(row) = serde_json::from_slice::<Entry>(&bytes) else {
                            continue;
                        };
                        let Ok(meta) = row.path.metadata() else {
                            continue;
                        };
                        if !meta.is_file() {
                            continue;
                        }
                        let path_lower = row.path.to_string_lossy().to_lowercase();
                        let source_lower = row.source.to_lowercase();
                        let is_generated = path_lower.contains("films")
                            || path_lower.contains("storyboard")
                            || path_lower.contains("production")
                            || source_lower.contains("ai")
                            || source_lower.contains("phim")
                            || source_lower.contains("gent");
                        rows.push((
                            meta.modified().unwrap_or(std::time::UNIX_EPOCH),
                            Record {
                                title: row
                                    .path
                                    .file_stem()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .into_owned(),
                                path: row.path,
                                detail: format!(
                                    "{} · {:.1} MB",
                                    row.source,
                                    meta.len() as f64 / 1048576.0
                                ),
                                preview: Default::default(),
                                ready: false,
                                is_generated,
                            },
                        ));
                    }
                }
                rows.sort_by(|a, b| b.0.cmp(&a.0));
                Ok(rows.into_iter().map(|(_, row)| row).collect())
            })();
            let _ = reply.upgrade_in_event_loop(move |app| {
                let ui = app.global::<StudioBulk>();
                ui.set_library_busy(false);
                match result {
                    Ok(rows) => {
                        ui.set_library_status(
                            format!(
                                "{} video · mới nhất ở trên · chọn thẻ để xem chi tiết",
                                rows.len()
                            )
                            .into(),
                        );
                        if let Ok(mut s) = shared.lock() {
                            s.rows = rows;
                            s.page = 0;
                            s.epoch += 1;
                            publish(&app, &mut s);
                        }
                        prepare_page(&app, shared.clone());
                    }
                    Err(e) => ui.set_library_status(e.into()),
                }
            });
        });
    });
    let weak = app.as_weak();
    app.global::<StudioBulk>()
        .on_video_action(move |action, path| {
            let Some(app) = weak.upgrade() else { return };
            let path = PathBuf::from(path.as_str());
            if !path.is_file() {
                if action == "use" {
                    app.global::<StudioBulk>().set_library_use_status("Video đã bị di chuyển hoặc xóa khỏi máy.".into());
                } else {
                    app.global::<StudioBulk>().set_library_status("Video đã bị di chuyển hoặc xóa khỏi máy.".into());
                }
                return;
            }
            if action == "use" {
                app.global::<StudioBulk>().set_library_use_status("Đang mở video trong dự án…".into());
                crate::host::spawn_in_project(
                    move || concat_host::media::probe(&path.to_string_lossy()),
                    |studio, app, _, result| {
                        if studio.session.is_none() {
                            app.global::<StudioBulk>().set_library_use_status("Mở hoặc tạo dự án để sử dụng video.".into());
                            return;
                        }
                        match result {
                            Ok(summary) => {
                                let key = media_path_key(&summary.path);
                                let existing = studio.project().media.iter()
                                    .find(|item| media_path_key(&item.path) == key)
                                    .map(|item| item.id.clone());
                                let media_id = existing.or_else(|| studio.apply(concat_project::Command::AddMedia {
                                    item: summary.to_new_media(),
                                }));
                                if let Some(id) = media_id {
                                    studio.media.selected.clear();
                                    studio.media.selected.insert(id.clone());
                                    studio.preview_media(&id, -1);
                                    studio.publish_source(app);
                                    studio.request_preview();
                                    app.global::<StudioBulk>().set_library_use_status("Đã mở video. Kéo thẻ trong dự án xuống timeline để dựng.".into());
                                }
                            }
                            Err(error) => {
                                app.global::<StudioBulk>().set_library_use_status(format!("Không mở được video: {error}").into());
                                studio.notify(&crate::host::probe_error(&error), true);
                            }
                        }
                    },
                );
                return;
            }
            let result = match action.as_str() {
                "play" => opener::open(&path).map_err(|e| e.to_string()),
                "reveal" => crate::platform::reveal(&path.to_string_lossy()),
                "import" => {
                    let mut imported = false;
                    crate::host::Shell::with(|shell, app| {
                        let mut studio = shell.studio.borrow_mut();
                        if studio.session.is_some() {
                            studio.handle(crate::panes::Msg::Media(
                                crate::panes::media_bin::MediaMsg::Import(vec![path.clone()]),
                            ));
                            studio.refresh_art();
                            studio.publish(&app, &shell.models);
                            imported = true;
                        }
                    });
                    if imported {
                        Ok(())
                    } else {
                        Err("Mở hoặc tạo dự án trước khi nhập video.".into())
                    }
                }
                _ => Err("Thao tác không hợp lệ".into()),
            };
            if let Err(e) = result {
                app.global::<StudioBulk>().set_library_status(e.into());
            }
        });
    if std::env::var_os("CONCAT_DOWNLOAD_SMOKE").is_some()
        && std::env::args().any(|a| a == "--smoke")
    {
        // Real gallery selection callback and bounded model, no provider calls.
        if let Some(fixture) = std::env::var_os("CONCAT_PREVIEW_FIXTURE_COVER") {
            crate::studio_video_preview::cover_smoke(
                Path::new(&fixture),
                &std::env::temp_dir().join("concat-gallery-smoke-thumbnails"),
            );
            log::info!(
                "video gallery CDN preview smoke: PASS actual downloaded cover decoded to bounded pixels"
            );
        }
        if let Some(fixture) = std::env::var_os("CONCAT_PREVIEW_FIXTURE_VIDEO") {
            let root = std::env::temp_dir().join("concat-gallery-smoke-thumbnails");
            let preview = crate::studio_video_preview::local(Path::new(&fixture), &root);
            assert!(
                preview.width > 0 && preview.height > 0,
                "actual video frame must decode"
            );
            assert!(!preview.pixels.is_empty());
            assert_ne!(preview.duration, "Video");
            assert!(preview.image().size().width > 0);
            let cached = crate::studio_video_preview::local(Path::new(&fixture), &root);
            assert_eq!(preview.pixels, cached.pixels);
            log::info!(
                "video gallery local preview smoke: PASS actual frame, duration and cached pixels"
            );
        }
        {
            let mut s = state.lock().expect("gallery smoke state");
            s.rows = (0..30)
                .map(|n| Record {
                    path: PathBuf::from(format!("fixture-{n}.mp4")),
                    title: format!("Video {n}"),
                    detail: "Fixture".into(),
                    preview: Default::default(),
                    ready: true,
                    is_generated: n % 2 == 0,
                })
                .collect();
            publish(app, &mut s);
        }
        app.global::<StudioBulk>()
            .invoke_select_video("fixture-2.mp4".into());
        assert_eq!(
            app.global::<StudioBulk>().get_selected_video().title,
            "Video 2"
        );
        assert_eq!(
            app.global::<StudioBulk>().get_videos().row_count(),
            PAGE_SIZE
        );
        // Do not invoke file preview workers in this callback-only fixture.
        {
            let mut s = state.lock().expect("gallery smoke reset");
            s.rows.clear();
            publish(app, &mut s);
        }
        log::info!(
            "video gallery smoke: PASS 24-card paging and selected-video detail; no original files changed"
        );
    }
}
