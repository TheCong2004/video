// SPDX-License-Identifier: AGPL-3.0-or-later
//! Kedi Video Factory – Slint bridge.
//!
//! Connects the `KediFactory` global in Slint to the engine-side
//! `concat_studio::kedi_factory` and `concat_studio::provider_registry`.
//!
//! # Threading model
//! - UI callbacks arrive on the Slint event-loop thread.
//! - All blocking work (resolve, download, file I/O) is spawned on
//!   std threads and communicate back via `upgrade_in_event_loop`.
//! - A `Mutex<KediFactoryState>` guards the shared data; it is never
//!   locked on the UI thread for more than a single field read.

use crate::ui::{App, KediFactory, KediJobRow, KediProviderRow, KediResolvedVideo};
use concat_studio::{
    kedi_factory::{self, FactoryStage, JobInput, KediFactory as FactoryState, KediJob},
    lux::{self, LuxDownloadOptions, LuxProgressEvent, LuxVideoInfo},
    provider_registry::{self, DownloadBackend, ProviderCategory},
    video_library,
};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

// ── Shared state ─────────────────────────────────────────────────────────────

struct State {
    factory: FactoryState,
    resolved: Vec<LuxVideoInfo>,
    selected: Vec<bool>,
    stream_indices: Vec<usize>,
    cancel: Arc<AtomicBool>,
    download_dir: PathBuf,
    assets: Vec<PathBuf>,
    download_cancel: Arc<AtomicBool>,
    render_cancel: Arc<AtomicBool>,
    active_job: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            factory: FactoryState::default(),
            resolved: Vec::new(),
            selected: Vec::new(),
            stream_indices: Vec::new(),
            cancel: Arc::new(AtomicBool::new(false)),
            download_dir: default_download_dir(),
            assets: vec![],
            download_cancel: Arc::new(AtomicBool::new(false)),
            render_cancel: Arc::new(AtomicBool::new(false)),
            active_job: None,
        }
    }
}

type Shared = Arc<Mutex<State>>;

fn default_download_dir() -> PathBuf {
    if let Ok(dirs) = concat_host::AppDirs::locate() {
        dirs.data.join("kedi-factory/downloads")
    } else {
        PathBuf::from("kedi-factory/downloads")
    }
}

// ── Converters ────────────────────────────────────────────────────────────────

fn provider_row(
    def: &'static provider_registry::ProviderDef,
    usable: &[String],
) -> KediProviderRow {
    KediProviderRow {
        id: def.id.into(),
        display_name: def.display_name.into(),
        category: match def.category {
            ProviderCategory::VideoSharing => "Video",
            ProviderCategory::ShortFormVideo => "Short-form",
            ProviderCategory::ECommerce => "Thương mại",
            ProviderCategory::LiveStreaming => "Live",
            ProviderCategory::Social => "Mạng xã hội",
            ProviderCategory::General => "Chung",
        }
        .into(),
        status_label: def.status_label().into(),
        can_download: def.supported_in_code
            && def.download_video
            && usable.contains(&def.id.to_owned()),
        needs_manual: def.needs_manual_fallback(),
        unavailable_reason: def.unavailable_reason.unwrap_or("").into(),
        backend: match def.backend {
            DownloadBackend::YtDlp => "yt-dlp",
            DownloadBackend::Lux => "lux",
            DownloadBackend::ManualOnly => "manual",
            DownloadBackend::Custom => "custom",
        }
        .into(),
    }
}

fn job_row(job: &KediJob) -> KediJobRow {
    let input_summary = match &job.input {
        JobInput::VideoUrl { urls } => format!(
            "{} ({} URL)",
            urls.first()
                .and_then(|u| url::Url::parse(u).ok())
                .and_then(|u| u.host_str().map(str::to_owned))
                .unwrap_or_else(|| "URL".into()),
            urls.len()
        ),
        JobInput::ProductUrl { url } => format!("Sản phẩm: {url}"),
        JobInput::LocalFiles { paths } => {
            format!("File local ({} file)", paths.len())
        }
        JobInput::Brief { text } => {
            let preview: String = text.chars().take(60).collect();
            format!("Brief: {preview}…")
        }
    };
    KediJobRow {
        job_id: job.job_id.clone().into(),
        input_summary: input_summary.into(),
        provider_name: job.provider_id.clone().unwrap_or_default().into(),
        stage_label: match job.stage {
            FactoryStage::Draft => "Bản nháp",
            FactoryStage::Queued => "Đang chờ",
            FactoryStage::Resolving => "Đang đọc thông tin",
            FactoryStage::Downloading => "Đang tải",
            FactoryStage::Paused => "Tạm dừng",
            FactoryStage::Cancelling => "Đang dừng",
            FactoryStage::Cancelled => "Đã dừng",
            FactoryStage::Failed => "Thất bại",
            FactoryStage::Completed => "Hoàn thành",
            FactoryStage::NeedsAttention => "Cần kiểm tra",
        }
        .into(),
        progress: job.progress,
        speed: String::new().into(),
        error: job.error_detail.clone().unwrap_or_default().into(),
        output_count: job.output_paths.len() as i32,
        can_cancel: job.stage.can_cancel(),
        can_retry: job.stage == FactoryStage::Failed && job.attempts < job.max_attempts,
    }
}

