// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Auto language workspace: a small native front end to pyVideoTrans CLI.
use crate::{studio_video_preview, ui};
use slint::{ComponentHandle, Model};
use std::{
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{atomic::{AtomicBool, Ordering}, mpsc, Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

static CHILD: OnceLock<Mutex<Option<Child>>> = OnceLock::new();
static ROOT_OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
static CANCEL_REQUESTED: AtomicBool = AtomicBool::new(false);
fn child_slot() -> &'static Mutex<Option<Child>> {
    CHILD.get_or_init(|| Mutex::new(None))
}

fn root_override() -> &'static Mutex<Option<PathBuf>> {
    ROOT_OVERRIDE.get_or_init(|| Mutex::new(None))
}

fn save_pyvideotrans_root(root: &Path) -> Result<(), String> {
    if !root.join("cli.py").is_file() {
        return Err("Thư mục đã chọn không có cli.py của pyVideoTrans.".into());
    }
    let dirs = concat_host::AppDirs::locate().map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&dirs.config).map_err(|error| error.to_string())?;
    std::fs::write(dirs.config.join("pyvideotrans-path.txt"), root.to_string_lossy().as_bytes())
        .map_err(|error| error.to_string())?;
    *root_override().lock().unwrap_or_else(|p| p.into_inner()) = Some(root.to_path_buf());
    Ok(())
}

fn pyvideotrans_root() -> Option<PathBuf> {
    if let Some(root) = std::env::var_os("PYVIDEOTRANS_DIR").map(PathBuf::from) {
        if root.join("cli.py").is_file() {
            return Some(root);
        }
    }
    if let Some(root) = root_override().lock().unwrap_or_else(|p| p.into_inner()).clone() {
        if root.join("cli.py").is_file() { return Some(root); }
    }
    if let Ok(dirs) = concat_host::AppDirs::locate() {
        if let Ok(saved) = std::fs::read_to_string(dirs.config.join("pyvideotrans-path.txt")) {
            let root = PathBuf::from(saved.trim());
            if root.join("cli.py").is_file() {
                *root_override().lock().unwrap_or_else(|p| p.into_inner()) = Some(root.clone());
                return Some(root);
            }
        }
    }
    let exe = std::env::current_exe().ok()?;
    let cwd = std::env::current_dir().ok();
    for base in exe.ancestors().chain(cwd.as_deref().into_iter()) {
        for candidate in [base.join("pyvideotrans"), base.to_path_buf()] {
            if candidate.join("cli.py").is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn language_code(index: i32, target: bool) -> &'static str {
    const SOURCE: [&str; 9] = ["auto", "vi", "en", "zh-cn", "ja", "ko", "es", "fr", "de"];
    const TARGET: [&str; 8] = ["vi", "en", "zh-cn", "ja", "ko", "es", "fr", "de"];
    if target {
        TARGET.get(index.clamp(0, TARGET.len() as i32 - 1) as usize).copied().unwrap_or("vi")
    } else {
        SOURCE.get(index.clamp(0, SOURCE.len() as i32 - 1) as usize).copied().unwrap_or("auto")
    }
}

fn target_voice_options(target: &'static str) -> Vec<(String, String)> {
    let Some(root) = pyvideotrans_root() else { return Vec::new() };
    let path = root.join("videotrans").join("voicejson").join("edge_tts.json");
    let Ok(json) = std::fs::read_to_string(path) else { return Vec::new() };
    let Ok(data) = serde_json::from_str::<std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>>(&json) else { return Vec::new() };
    let language = match target { "zh-cn" => "zh", code => code.split('-').next().unwrap_or(code) };
    data.get(language).into_iter().flat_map(|voices| voices.iter())
        .map(|(label, role)| (label.clone(), role.clone())).collect()
}

fn update_voice_options(app: &slint::Weak<ui::App>) {
    let Some(window) = app.upgrade() else { return };
    let state = window.global::<ui::AutoLanguage>();
    let options = target_voice_options(language_code(state.get_target_language(), true));
    if options.is_empty() {
        state.set_voice_options(slint::ModelRc::default());
        state.set_voice_role("".into());
        state.set_status("Không nạp được danh sách giọng Edge-TTS. Chọn đúng thư mục pyVideoTrans.".into());
        return;
    }
    state.set_voice_options(slint::ModelRc::new(slint::VecModel::from(options.iter().map(|(label, _)| label.clone().into()).collect::<Vec<slint::SharedString>>())));
    let current = state.get_voice_role_index().clamp(0, options.len() as i32 - 1) as usize;
    state.set_voice_role_index(current as i32);
    state.set_voice_role(options[current].1.clone().into());
}

fn translated_text(path: &Path) -> String {
    let Ok(text) = std::fs::read_to_string(path) else { return String::new() };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.chars().all(|c| c.is_ascii_digit()) && !line.contains(" --> "))
        .collect::<Vec<_>>()
        .join("\n")
}

fn forward_lines<R>(stream: R, tx: mpsc::Sender<String>)
where
    R: Read + Send + 'static,
{
    std::thread::spawn(move || {
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            let _ = tx.send(line);
        }
    });
}

fn newest_video(folder: &Path) -> Option<PathBuf> {
    let mut videos: Vec<_> = std::fs::read_dir(folder).ok()?.flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("mp4")))
        .collect();
    videos.sort_by_key(|path| std::fs::metadata(path).and_then(|m| m.modified()).ok());
    videos.pop()
}

fn subtitle_for(folder: &Path, language: &str, fallback: bool) -> Option<PathBuf> {
    let mut subtitles: Vec<_> = std::fs::read_dir(folder).ok()?.flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("srt")))
        .collect();
    subtitles.sort();
    subtitles.iter().find(|path| {
        path.file_stem().and_then(|name| name.to_str()).is_some_and(|name| name.eq_ignore_ascii_case(language))
    }).cloned().or_else(|| fallback.then(|| subtitles.into_iter().next()).flatten())
}

fn publish_frame(app: &slint::Weak<ui::App>, path: PathBuf, is_output: bool) {
    let Some(app_dirs) = concat_host::AppDirs::locate().ok() else { return };
    let preview = studio_video_preview::local(&path, &app_dirs.data.join("language-previews"));
    // Slint Image is UI-thread-bound; transfer only the plain pixel buffer.
    let (pixels, width, height) = (preview.pixels, preview.width, preview.height);
    let preview_path = path.to_string_lossy().into_owned();
    let _ = slint::invoke_from_event_loop({
        let app = app.clone();
        move || if let Some(app) = app.upgrade() {
            let language = app.global::<ui::AutoLanguage>();
            let selected = if is_output { language.get_output_path() } else { language.get_source_path() };
            if selected.as_str() != preview_path { return; }
            let frame = if width == 0 || height == 0 {
                slint::Image::default()
            } else {
                slint::Image::from_rgba8(slint::SharedPixelBuffer::clone_from_slice(&pixels, width, height))
            };
            if is_output { language.set_translated_frame(frame); } else { language.set_source_frame(frame); }
        }
    });
}

fn set_status(app: &slint::Weak<ui::App>, status: String) {
    let _ = slint::invoke_from_event_loop({
        let app = app.clone();
        move || if let Some(app) = app.upgrade() { app.global::<ui::AutoLanguage>().set_status(status.into()); }
    });
}