fn resolved_video_row(
    index: usize,
    video: &LuxVideoInfo,
    selected: bool,
    exists: bool,
    stream_index: usize,
) -> KediResolvedVideo {
    let quality_options: Vec<SharedString> = video
        .streams
        .iter()
        .map(|s| s.quality.clone().into())
        .collect();
    let duration_label = if video.duration_seconds > 0 {
        let m = video.duration_seconds / 60;
        let s = video.duration_seconds % 60;
        format!("{m}:{s:02}")
    } else {
        String::new()
    };
    KediResolvedVideo {
        index: index as i32,
        url: video.url.clone().into(),
        title: video.title.clone().into(),
        site: video.site.clone().into(),
        duration_label: duration_label.into(),
        thumbnail: slint::Image::default(),
        selected,
        exists,
        quality_options: ModelRc::new(VecModel::from(quality_options)),
        quality_index: stream_index.min(video.streams.len().saturating_sub(1)) as i32,
    }
}

// ── Publishing helpers ────────────────────────────────────────────────────────

fn push_providers(app: &App, state: &State) {
    let ui = app.global::<KediFactory>();
    let usable = &state.factory.usable_providers;
    let rows: Vec<KediProviderRow> = provider_registry::PROVIDERS
        .iter()
        .map(|p| provider_row(p, usable))
        .collect();
    ui.set_providers(ModelRc::new(VecModel::from(rows)));
    ui.set_ytdlp_available(state.factory.ytdlp_available);
    ui.set_lux_available(state.factory.lux_available);
}

fn push_resolved(app: &App, state: &State) {
    let ui = app.global::<KediFactory>();
    let rows: Vec<KediResolvedVideo> = state
        .resolved
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let sel = state.selected.get(i).copied().unwrap_or(false);
            let stream = state.stream_indices.get(i).copied().unwrap_or(0);
            resolved_video_row(i, v, sel, false, stream)
        })
        .collect();
    let selected_count = state.selected.iter().filter(|&&s| s).count();
    ui.set_resolved_videos(ModelRc::new(VecModel::from(rows)));
    ui.set_selected_count(selected_count as i32);
    ui.set_has_resolved(!state.resolved.is_empty());
}

fn push_jobs(app: &App, state: &State) {
    let ui = app.global::<KediFactory>();
    let rows: Vec<KediJobRow> = state.factory.jobs.iter().map(job_row).collect();
    ui.set_jobs(ModelRc::new(VecModel::from(rows)));
    ui.set_jobs_loading(false);
}

fn jobs_root() -> PathBuf {
    default_download_dir().parent().unwrap().join("jobs")
}

fn task_id() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    format!("{}-{nanos}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))
}
fn publish_assets(app: &App, state: &State) {
    let ui = app.global::<KediFactory>();
    ui.set_asset_count(state.assets.len() as i32);
    ui.set_local_paths_display(
        state
            .assets
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("\n")
            .into(),
    );
}
fn ffprobe() -> Result<PathBuf, String> {
    let ffmpeg = crate::studio_scenes::ffmpeg()?;
    Ok(if ffmpeg.components().count() > 1 {
        ffmpeg.with_file_name("ffprobe.exe")
    } else {
        "ffprobe".into()
    })
}
fn register(path: &std::path::Path, url: Option<&str>) -> Result<(), String> {
    let root = concat_host::AppDirs::locate()?
        .data
        .join("studio/video-library");
    video_library::record_online(&root, path, "Kedi Factory", url)
}

/// Downloads are owned by job ID and an independent cancellation token.
fn start_job(app: &App, shared: Shared, id: String) {
    let (job, cancel) = {
        let mut s = shared.lock().expect("state");
        if s.active_job.is_some() {
            return;
        }
        let Some(job) = s.factory.job_mut(&id) else {
            return;
        };
        if job.attempts >= job.max_attempts {
            return;
        }
        job.attempts += 1;
        job.error_code = None;
        job.error_detail = None;
        job.completed_at = None;
        job.progress = 0.0;
        job.advance(FactoryStage::Downloading);
        let job = job.clone();
        s.download_cancel = Arc::new(AtomicBool::new(false));
        s.active_job = Some(id.clone());
        push_jobs(app, &s);
        (job, s.download_cancel.clone())
    };
    app.global::<KediFactory>().set_downloading(true);
    app.global::<KediFactory>().set_global_error("".into());
    let reply = app.as_weak();
    std::thread::spawn(move || {
        let mut errors = vec![];
        if let Err(e) = kedi_factory::save_job(&jobs_root(), &job) {
            errors.push(format!("Không lưu được tác vụ: {e}"));
        }
        let dir = job
            .download_dir
            .clone()
            .unwrap_or_else(default_download_dir);
        let urls = match &job.input {
            JobInput::VideoUrl { urls } => urls.clone(),
            _ => vec![],
        };
        let mut outputs = vec![];
        for (index, url) in urls.iter().enumerate() {
            if cancel.load(Ordering::Acquire) {
                break;
            }
            let share = shared.clone();
            let weak = reply.clone();
            let job_id = id.clone();
            let total = urls.len().max(1) as f32;
            let options = LuxDownloadOptions {
                cancel: cancel.clone(),
                url: url.clone(),
                stream_id: job.selected_streams.get(&index).cloned(),
                playlist: false,
                output_dir: Some(dir.clone()),
                audio_only: false,
            };
            match lux::download_video(&options, move |event| {
                if let LuxProgressEvent::Progress { percent, speed, .. } = event {
                    {
                        let mut s = share.lock().expect("state");
                        if let Some(j) = s.factory.job_mut(&job_id) {
                            j.progress = ((index as f32 + (percent / 100.0).clamp(0.0, 0.99))
                                / total)
                                .min(0.99);
                        }
                    }
                    let sh = share.clone();
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        push_jobs(&app, &sh.lock().expect("state"));
                        app.global::<KediFactory>()
                            .set_status(format!("Đang tải · {speed}").into());
                    });
                }
            }) {
                Ok(paths) => {
                    for path in paths {
                        match ffprobe()
                            .and_then(|probe| kedi_factory::verify_media(&probe, &path))
                            .and_then(|_| register(&path, Some(url)))
                        {
                            Ok(()) => outputs.push(path),
                            Err(e) => errors.push(e),
                        }
                    }
                }
                Err(e) => errors.push(e),
            }
        }
        let cancelled = cancel.load(Ordering::Acquire);
        let mut s = shared.lock().expect("state");
        if let Some(job) = s.factory.job_mut(&id) {
            kedi_factory::finish_download(job, outputs.clone(), cancelled, &errors);
            if let Err(e) = kedi_factory::save_job(&jobs_root(), job) {
                errors.push(format!("Không lưu được kết quả: {e}"));
            }
        }
        for path in outputs {
            if !s.assets.contains(&path) {
                s.assets.push(path);
            }
        }
        s.active_job = None;
        drop(s);
        let _ = reply.upgrade_in_event_loop(move |app| {
            let s = shared.lock().expect("state");
            push_jobs(&app, &s);
            publish_assets(&app, &s);
            let ui = app.global::<KediFactory>();
            ui.set_downloading(false);
            ui.set_status(if cancelled {
                "Đã dừng tải.".into()
            } else if errors.is_empty() {
                "Đã tải, kiểm tra video và lưu vào thư viện.".into()
            } else {
                "Có lỗi khi tải. Xem chi tiết tác vụ bên dưới.".into()
            });
            if !errors.is_empty() {
                ui.set_global_error(errors.join("\n").into());
            }
        });
    });
}