fn run_translation_one(app: slint::Weak<ui::App>, input: PathBuf, source: &'static str, target: &'static str, voice_role: String) -> Result<ui::TranslationQueueItem, String> {
    let Some(root) = pyvideotrans_root() else {
        return Err("Không tìm thấy pyVideoTrans. Hãy chọn thư mục chứa cli.py.".into());
    };
    let Ok(dirs) = concat_host::AppDirs::locate() else {
        return Err("Không xác định được thư mục dữ liệu để lưu kết quả.".into());
    };
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    let output = dirs.data.join("language-translations").join(format!("job-{}-{stamp}", std::process::id()));
    if let Err(error) = std::fs::create_dir_all(&output) {
        return Err(format!("Không tạo được thư mục kết quả: {error}"));
    }

    // pyVideoTrans' full environment can be several gigabytes (Torch/CUDA).
    // Keep it with Concat's local app data rather than in the source checkout
    // or on the project drive, and point `uv run --no-sync` at that exact env.
    let runtime_env = dirs.data.join("pyvideotrans-env");
    let mut command = if root.join("cli.exe").is_file() {
        let mut cmd = Command::new(root.join("cli.exe"));
        cmd.current_dir(&root);
        cmd
    } else {
        let mut cmd = Command::new("uv");
        cmd.current_dir(&root)
            .env("UV_PROJECT_ENVIRONMENT", &runtime_env)
            .args(["run", "--no-sync", "--python", "3.10", "cli.py"]);
        cmd
    };
    command.env("PYTHONIOENCODING", "utf-8").env("PYTHONUTF8", "1");
    command.args([
        "--task", "vtv", "--name",
    ]).arg(&input).args([
        "--source_language_code", source, "--target_language_code", target, "--output-dir",
    ]).arg(&output);
    // The pyVideoTrans "No" voice role produces subtitles without dubbing.
    if voice_role.trim().is_empty() || voice_role == "No" {
        return Err("Chưa chọn được giọng đọc cho ngôn ngữ đích.".into());
    }
    command.args(["--tts_type", "0", "--voice_role"]).arg(voice_role.trim())
        .args(["--voice_autorate", "--align_sub_audio"]);
    command.arg("--verbose")
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());

    let Ok(mut child) = command.spawn() else {
        return Err(format!("Không khởi động được pyVideoTrans CLI tại {}. Kiểm tra môi trường Python.", root.display()));
    };
    #[cfg(windows)]
    let mut process_tree = match concat_studio::lux::own_process_tree(&child) {
        Ok(tree) => Some(tree),
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error.into());
        }
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let (tx, rx) = mpsc::channel::<String>();
    if let Some(stdout) = stdout { forward_lines(stdout, tx.clone()); }
    if let Some(stderr) = stderr { forward_lines(stderr, tx.clone()); }
    drop(tx);
    *child_slot().lock().unwrap_or_else(|p| p.into_inner()) = Some(child);
    set_status(&app, "pyVideoTrans đang nhận dạng, dịch, lồng tiếng và ghép video…".into());

    let mut last_line = String::new();
    let mut last_error = String::new();
    let mut log_lines = Vec::new();
    let status = loop {
        if CANCEL_REQUESTED.load(Ordering::Relaxed) {
            #[cfg(windows)]
            drop(process_tree.take());
            if let Some(child) = child_slot().lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
            break None;
        }
        while let Ok(line) = rx.try_recv() {
            if !line.trim().is_empty() {
                last_line = line.chars().rev().take(180).collect::<String>().chars().rev().collect();
                let lower = line.to_ascii_lowercase();
                if ["error", "exception", "traceback", "failed"].iter().any(|word| lower.contains(word)) { last_error = line.clone(); }
                log_lines.push(line);
                set_status(&app, last_line.clone());
            }
        }
        let result = {
            let mut slot = child_slot().lock().unwrap_or_else(|p| p.into_inner());
            slot.as_mut().map(Child::try_wait)
        };
        match result {
            Some(Ok(Some(status))) => break Some(status),
            Some(Err(error)) => {
                last_line = format!("Không đọc được trạng thái tiến trình pyVideoTrans: {error}");
                if let Some(child) = child_slot().lock().unwrap_or_else(|p| p.into_inner()).as_mut() { let _ = child.kill(); }
                break None;
            }
            None => break None,
            Some(Ok(None)) => {}
        }
        std::thread::sleep(Duration::from_millis(250));
    };
    child_slot().lock().unwrap_or_else(|p| p.into_inner()).take();
    while let Ok(line) = rx.try_recv() { if !line.trim().is_empty() { last_line = line.clone(); log_lines.push(line); } }
    let _ = std::fs::write(output.join("pyvideotrans.log"), log_lines.join("\n"));
    if CANCEL_REQUESTED.load(Ordering::Relaxed) { return Err("Đã hủy dịch và lồng tiếng video.".into()); }

    if status.is_some_and(|status| status.success()) {
        if let Some(video) = newest_video(&output) {
            let dubbed_audio = output.join(format!("{target}.m4a"));
            if !std::fs::metadata(&dubbed_audio).is_ok_and(|metadata| metadata.len() > 0) {
                return Err(format!("Chưa tạo được âm thanh lồng tiếng. Xem {}", output.join("pyvideotrans.log").display()));
            }
            let path = video.to_string_lossy().into_owned();
            let _ = crate::studio_video_library::record(&video, "Auto ngôn ngữ");
            let source_text = {
                subtitle_for(&output, source, false).map(|p| translated_text(&p))
                    .filter(|text| !text.is_empty())
                    .unwrap_or_else(|| "Phụ đề nguồn chưa được pyVideoTrans xuất thành tệp riêng.".to_owned())
            };
            let target_text = subtitle_for(&output, target, false).map(|p| translated_text(&p))
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| "pyVideoTrans chưa xuất tệp phụ đề mang mã ngôn ngữ đích.".to_owned());
            let _ = slint::invoke_from_event_loop({
                let app = app.clone();
                move || if let Some(app) = app.upgrade() { app.global::<ui::StudioBulk>().invoke_refresh_videos(); }
            });
            return Ok(ui::TranslationQueueItem {
                source_path: input.to_string_lossy().into_owned().into(),
                status: "Hoàn tất".into(),
                output_path: path.into(),
                source_text: source_text.into(),
                translated_text: target_text.into(),
                detail: format!("Đã dịch và lồng tiếng · {voice_role}").into(),
            });
        } else {
            return Err(format!("pyVideoTrans kết thúc nhưng không tạo MP4 trong {}. {}", output.display(), last_line));
        }
    } else {
        let detail = if !last_error.is_empty() { last_error } else if last_line.is_empty() { "Xem pyvideotrans.log trong thư mục kết quả để biết chi tiết.".into() } else { last_line };
        return Err(format!("Tác vụ chưa hoàn tất. {detail}\nNhật ký: {}", output.join("pyvideotrans.log").display()));
    }
}

fn queue_item(source: &Path, status: &str) -> ui::TranslationQueueItem {
    ui::TranslationQueueItem {
        source_path: source.to_string_lossy().into_owned().into(),
        status: status.into(), output_path: "".into(), source_text: "".into(), translated_text: "".into(), detail: "".into(),
    }
}

fn publish_queue(app: &slint::Weak<ui::App>, items: &[ui::TranslationQueueItem]) {
    let waiting = items.iter().filter(|item| item.status == "Đang chờ").count() as i32;
    let running = items.iter().filter(|item| item.status == "Đang dịch").count() as i32;
    let completed = items.iter().filter(|item| item.status == "Hoàn tất").count() as i32;
    let failed = items.iter().filter(|item| item.status == "Lỗi" || item.status == "Đã hủy").count() as i32;
    let items = items.to_vec();
    let _ = slint::invoke_from_event_loop({
        let app = app.clone();
        move || if let Some(window) = app.upgrade() {
            let state = window.global::<ui::AutoLanguage>();
            state.set_queue_items(slint::ModelRc::new(slint::VecModel::from(items)));
            state.set_queue_waiting(waiting);
            state.set_queue_running(running);
            state.set_queue_completed(completed);
            state.set_queue_failed(failed);
        }
    });
}

fn show_queue_item(app: &slint::Weak<ui::App>, index: usize) {
    let weak = app.clone();
    let _ = slint::invoke_from_event_loop(move || {
        let Some(window) = weak.upgrade() else { return };
        let state = window.global::<ui::AutoLanguage>();
        let Some(item) = state.get_queue_items().row_data(index) else { return };
        let source = item.source_path.to_string();
        let output = item.output_path.to_string();
        state.set_source_path(source.clone().into());
        state.set_output_path(output.clone().into());
        state.set_source_text(item.source_text);
        state.set_translated_text(item.translated_text);
        state.set_result_ready(!output.is_empty());
        state.set_selected_detail(item.detail);
        state.set_selected_queue_index(index as i32);
        state.set_source_frame(Default::default());
        state.set_translated_frame(Default::default());
        let weak = window.as_weak();
        std::thread::spawn(move || {
            if !source.is_empty() { publish_frame(&weak, PathBuf::from(source), false); }
            if !output.is_empty() { publish_frame(&weak, PathBuf::from(output), true); }
        });
    });
}