pub fn bind(app: &App) {
    let mut state = State::default();
    state.factory.jobs = kedi_factory::load_jobs(&jobs_root());
    state.factory.check_availability();
    push_jobs(app, &state);
    push_providers(app, &state);
    app.global::<KediFactory>()
        .set_download_dir(state.download_dir.display().to_string().into());
    let shared: Shared = Arc::new(Mutex::new(state));
    {
        let sh = shared.clone();
        let weak = app.as_weak();
        app.global::<KediFactory>().on_import_product(move || {
            let Some(app) = weak.upgrade() else { return };
            let ui = app.global::<KediFactory>();
            if ui.get_product_busy() || ui.get_rendering() { return; }
            let source = ui.get_product_url_input().to_string();
            let link = match concat_studio::factory_product::parse_shopee_link(&source) {
                Ok(link) => link, Err(e) => { ui.set_global_error(e.into()); return; }
            };
            if sh.lock().expect("state").assets.len() + link.image_ids.len() > 100 {
                ui.set_global_error("Tối đa 100 tư liệu. Bỏ bớt tư liệu trước khi nhập thêm.".into()); return;
            }
            ui.set_product_busy(true); ui.set_global_error("".into());
            let sh = sh.clone(); let weak = weak.clone();
            std::thread::spawn(move || {
                let result = (|| {
                    let root = default_download_dir().parent().unwrap().join("products").join(format!("product-{}", task_id()));
                    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
                    let mut paths = vec![];
                    for id in &link.image_ids {
                        let bytes = concat_studio::factory_product::fetch_shopee_image(id)?;
                        let reader = image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format().map_err(|_| "Không đọc được ảnh sản phẩm.")?;
                        let extension = match reader.format() {
                            Some(image::ImageFormat::Jpeg) => "jpg", Some(image::ImageFormat::Png) => "png", Some(image::ImageFormat::WebP) => "webp",
                            _ => return Err("Ảnh không có định dạng JPEG, PNG hoặc WebP được hỗ trợ.".to_string()),
                        };
                        let (w,h) = reader.into_dimensions().map_err(|_| "Ảnh sản phẩm không hợp lệ.")?;
                        if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 32_000_000 { return Err("Kích thước ảnh sản phẩm vượt giới hạn.".into()); }
                        let path = root.join(format!("{id}.{extension}"));
                        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
                        paths.push(path);
                    }
                    std::fs::write(root.join("source.json"), serde_json::to_vec_pretty(&link).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
                    Ok::<_,String>((link, paths))
                })();
                let _ = weak.upgrade_in_event_loop(move |app| {
                    let ui = app.global::<KediFactory>(); ui.set_product_busy(false);
                    if ui.get_product_url_input().as_str() != source { return; }
                    match result {
                        Err(e) => ui.set_global_error(e.into()),
                        Ok((link, paths)) => {
                            ui.set_product_title(link.title_hint.into()); ui.set_product_price("".into());
                            ui.set_product_description("".into()); ui.set_caption("".into());
                            ui.set_product_image(paths.first().and_then(|path| slint::Image::load_from_path(path).ok()).unwrap_or_default());
                            let count = paths.len();
                            let mut s = sh.lock().expect("state");
                            for path in paths { if !s.assets.contains(&path) { s.assets.push(path); } }
                            publish_assets(&app, &s);
                            ui.set_status(if count == 0 { "Đã nhận mã sản phẩm. Link không có mã ảnh công khai; chọn ảnh/video của bạn ở bước 1. Giá chưa được xác minh.".into() } else { format!("Đã nhập {count} ảnh công khai từ link sản phẩm. Tên lấy từ link cần kiểm tra; giá chưa được xác minh.").into() });
                        }
                    }
                });
            });
        });
    }
    {
        let sh = shared.clone();
        let weak = app.as_weak();
        app.global::<KediFactory>().on_check_providers(move || {
            let sh = sh.clone();
            let weak = weak.clone();
            std::thread::spawn(move || {
                sh.lock().expect("state").factory.check_availability();
                let _ = weak.upgrade_in_event_loop(move |app| {
                    push_providers(&app, &sh.lock().expect("state"))
                });
            });
        });
    }
    {
        let sh = shared.clone();
        let weak = app.as_weak();
        app.global::<KediFactory>().on_resolve_url(move || {
            let Some(app) = weak.upgrade() else { return };
            let ui = app.global::<KediFactory>();
            let urls: Vec<String> = ui
                .get_url_input()
                .lines()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect();
            if urls.is_empty() || urls.len() > 100 {
                ui.set_resolve_error("Nhập từ 1 đến 100 link video.".into());
                return;
            }
            let playlist = ui.get_playlist_mode();
            let cancel = {
                let mut s = sh.lock().expect("state");
                s.cancel.store(true, Ordering::Release);
                s.cancel = Arc::new(AtomicBool::new(false));
                s.resolved.clear();
                s.selected.clear();
                s.stream_indices.clear();
                push_resolved(&app, &s);
                s.cancel.clone()
            };
            ui.set_resolving(true);
            ui.set_resolve_error("".into());
            let sh = sh.clone();
            let weak = weak.clone();
            std::thread::spawn(move || {
                let mut videos = vec![];
                let mut errors = vec![];
                for url in urls {
                    if cancel.load(Ordering::Acquire) {
                        break;
                    }
                    match kedi_factory::resolve_url(&url, playlist, cancel.clone()) {
                        Ok(mut rows) => videos.append(&mut rows),
                        Err(e) => errors.push(e),
                    }
                    videos.truncate(500);
                    if videos.len() == 500 {
                        break;
                    }
                }
                if videos.is_empty() && errors.is_empty() {
                    errors.push("Không tìm thấy video công khai trong nguồn này.".into());
                }
                let _ = weak.upgrade_in_event_loop(move |app| {
                    let mut s = sh.lock().expect("state");
                    if !Arc::ptr_eq(&s.cancel, &cancel) || cancel.load(Ordering::Acquire) {
                        return;
                    }
                    s.selected = vec![true; videos.len()];
                    s.stream_indices = vec![0; videos.len()];
                    s.resolved = videos;
                    push_resolved(&app, &s);
                    app.global::<KediFactory>().set_resolving(false);
                    app.global::<KediFactory>()
                        .set_resolve_error(errors.join("\n").into());
                });
            });
        });
    }
    {
        let sh = shared.clone();
        let weak = app.as_weak();
        app.global::<KediFactory>().on_clear_resolved(move || {
            let mut s = sh.lock().expect("state");
            s.cancel.store(true, Ordering::Release);
            s.resolved.clear();
            s.selected.clear();
            s.stream_indices.clear();
            if let Some(app) = weak.upgrade() {
                push_resolved(&app, &s);
                app.global::<KediFactory>().set_resolving(false);
            }
        });
    }
    {
        let sh = shared.clone();
        let weak = app.as_weak();
        app.global::<KediFactory>().on_toggle_video(move |index| {
            let mut s = sh.lock().expect("state");
            if let Some(value) = s.selected.get_mut(index as usize) {
                *value = !*value;
            }
            if let Some(app) = weak.upgrade() {
                push_resolved(&app, &s);
            }
        });
    }
    for select in [true, false] {
        let sh = shared.clone();
        let weak = app.as_weak();
        let cb = move || {
            let mut s = sh.lock().expect("state");
            s.selected.fill(select);
            if let Some(app) = weak.upgrade() {
                push_resolved(&app, &s);
            }
        };
        if select {
            app.global::<KediFactory>().on_select_all(cb);
        } else {
            app.global::<KediFactory>().on_deselect_all(cb);
        }
    }
    {
        let sh = shared.clone();
        let weak = app.as_weak();
        app.global::<KediFactory>()
            .on_set_stream(move |index, quality| {
                let mut s = sh.lock().expect("state");
                if let Some(v) = s.resolved.get(index as usize) {
                    if quality >= 0 && (quality as usize) < v.streams.len() {
                        s.stream_indices[index as usize] = quality as usize;
                    }
                }
                if let Some(app) = weak.upgrade() {
                    push_resolved(&app, &s);
                }
            });
    }
    {
        let sh = shared.clone();
        let weak = app.as_weak();
        app.global::<KediFactory>().on_pick_download_dir(move || {
            let sh = sh.clone();
            let weak = weak.clone();
            std::thread::spawn(move || {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        let ui = app.global::<KediFactory>();
                        if ui.get_rendering() || ui.get_downloading() {
                            return;
                        }
                        sh.lock().expect("state").download_dir = path.clone();
                        ui.set_download_dir(path.display().to_string().into());
                    });
                }
            });
        });
    }
    {
        let sh = shared.clone();
        let weak = app.as_weak();
        app.global::<KediFactory>().on_pick_local_files(move || {
            let sh = sh.clone();
            let weak = weak.clone();
            std::thread::spawn(move || {
                let Some(files) = rfd::FileDialog::new()
                    .add_filter(
                        "Ảnh / video",
                        &[
                            "mp4", "mov", "mkv", "avi", "webm", "jpg", "jpeg", "png", "webp",
                        ],
                    )
                    .pick_files()
                else {
                    return;
                };
                let _ = weak.upgrade_in_event_loop(move |app| {
                    if app.global::<KediFactory>().get_rendering() {
                        return;
                    }
                    let mut s = sh.lock().expect("state");
                    for path in files {
                        if path.is_file() && !s.assets.contains(&path) && s.assets.len() < 100 {
                            s.assets.push(path);
                        }
                    }
                    publish_assets(&app, &s);
                });
            });
        });
    }
    {
        let sh = shared.clone();
        let weak = app.as_weak();
        app.global::<KediFactory>().on_clear_assets(move || {
            if let Some(app) = weak.upgrade() {
                if !app.global::<KediFactory>().get_rendering() {
                    let mut s = sh.lock().expect("state");
                    s.assets.clear();
                    publish_assets(&app, &s);
                }
            }
        });
    }
    {
        let sh = shared.clone();
        let weak = app.as_weak();
        app.global::<KediFactory>().on_start_download(move || {
            let Some(app) = weak.upgrade() else { return };
            let id = {
                let mut s = sh.lock().expect("state");
                if s.active_job.is_some() {
                    return;
                }
                let selected: Vec<_> = s
                    .resolved
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| s.selected.get(*i) == Some(&true))
                    .map(|(i, v)| (v.clone(), s.stream_indices[i]))
                    .collect();
                if selected.is_empty() {
                    return;
                }
                let dir = s.download_dir.clone();
                let id = s.factory.create_job(JobInput::VideoUrl {
                    urls: selected.iter().map(|(v, _)| v.url.clone()).collect(),
                });
                let job = s.factory.job_mut(&id).unwrap();
                job.download_dir = Some(dir);
                for (index, (video, quality)) in selected.iter().enumerate() {
                    if let Some(stream) = video.streams.get(*quality) {
                        job.selected_streams.insert(index, stream.id.clone());
                    }
                }
                id
            };
            start_job(&app, sh.clone(), id);
        });
    }
    {
        let sh = shared.clone();
        let weak = app.as_weak();
        app.global::<KediFactory>().on_retry_job(move |id| {
            if let Some(app) = weak.upgrade() {
                let eligible = sh
                    .lock()
                    .expect("state")
                    .factory
                    .jobs
                    .iter()
                    .any(|j| j.job_id == id.as_str() && j.stage == FactoryStage::Failed);
                if eligible {
                    start_job(&app, sh.clone(), id.to_string());
                }
            }
        });
    }
    {
        let sh = shared.clone();
        let weak = app.as_weak();
        app.global::<KediFactory>().on_cancel_job(move |id| {
            let mut s = sh.lock().expect("state");
            if s.active_job.as_deref() == Some(id.as_str()) {
                s.download_cancel.store(true, Ordering::Release);
                if let Some(j) = s.factory.job_mut(&id) {
                    j.advance(FactoryStage::Cancelling);
                }
            }
            if let Some(app) = weak.upgrade() {
                push_jobs(&app, &s);
            }
        });
    }
    {
        let sh = shared.clone();
        app.global::<KediFactory>().on_cancel_download(move || {
            sh.lock()
                .expect("state")
                .download_cancel
                .store(true, Ordering::Release)
        });
    }
    {
        let sh = shared.clone();
        let weak = app.as_weak();
        app.global::<KediFactory>().on_refresh_jobs(move || {
            if let Some(app) = weak.upgrade() {
                push_jobs(&app, &sh.lock().expect("state"));
            }
        });
    }
    {
        let sh = shared.clone();
        let weak = app.as_weak();
        app.global::<KediFactory>().on_open_output(move |id| {
            let path = sh
                .lock()
                .expect("state")
                .factory
                .jobs
                .iter()
                .find(|j| j.job_id == id.as_str())
                .and_then(|j| j.output_paths.first())
                .cloned();
            if let (Some(path), Some(app)) = (path, weak.upgrade()) {
                if let Err(e) = crate::platform::reveal(&path.to_string_lossy()) {
                    app.global::<KediFactory>()
                        .set_global_error(e.to_string().into());
                }
            }
        });
    }
    {
        let weak = app.as_weak();
        app.global::<KediFactory>().on_pick_audio(move || {
            let weak = weak.clone();
            std::thread::spawn(move || {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Âm thanh", &["mp3", "wav", "m4a", "aac", "flac", "ogg"])
                    .pick_file()
                {
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        if !app.global::<KediFactory>().get_rendering() {
                            app.global::<KediFactory>()
                                .set_audio_path(path.display().to_string().into());
                        }
                    });
                }
            });
        });
        let weak = app.as_weak();
        app.global::<KediFactory>().on_clear_audio(move || {
            if let Some(app) = weak.upgrade() {
                app.global::<KediFactory>().set_audio_path("".into());
            }
        });
    }
    {
        let weak = app.as_weak();
        app.global::<KediFactory>().on_prepare_script(move || {
            if let Some(app) = weak.upgrade() {
                let ui = app.global::<KediFactory>();
                if ui.get_rendering() { return; }
                let text = [ui.get_product_title(), ui.get_product_price(), ui.get_product_description()]
                    .iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(". ");
                if text.is_empty() { ui.set_global_error("Nhập thông tin sản phẩm trước khi lấy bản nháp.".into()); }
                else { ui.set_script(text.into()); ui.set_global_error("".into()); }
            }
        });
    }
    {
        let sh = shared.clone();
        let weak = app.as_weak();
        app.global::<KediFactory>().on_render(move || {
            let Some(app) = weak.upgrade() else { return };
            let ui = app.global::<KediFactory>(); if ui.get_rendering() { return; }
            let seconds = ui.get_seconds().replace(',', ".").parse::<f64>().unwrap_or(f64::NAN);
            let (paths, dir, cancel) = { let mut s = sh.lock().expect("state"); s.render_cancel = Arc::new(AtomicBool::new(false)); (s.assets.clone(), s.download_dir.clone(), s.render_cancel.clone()) };
            let size = match ui.get_aspect_index() { 1 => (1280,720), 2 => (720,720), 3 => (1080,1920), _ => (720,1280) };
            let request = concat_studio::factory_edit::EditRequest {
                paths, seconds, size, script: ui.get_script().to_string(), audio_mode: ui.get_audio_mode(),
                voice_index: ui.get_voice_index().max(0) as usize, audio_path: ui.get_audio_path().to_string(),
            };
            let metadata = serde_json::json!({ "productTitle": ui.get_product_title().as_str(), "price": ui.get_product_price().as_str(), "description": ui.get_product_description().as_str(), "sourceUrl": ui.get_product_url_input().as_str() });
            ui.set_rendering(true); ui.set_render_progress(0.0); ui.set_output_path("".into()); ui.set_global_error("".into());
            let weak = weak.clone();
            std::thread::spawn(move || {
                let result = (|| {
                    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                    let root = dir.join(format!("render-{}", task_id()));
                    let progress = weak.clone();
                    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
                    let output = concat_studio::factory_edit::render(&concat_studio::factory_edit::python(&repo), &crate::studio_scenes::ffmpeg()?, &crate::studio_scenes::font()?, &request, &root, &cancel, move |status| {
                        let _ = progress.upgrade_in_event_loop(move |app| app.global::<KediFactory>().set_status(status.into()));
                    })?;
                    std::fs::write(root.join("product.json"), serde_json::to_vec_pretty(&metadata).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
                    kedi_factory::verify_media(&ffprobe()?, &output)?;
                    if cancel.load(Ordering::Acquire) { return Err("Đã dừng dựng video.".to_string()); }
                    register(&output, None)?;
                    Ok::<PathBuf,String>(output)
                })();
                let _ = weak.upgrade_in_event_loop(move |app| {
                    let ui = app.global::<KediFactory>(); ui.set_rendering(false);
                    match result { Ok(path) => { ui.set_output_path(path.display().to_string().into()); ui.set_step(5); ui.set_status("Đã xuất MP4 có âm thanh và lưu vào thư viện video.".into()); }, Err(e) => ui.set_global_error(e.into()) }
                });
            });
        });
    }
    {
        let sh = shared.clone();
        app.global::<KediFactory>().on_cancel_render(move || {
            sh.lock()
                .expect("state")
                .render_cancel
                .store(true, Ordering::Release)
        });
        let weak = app.as_weak();
        app.global::<KediFactory>().on_reveal_render(move || {
            if let Some(app) = weak.upgrade() {
                let path = app.global::<KediFactory>().get_output_path();
                if let Err(e) = crate::platform::reveal(&path) {
                    app.global::<KediFactory>()
                        .set_global_error(e.to_string().into());
                }
            }
        });
    }
}