fn run_translation(app: slint::Weak<ui::App>, inputs: Vec<PathBuf>, source: &'static str, target: &'static str, voice_role: String) {
    let total = inputs.len();
    let mut completed = 0usize;
    let mut errors = Vec::new();
    let mut items: Vec<_> = inputs.iter().map(|input| queue_item(input, "Đang chờ")).collect();
    publish_queue(&app, &items);
    for (index, input) in inputs.iter().enumerate() {
        if CANCEL_REQUESTED.load(Ordering::Relaxed) { break; }
        items[index] = queue_item(input, "Đang dịch");
        publish_queue(&app, &items);
        let progress = format!("Đang dịch video {}/{}: {}", index + 1, total, input.file_name().unwrap_or_default().to_string_lossy());
        let _ = slint::invoke_from_event_loop({
            let app = app.clone();
            move || if let Some(window) = app.upgrade() {
                let state = window.global::<ui::AutoLanguage>();
                state.set_busy(true);
                state.set_status(progress.into());
            }
        });
        match run_translation_one(app.clone(), input.clone(), source, target, voice_role.clone()) {
            Ok(result) => {
                items[index] = result;
                completed += 1;
                publish_queue(&app, &items);
                let weak = app.clone();
                let _ = slint::invoke_from_event_loop(move || if let Some(window) = weak.upgrade() {
                    let selected = window.global::<ui::AutoLanguage>().get_selected_queue_index();
                    if selected < 0 || selected == index as i32 { show_queue_item(&weak, index); }
                });
            }
            Err(detail) => {
                items[index].status = if CANCEL_REQUESTED.load(Ordering::Relaxed) { "Đã hủy" } else { "Lỗi" }.into();
                items[index].detail = detail.clone().into();
                set_status(&app, detail);
                publish_queue(&app, &items);
                errors.push(input.file_name().unwrap_or_default().to_string_lossy().into_owned());
            }
        }
    }
    if CANCEL_REQUESTED.load(Ordering::Relaxed) {
        for item in &mut items {
            if item.status == "Đang chờ" || item.status == "Đang dịch" { item.status = "Đã hủy".into(); }
        }
        publish_queue(&app, &items);
    }
    let message = if CANCEL_REQUESTED.load(Ordering::Relaxed) {
            format!("Đã hủy hàng chờ. Hoàn tất {completed}/{total} video.")
        } else if errors.is_empty() {
            format!("Đã dịch xong {completed}/{total} video. Kết quả được lưu trong Danh sách video.")
        } else {
            format!("Đã dịch xong {completed}/{total}; {} video lỗi: {}", errors.len(), errors.join(", "))
        };
    let _ = slint::invoke_from_event_loop({
        let app = app.clone();
        move || if let Some(window) = app.upgrade() {
            let state = window.global::<ui::AutoLanguage>();
            state.set_busy(false);
            state.set_status(message.into());
        }
    });
}

pub(crate) fn bind(app: &ui::App) {
    let weak = app.as_weak();
    update_voice_options(&weak);
    let weak = app.as_weak();
    app.global::<ui::AutoLanguage>().on_refresh_voices(move || update_voice_options(&weak));

    let weak = app.as_weak();
    app.global::<ui::AutoLanguage>().on_select_queue_item(move |index| {
        if index >= 0 { show_queue_item(&weak, index as usize); }
    });

    let weak = app.as_weak();
    app.global::<ui::AutoLanguage>().on_choose_video(move || {
        let Some(paths) = crate::platform::pick_files("Chọn video để dịch", Some(("Video", &["mp4", "mkv", "mov", "m4v", "webm", "avi"]))) else { return };
        let Some(path) = paths.into_iter().next() else { return };
        if let Some(app) = weak.upgrade() {
            let state = app.global::<ui::AutoLanguage>();
            state.set_source_path(path.to_string_lossy().into_owned().into());
            state.set_source_files_json(serde_json::to_string(&[path.to_string_lossy().to_string()]).unwrap_or_else(|_| "[]".into()).into());
            state.set_source_file_count(1);
            state.set_output_path("".into());
            state.set_result_ready(false);
            state.set_translated_frame(Default::default());
            state.set_source_text("".into());
            state.set_translated_text("".into());
            state.set_queue_items(slint::ModelRc::default());
            state.set_selected_queue_index(-1); state.set_selected_detail("".into());
            state.set_queue_waiting(0); state.set_queue_running(0); state.set_queue_completed(0); state.set_queue_failed(0);
        }
        let weak = weak.clone();
        std::thread::spawn(move || publish_frame(&weak, path, false));
    });

    let weak = app.as_weak();
    app.global::<ui::AutoLanguage>().on_choose_videos(move || {
        let Some(paths) = crate::platform::pick_files("Chọn các video để dịch", Some(("Video", &["mp4", "mkv", "mov", "m4v", "webm", "avi"]))) else { return };
        let paths: Vec<PathBuf> = paths.into_iter().filter(|path| path.is_file()).collect();
        if paths.is_empty() { return; }
        if let Some(app) = weak.upgrade() {
            let state = app.global::<ui::AutoLanguage>();
            state.set_source_path(paths[0].to_string_lossy().into_owned().into());
            state.set_source_file_count(paths.len() as i32);
            let json = serde_json::to_string(&paths.iter().map(|path| path.to_string_lossy().to_string()).collect::<Vec<_>>()).unwrap_or_else(|_| "[]".into());
            state.set_source_files_json(json.into());
            state.set_output_path("".into());
            state.set_result_ready(false);
            state.set_translated_frame(Default::default());
            state.set_source_text("".into());
            state.set_translated_text("".into());
            state.set_queue_items(slint::ModelRc::default());
            state.set_selected_queue_index(-1); state.set_selected_detail("".into());
            state.set_queue_waiting(0); state.set_queue_running(0); state.set_queue_completed(0); state.set_queue_failed(0);
        }
        let weak = weak.clone();
        let first = paths[0].clone();
        std::thread::spawn(move || publish_frame(&weak, first, false));
    });

    let weak = app.as_weak();
    app.global::<ui::AutoLanguage>().on_choose_runtime(move || {
        let Some(root) = crate::platform::pick_folder("Chọn thư mục pyVideoTrans", "") else { return };
        match save_pyvideotrans_root(&root) {
            Ok(()) => if let Some(app) = weak.upgrade() {
                app.global::<ui::AutoLanguage>().set_status(format!("Đã kết nối pyVideoTrans tại {}. Chọn giọng đọc trước khi dịch.", root.display()).into());
                update_voice_options(&app.as_weak());
            },
            Err(error) => if let Some(app) = weak.upgrade() { app.global::<ui::AutoLanguage>().set_status(error.into()); },
        }
    });

    let weak_translate = app.as_weak();
    app.global::<ui::AutoLanguage>().on_translate(move || {
        let Some(app) = weak_translate.upgrade() else { return };
        let state = app.global::<ui::AutoLanguage>();
        if state.get_busy() { return; }
        let source_path = PathBuf::from(state.get_source_path().to_string());
        let mut inputs = serde_json::from_str::<Vec<String>>(&state.get_source_files_json().to_string()).unwrap_or_default().into_iter().map(PathBuf::from).collect::<Vec<_>>();
        if inputs.is_empty() && source_path.is_file() { inputs.push(source_path); }
        inputs.retain(|path| path.is_file());
        let mut seen = std::collections::HashSet::new();
        inputs.retain(|path| path.canonicalize().ok().is_some_and(|path| seen.insert(path)));
        if inputs.is_empty() { state.set_status("Tệp video không còn tồn tại. Hãy chọn lại video.".into()); return; }
        state.set_source_file_count(inputs.len() as i32);
        let source = language_code(state.get_source_language(), false);
        let target = language_code(state.get_target_language(), true);
        let voices = target_voice_options(target);
        if voices.is_empty() { state.set_status("Không tìm thấy danh sách Edge-TTS trong pyVideoTrans. Hãy chọn thư mục pyVideoTrans.".into()); return; }
        let voice_index = state.get_voice_role_index().clamp(0, voices.len() as i32 - 1) as usize;
        let voice_role = voices[voice_index].1.clone();
        if source == target { state.set_status("Ngôn ngữ nguồn và đích đang giống nhau.".into()); return; }
        state.set_busy(true);
        CANCEL_REQUESTED.store(false, Ordering::Relaxed);
        state.set_output_path("".into());
        state.set_result_ready(false);
        state.set_translated_frame(Default::default());
        state.set_selected_queue_index(-1);
        state.set_selected_detail("".into());
        state.set_queue_filter(0);
        state.set_status("Đang chuẩn bị pyVideoTrans…".into());
        let weak = weak_translate.clone();
        std::thread::spawn(move || run_translation(weak, inputs, source, target, voice_role));
    });

    app.global::<ui::AutoLanguage>().on_cancel(|| {
        CANCEL_REQUESTED.store(true, Ordering::Relaxed);
    });
    let weak_source = app.as_weak();
    app.global::<ui::AutoLanguage>().on_open_source(move || {
        if let Some(app) = weak_source.upgrade() {
            let path = app.global::<ui::AutoLanguage>().get_source_path().to_string();
            if !path.is_empty() { let _ = opener::open(path); }
        }
    });
    let weak_output = app.as_weak();
    app.global::<ui::AutoLanguage>().on_open_output(move || {
        if let Some(app) = weak_output.upgrade() {
            let path = app.global::<ui::AutoLanguage>().get_output_path().to_string();
            if !path.is_empty() { let _ = opener::open(path); }
        }
    });
    let weak_reveal = app.as_weak();
    app.global::<ui::AutoLanguage>().on_reveal_output(move || {
        if let Some(app) = weak_reveal.upgrade() {
            let path = app.global::<ui::AutoLanguage>().get_output_path().to_string();
            if !path.is_empty() { let _ = opener::reveal(path); }
        }
    });
}

