// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native storyboard editing and frozen local render jobs. Providers stay explicit.
use crate::ui::{App, ProductionScenes, ProductionStudio, SceneCardRow};
use concat_studio::{
    storyboard::{Direction, Shot, Storyboard, VisualLayer},
    storyboard_render::{RenderSize, render_board},
};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{
    cell::RefCell,
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

const STYLES: &[&str] = &["2D", "2.5D", "Phong cách 3D"];
const CAMERAS: &[&str] = &["Toàn cảnh", "Trung cảnh", "Cận mặt", "Góc cao", "Góc thấp"];
const MOTIONS: &[&str] = &["Đứng yên", "Đẩy vào", "Lùi ra", "Máy lia ngang"];
#[derive(Default)]
struct State {
    board: Storyboard,
    selected: Option<usize>,
    undo: Vec<Storyboard>,
    redo: Vec<Storyboard>,
    root: Option<PathBuf>,
    output: Option<PathBuf>,
    cancel: Option<Arc<AtomicBool>>,
    layer: Option<usize>,
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }
// Slint images remain on their owning UI thread. Reuse decoded thumbnails
// across selection/phase changes instead of decoding up to 25 images per publish.
thread_local! { static THUMBNAILS: RefCell<HashMap<PathBuf, (std::time::SystemTime, u64, slint::Image)>> = RefCell::new(HashMap::new()); }
#[cfg(debug_assertions)]
thread_local! { static SMOKE_ROOT: RefCell<Option<PathBuf>> = const {RefCell::new(None)}; }
enum SaveJob {
    Board(PathBuf, Storyboard),
    Flush(mpsc::Sender<()>),
}
static SAVER: OnceLock<Option<mpsc::Sender<SaveJob>>> = OnceLock::new();

fn persist(root: &Path, board: &Storyboard) {
    let sender = SAVER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<SaveJob>();
        std::thread::Builder::new()
            .name("storyboard-save".into())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    match job {
                        SaveJob::Board(root, board) => {
                            if let Err(e) = write_board(&root.join("draft.json"), &board) {
                                log::error!("Storyboard autosave: {e}");
                            }
                        }
                        SaveJob::Flush(done) => {
                            let _ = done.send(());
                        }
                    }
                }
            })
            .ok()
            .map(|_| tx)
    });
    if let Some(tx) = sender {
        let _ = tx.send(SaveJob::Board(root.to_owned(), board.clone()));
    }
}
pub(crate) fn shutdown(app: &App) {
    STATE.with_borrow_mut(|s| {
        if let Some(cancel) = &s.cancel {
            cancel.store(true, Ordering::Release);
        }
        if let Err(e) = commit(app, s) {
            log::error!("Storyboard close: {e}");
        }
    });
    if let Some(Some(tx)) = SAVER.get() {
        let (done, rx) = mpsc::channel();
        if tx.send(SaveJob::Flush(done)).is_ok() {
            let _ = rx.recv_timeout(std::time::Duration::from_secs(5));
        }
    }
}
fn write_board(path: &Path, board: &Storyboard) -> Result<(), String> {
    board.validate()?;
    let parent = path.parent().ok_or("Thư mục không hợp lệ")?;
    std::fs::create_dir_all(parent).map_err(|_| "Không tạo được thư mục phân cảnh")?;
    let temporary = parent.join(format!(".storyboard-{:032x}.tmp", rand_id()));
    std::fs::write(
        &temporary,
        serde_json::to_vec_pretty(board).map_err(|_| "Không mã hóa được phân cảnh")?,
    )
    .map_err(|_| "Không ghi được phân cảnh")?;
    if std::fs::rename(&temporary, path).is_err() {
        let _ = std::fs::remove_file(&temporary);
        return Err("Không thay được file phân cảnh; kiểm tra quyền ghi".into());
    }
    Ok(())
}
fn rand_id() -> u128 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        + NEXT.fetch_add(1, Ordering::Relaxed) as u128
}
fn read_board(path: &Path) -> Result<Storyboard, String> {
    let f = std::fs::File::open(path).map_err(|_| "Không đọc được tệp phân cảnh")?;
    let mut bytes = vec![];
    f.take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Không đọc được tệp")?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("Tệp phân cảnh quá lớn".into());
    }
    let board: Storyboard = serde_json::from_slice(&bytes)
        .map_err(|_| "Tệp không đúng định dạng phân cảnh Concat v1")?;
    board.validate()?;
    Ok(board)
}
fn root_for_project() -> Result<PathBuf, String> {
    #[cfg(debug_assertions)]
    if let Some(root) = SMOKE_ROOT.with_borrow(Clone::clone) {
        return Ok(root);
    }
    let mut project = None;
    crate::host::Shell::with(|shell, _| {
        project = shell
            .studio
            .borrow()
            .session
            .as_ref()
            .map(|s| PathBuf::from(s.path()));
    });
    if let Some(path) = project {
        return Ok(path.join(".concat-storyboard"));
    }
    Ok(concat_host::AppDirs::locate()?
        .data
        .join("studio/storyboard"))
}
fn checkpoint(s: &mut State) {
    s.undo.push(s.board.clone());
    if s.undo.len() > 40 {
        s.undo.remove(0);
    }
    s.redo.clear();
    s.output = None;
    s.board.output = None;
}
fn choice(index: i32, values: &[&str]) -> String {
    usize::try_from(index)
        .ok()
        .and_then(|i| values.get(i))
        .unwrap_or(&values[0])
        .to_string()
}
fn index(value: &str, values: &[&str]) -> i32 {
    values.iter().position(|s| *s == value).unwrap_or(0) as i32
}
fn commit(app: &App, s: &mut State) -> Result<(), String> {
    let Some(i) = s.selected.filter(|i| *i < s.board.shots.len()) else {
        return Ok(());
    };
    let ui = app.global::<ProductionScenes>();
    let old = s.board.shots[i].clone();
    let mut shot = old.clone();
    shot.title = ui.get_title().into();
    shot.narration = ui.get_narration().into();
    shot.duration = ui.get_duration() as f64;
    shot.caption = ui.get_caption().into();
    shot.source_note = ui.get_source_note().into();
    shot.asset_notes = ui.get_asset_notes().into();
    if let Some(layer) = s.layer.and_then(|i| shot.layers.get_mut(i)) {
        layer.x = ui.get_layer_x() as f64;
        layer.y = ui.get_layer_y() as f64;
        layer.width = ui.get_layer_width() as f64;
        layer.opacity = (ui.get_layer_opacity() as f64 / 100.0).clamp(0.0, 1.0);
    }
    shot.direction = Direction {
        style: choice(ui.get_style(), STYLES),
        camera: choice(ui.get_camera(), CAMERAS),
        motion: choice(ui.get_motion(), MOTIONS),
        setting: ui.get_setting().into(),
        lighting: ui.get_lighting().into(),
        expression: ui.get_expression().into(),
        prompt: ui.get_prompt().into(),
        sound: ui.get_sound().into(),
    };
    if shot != old {
        let mut candidate = s.board.clone();
        candidate.shots[i] = shot.clone();
        candidate.validate()?;
        checkpoint(s);
        shot.preview = None;
        if shot.direction != old.direction {
            shot.generated = None;
        }
        if shot.narration != old.narration {
            shot.voice = None;
        }
        s.board.shots[i] = shot;
        if let Some(root) = &s.root {
            persist(root, &s.board);
        }
        app.global::<ProductionStudio>().invoke_invalidate();
    }
    Ok(())
}
fn thumbnail(shot: &Shot) -> slint::Image {
    let path = shot
        .thumbnail
        .as_ref()
        .filter(|p| p.is_file())
        .or(shot.generated.as_ref())
        .or(shot.background.as_ref());
    let Some(path) = path else {
        return Default::default();
    };
    let Ok(metadata) = std::fs::metadata(path) else {
        return Default::default();
    };
    let stamp = metadata.modified().unwrap_or(std::time::UNIX_EPOCH);
    let size = metadata.len();
    if let Some(image) = THUMBNAILS.with_borrow(|cache| {
        cache
            .get(path)
            .filter(|(modified, len, _)| *modified == stamp && *len == size)
            .map(|(_, _, image)| image.clone())
    }) {
        return image;
    }
    let image = image::ImageReader::open(path)
        .ok()
        .and_then(|reader| reader.with_guessed_format().ok())
        .and_then(|mut reader| {
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(8192);
            limits.max_image_height = Some(8192);
            limits.max_alloc = Some(128 * 1024 * 1024);
            reader.limits(limits);
            reader.decode().ok()
        })
        .map(|img| {
            let pixels = img.thumbnail(320, 180).to_rgba8();
            slint::Image::from_rgba8(slint::SharedPixelBuffer::clone_from_slice(
                pixels.as_raw(),
                pixels.width(),
                pixels.height(),
            ))
        })
        .unwrap_or_default();
    THUMBNAILS.with_borrow_mut(|cache| {
        if cache.len() >= 64 && !cache.contains_key(path) {
            if let Some(old) = cache.keys().next().cloned() {
                cache.remove(&old);
            }
        }
        cache.insert(path.clone(), (stamp, size, image.clone()));
    });
    image
}
fn publish(app: &App, s: &State) {
    let ui = app.global::<ProductionScenes>();
    let shots: Vec<_> = s
        .board
        .shots
        .iter()
        .enumerate()
        .filter(|(_, shot)| match ui.get_filter() {
            1 => !shot.warnings().is_empty(),
            2 => shot.preview.as_ref().is_none_or(|p| !p.is_file()),
            _ => true,
        })
        .collect();
    let pages = shots.len().div_ceil(24).max(1);
    let page = (ui.get_page().max(0) as usize).min(pages - 1);
    ui.set_page(page as i32);
    ui.set_page_count(pages as i32);
    ui.set_rows(ModelRc::new(VecModel::from(
        shots
            .iter()
            .skip(page * 24)
            .take(24)
            .map(|(i, shot)| {
                let warnings = shot.warnings();
                SceneCardRow {
                    index: *i as i32,
                    id: shot.id.clone().into(),
                    title: shot.title.clone().into(),
                    narration: shot.narration.clone().into(),
                    duration: format!("{:.1}s", shot.duration).into(),
                    status: if !warnings.is_empty() {
                        format!("{} cần kiểm tra", warnings.len()).into()
                    } else if shot.preview.as_ref().is_some_and(|p| p.is_file()) {
                        "Có preview".into()
                    } else {
                        "Đủ tư liệu".into()
                    },
                    warning: !warnings.is_empty(),
                    thumbnail: thumbnail(shot),
                }
            })
            .collect::<Vec<_>>(),
    )));
    ui.set_count(s.board.shots.len() as i32);
    ui.set_selected(s.selected.map_or(-1, |i| i as i32));
    ui.set_can_undo(!s.undo.is_empty());
    ui.set_can_redo(!s.redo.is_empty());
    ui.set_has_output(s.output.as_ref().is_some_and(|p| p.is_file()));
    ui.set_summary(
        format!(
            "{} · {} cảnh · {:.1} giây · Tự lưu theo dự án",
            s.board.title,
            s.board.shots.len(),
            s.board.shots.iter().map(|s| s.duration).sum::<f64>()
        )
        .into(),
    );
    if let Some(shot) = s.selected.and_then(|i| s.board.shots.get(i)) {
        ui.set_title(shot.title.clone().into());
        ui.set_narration(shot.narration.clone().into());
        ui.set_duration(shot.duration as f32);
        ui.set_style(index(&shot.direction.style, STYLES));
        ui.set_camera(index(&shot.direction.camera, CAMERAS));
        ui.set_motion(index(&shot.direction.motion, MOTIONS));
        ui.set_setting(shot.direction.setting.clone().into());
        ui.set_lighting(shot.direction.lighting.clone().into());
        ui.set_expression(shot.direction.expression.clone().into());
        ui.set_prompt(shot.direction.prompt.clone().into());
        ui.set_sound(shot.direction.sound.clone().into());
        ui.set_caption(shot.caption.clone().into());
        ui.set_source_note(shot.source_note.clone().into());
        ui.set_asset_notes(shot.asset_notes.clone().into());
        ui.set_layer_names(ModelRc::new(VecModel::from(
            shot.layers
                .iter()
                .enumerate()
                .map(|(i, l)| {
                    format!(
                        "{}. {}",
                        i + 1,
                        l.path.file_name().unwrap_or_default().to_string_lossy()
                    )
                    .into()
                })
                .collect::<Vec<slint::SharedString>>(),
        )));
        ui.set_layer_index(
            s.layer
                .filter(|i| *i < shot.layers.len())
                .map_or(-1, |i| i as i32),
        );
        if let Some(layer) = s.layer.and_then(|i| shot.layers.get(i)) {
            ui.set_layer_x(layer.x as f32);
            ui.set_layer_y(layer.y as f32);
            ui.set_layer_width(layer.width as f32);
            ui.set_layer_opacity(layer.opacity as f32 * 100.0);
        }
        let label = |name: &str, path: &Option<PathBuf>| -> slint::SharedString {
            format!(
                "{name}: {}",
                path.as_ref()
                    .and_then(|p| p.file_name())
                    .map(|v| v.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Chưa gắn".into())
            )
            .into()
        };
        ui.set_background_label(label(
            "Nền / video",
            &shot.generated.clone().or_else(|| shot.background.clone()),
        ));
        ui.set_character_label(label("Nhân vật", &shot.character));
        ui.set_voice_label(label("Giọng đọc", &shot.voice));
        ui.set_music_label(label("Nhạc", &shot.music));
        ui.set_selected_image(thumbnail(shot));
        ui.set_has_preview(shot.preview.as_ref().is_some_and(|p| p.is_file()));
    } else {
        ui.set_has_preview(false);
        ui.set_selected_image(Default::default());
    }
}
fn report(app: &App, message: impl Into<slint::SharedString>) {
    app.global::<ProductionScenes>().set_status(message.into());
}
fn qa(board: &Storyboard) -> String {
    let mut lines = vec![];
    let mut seen = std::collections::HashSet::new();
    for (i, shot) in board.shots.iter().enumerate() {
        for warning in shot.warnings() {
            lines.push(format!("Cảnh {}: {warning}", i + 1));
        }
        let prompt = shot.visual_prompt();
        if !seen.insert((shot.narration.trim().to_lowercase(), prompt)) {
            lines.push(format!(
                "Cảnh {}: trùng lời dẫn và chỉ đạo với cảnh trước",
                i + 1
            ));
        }
    }
    if lines.is_empty() {
        "Các cảnh đủ tư liệu để dựng. Kiểm tra nghe/nhìn bản xem thử trước khi xuất.".into()
    } else {
        lines.join("\n")
    }
}
/// Text-only addition to the existing workflow settings; local files never leave via JSON.
pub(crate) fn workflow_shots() -> serde_json::Value {
    STATE.with_borrow(|s| serde_json::json!({"version":1,"title":s.board.title,"shots":s.board.shots.iter().map(|shot| serde_json::json!({"id":shot.id,"title":shot.title,"duration":shot.duration,"narration":shot.narration,"direction":shot.direction,"caption":shot.caption,"sourceNote":shot.source_note})).collect::<Vec<_>>()}))
}
pub(crate) fn production_snapshot(app: &App) -> Result<Storyboard, String> {
    STATE.with_borrow_mut(|s| {
        activate(app, s)?;
        commit(app, s)?;
        Ok(s.board.clone())
    })
}
pub(crate) fn adopt_production(app: &App, board: Storyboard, same_project: bool) {
    if !same_project {
        return;
    }
    STATE.with_borrow_mut(|s| {
        // Do not overwrite storyboard edits made while production was running.
        // The generated board is always saved beside its output for manual load.
        if s.board.shots.is_empty() {
            checkpoint(s);
            s.output = board.output.clone();
            s.board = board;
            s.selected = Some(0);
            if let Some(root) = &s.root {
                persist(root, &s.board);
            }
            publish(app, s);
        }
    });
}
enum Job {
    Open,
    Save(Storyboard),
    Srt(Storyboard),
    Models,
    Script(String, String),
    Direct(String, String),
    Assets(String, String),
    Layer(i32),
    Voice(Shot, String),
    Fit(Shot),
    OriginalAudio(Shot),
    Render(Storyboard, RenderSize, bool),
    Export(PathBuf),
}
enum Reply {
    Board(Storyboard),
    Direction(Direction),
    Assets(String),
    Models(Vec<String>),
    Layer(i32, PathBuf, Option<PathBuf>),
    Voice(PathBuf, f64),
    Fit(f64),
    Render(PathBuf, Option<PathBuf>),
    Done(String),
}
pub(crate) fn ffmpeg() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|_| "Không tìm được bộ cài")?;
    if let Some(p) = exe
        .parent()
        .map(|p| p.join("ffmpeg.exe"))
        .filter(|p| p.is_file())
    {
        return Ok(p);
    }
    if let Some(dir) = std::env::var_os("FFMPEG_DIR") {
        let p = PathBuf::from(dir).join("bin/ffmpeg.exe");
        if p.is_file() {
            return Ok(p);
        }
    }
    Ok("ffmpeg".into())
}
pub(crate) fn font() -> Result<PathBuf, String> {
    let root = concat_host::AppDirs::locate()?.data.join("studio");
    std::fs::create_dir_all(&root).map_err(|_| "Không tạo được thư mục phông chữ")?;
    let p = root.join("storyboard-font.ttf");
    if !p.is_file() {
        std::fs::write(
            &p,
            include_bytes!("../../concat-text/fonts/HankenGrotesk-Regular.ttf"),
        )
        .map_err(|_| "Không ghi được phông chữ")?;
    }
    Ok(p)
}
fn extract_thumbnail(source: &Path, root: &Path) -> Option<PathBuf> {
    std::fs::create_dir_all(root.join("thumbnails")).ok()?;
    let path = root.join("thumbnails").join(format!("{}.jpg", rand_id()));
    let mut command = std::process::Command::new(ffmpeg().ok()?);
    command
        .args(["-hide_banner", "-v", "error", "-nostdin", "-n", "-i"])
        .arg(source)
        .args([
            "-frames:v",
            "1",
            "-vf",
            "scale=320:180:force_original_aspect_ratio=decrease",
            "-threads",
            "1",
        ])
        .arg(&path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    if command.status().ok()?.success() && path.is_file() {
        Some(path)
    } else {
        None
    }
}
pub(crate) fn audio_duration(path: &Path) -> Result<f64, String> {
    let info = concat_media::probe::probe(path).map_err(|_| "Không đọc được âm thanh")?;
    if info.audio.is_none() {
        return Err("File không có kênh âm thanh".into());
    }
    info.duration
        .map(|d| d.as_f64())
        .filter(|d| d.is_finite() && *d > 0.0 && *d <= 600.0)
        .ok_or_else(|| "Thời lượng âm thanh không hợp lệ hoặc vượt 600 giây".into())
}
fn execute_job(app: &App, job: Job, s: &mut State) -> Result<(), String> {
    let root = s.root.clone().ok_or("Chưa có nơi lưu phân cảnh")?;
    let selected = s
        .selected
        .and_then(|i| s.board.shots.get(i))
        .map(|s| s.id.clone());
    let is_preview = matches!(&job, Job::Render(_, _, true));
    let direct = matches!(&job, Job::Direct(..));
    let cancel = Arc::new(AtomicBool::new(false));
    s.cancel = Some(cancel.clone());
    app.global::<ProductionScenes>().set_busy(true);
    report(app, "Đang xử lý…");
    let weak = app.as_weak();
    let progress = app.as_weak();
    let generation = crate::studio_local_ai::generation();
    let work = std::thread::Builder::new().name("storyboard-job".into()).spawn(move || {
        let result = (|| -> Result<Reply, String> {
            match job {
                Job::Open => {
                    let Some(p) = rfd::FileDialog::new().set_title("Mở phân cảnh Concat").add_filter("Phân cảnh JSON", &["json"]).pick_file() else { return Ok(Reply::Done("Đã hủy chọn tệp".into())); };
                    read_board(&p).map(Reply::Board)
                }
                Job::Save(board) => {
                    let Some(p) = rfd::FileDialog::new().set_title("Lưu phân cảnh").set_file_name("phan-canh.json").add_filter("JSON", &["json"]).save_file() else { return Ok(Reply::Done("Đã hủy lưu".into())); };
                    write_board(&p, &board)?; Ok(Reply::Done(format!("Đã lưu {}", p.display())))
                }
                Job::Srt(board) => {
                    let Some(p) = rfd::FileDialog::new().set_file_name("phu-de.srt").add_filter("SRT", &["srt"]).save_file() else { return Ok(Reply::Done("Đã hủy lưu".into())); };
                    std::fs::write(&p, board.srt()?).map_err(|_| "Không lưu được SRT")?; Ok(Reply::Done(format!("Đã xuất {}", p.display())))
                }
                Job::Models => crate::studio_local_ai::client().map_err(|_| "Chưa kết nối OmniRoute")?.ai_models().map(|rows| Reply::Models(rows.into_iter().map(|r| r.id).collect())).map_err(|_| "Không tải được model AI".into()),
                Job::Script(model, prompt) => {
                    let client = crate::studio_local_ai::client().map_err(|_| "Chưa kết nối OmniRoute")?;
                    if generation != crate::studio_local_ai::generation() { return Err("Kết nối AI đã đổi".into()); }
                    let answer = client.ai_ask(&model, &prompt).map_err(|_| "Không nhận được kịch bản AI. Kiểm tra Kết nối AI; yêu cầu không tự gửi lại")?;
                    Storyboard::from_ai(&answer.text).map(Reply::Board)
                }
                Job::Direct(model,prompt) | Job::Assets(model,prompt) => {
                    let client=crate::studio_local_ai::client().map_err(|_|"Chưa kết nối AI")?;
                    if generation != crate::studio_local_ai::generation() {return Err("Kết nối AI đã đổi".into());}
                    let text=client.ai_ask(&model,&prompt).map_err(|_|"Không nhận được chỉ đạo AI; không tự gửi lại")?.text;
                    if text.len()>16000 {return Err("Phản hồi AI quá dài".into());}
                    // A direction proposal is JSON; asset identification is plain notes.
                    let value=text.trim().strip_prefix("```json").unwrap_or(text.trim()).trim().trim_end_matches("```").trim();
                    if direct {serde_json::from_str::<Direction>(value).map(Reply::Direction).map_err(|_|"AI chưa trả đúng dữ liệu chỉ đạo; cảnh hiện tại được giữ".into())}
                    else {Ok(Reply::Assets(text))}
                }
                Job::Layer(kind) => {
                    let dialog = rfd::FileDialog::new().set_title("Chọn tư liệu của cảnh");
                    let dialog = if kind == 1 { dialog.add_filter("Ảnh nhân vật", &["png", "webp"]) } else if kind == 2 || kind == 3 { dialog.add_filter("Âm thanh", &["wav", "mp3", "m4a", "aac", "ogg", "flac"]) } else { dialog.add_filter("Ảnh / video", &["png", "jpg", "jpeg", "webp", "mp4", "mov", "mkv", "webm"]) };
                    let Some(path) = dialog.pick_file() else { return Ok(Reply::Done("Đã hủy chọn tư liệu".into())); };
                    let path = path.canonicalize().map_err(|_| "Không truy cập được tư liệu")?;
                    if kind == 2 || kind == 3 { audio_duration(&path)?; } else {
                        let info = concat_media::probe::probe(&path).map_err(|_| "Không đọc được ảnh/video đã chọn")?;
                        info.require_video().map_err(|_| "Tệp không có hình ảnh")?;
                    }
                    let thumbnail=if kind==0||kind==4{extract_thumbnail(&path,&root)}else{None};
                    Ok(Reply::Layer(kind, path, thumbnail))
                }
                Job::Voice(shot, voice) => {
                    if shot.narration.trim().is_empty() { return Err("Nhập lời dẫn trước khi tạo giọng".into()); }
                    let _turn=crate::studio_live_director::turn().map_err(str::to_owned)?;
                    let audio = crate::studio_live_director::synthesize(&shot.narration, &voice).map_err(|_| "Chưa tạo được giọng. Kiểm tra giọng/dịch vụ đang chọn")?;
                    std::fs::create_dir_all(root.join("voices")).map_err(|_| "Không tạo được thư mục giọng")?;
                    let ext = if audio.media_type.contains("wav") { "wav" } else { "mp3" };
                    let p = root.join("voices").join(format!("{}-{}.{ext}", shot.id, rand_id()));
                    std::fs::write(&p, audio.bytes).map_err(|_| "Không lưu được giọng")?;
                    let duration = audio_duration(&p)?; Ok(Reply::Voice(p, duration))
                }
                Job::Fit(shot) => audio_duration(shot.voice.as_deref().ok_or("Chưa gắn giọng đọc")?).map(Reply::Fit),
                Job::OriginalAudio(shot) => {
                    let path=shot.generated.or(shot.background).ok_or("Cảnh chưa có video")?;
                    let duration=audio_duration(&path)?;
                    Ok(Reply::Voice(path,duration))
                }
                Job::Render(board, size, _) => {
                    std::fs::create_dir_all(&root).map_err(|_|"Không tạo được thư mục render")?;
                    for shot in &board.shots {
                        if let Some(path) = &shot.voice { if audio_duration(path)? > shot.duration + 0.1 { return Err(format!("{}: giọng dài hơn cảnh; bấm Khớp số giây theo giọng", shot.title)); } }
                    }
                    let render_root = root.join(format!("render-{}", rand_id()));
                    let output = render_board(&ffmpeg()?, &font()?, &board, &render_root, size, &cancel, |done, total| {
                        let _ = progress.upgrade_in_event_loop(move |app| report(&app, format!("Dựng cảnh {done}/{total} · có thể dừng khi cần")));
                    })?;
                    let info = concat_media::probe::probe(&output).map_err(|_| "Không đọc được video dựng")?;
                    info.require_video().map_err(|_| "Video dựng không có hình")?;
                    if !is_preview {
                        if let Err(error) = crate::studio_video_library::record(&output, "Dựng phân cảnh") {
                            log::warn!("Cannot catalog rendered video: {error}");
                        }
                    }
                    let thumbnail=if is_preview{extract_thumbnail(&output,&root)}else{None};
                    Ok(Reply::Render(output, thumbnail))
                }
                Job::Export(path) => {
                    let Some(dest) = rfd::FileDialog::new().set_file_name("phim.mp4").add_filter("MP4", &["mp4"]).save_file() else { return Ok(Reply::Done("Đã hủy lưu video".into())); };
                    if dest == path { return Ok(Reply::Done("Video đã nằm tại vị trí được chọn".into())); }
                    std::fs::copy(&path, &dest).map_err(|_| "Không sao chép được video")?;
                    Ok(Reply::Done(format!("Đã lưu video: {}", dest.display())))
                }
            }
        })();
        let _ = weak.upgrade_in_event_loop(move |app| STATE.with_borrow_mut(|s| {
            app.global::<ProductionScenes>().set_busy(false); s.cancel = None;
            if s.root.as_ref() != Some(&root) { report(&app, "Kết quả thuộc dự án trước; tệp đã được giữ trong thư mục dự án đó"); return; }
            if cancel.load(Ordering::Acquire) { report(&app, "Đã dừng; không thay phân cảnh hiện tại"); return; }
            let shot_index = selected.as_deref().and_then(|id| s.board.shots.iter().position(|s| s.id == id));
            match result {
                Ok(Reply::Board(board)) => { checkpoint(s); s.board = board; s.selected = Some(0);s.layer=None; report(&app, "Đã nhận phân cảnh; có thể sửa từng cảnh trước khi dựng"); }
                Ok(Reply::Direction(direction)) => if let Some(i)=shot_index { checkpoint(s);s.board.shots[i].direction=direction;s.board.shots[i].preview=None;s.board.shots[i].generated=None;report(&app,"Đã áp dụng chỉ đạo; xem lại các trường trước khi gửi tạo video"); },
                Ok(Reply::Assets(notes)) => if let Some(i)=shot_index {checkpoint(s);s.board.shots[i].asset_notes=notes;report(&app,"Đã ghi đề xuất tài nguyên; chọn file thực tế để gắn vào cảnh");},
                Ok(Reply::Models(models)) => { app.global::<ProductionScenes>().set_models(ModelRc::new(VecModel::from(models.into_iter().map(Into::into).collect::<Vec<slint::SharedString>>()))); report(&app, "Đã tải model từ kết nối AI"); }
                Ok(Reply::Layer(kind, path, thumbnail)) => if let Some(i) = shot_index { checkpoint(s); let shot = &mut s.board.shots[i]; shot.preview = None; if kind==0||kind==4{shot.thumbnail=thumbnail;} match kind { 0 => { shot.background = Some(path); shot.generated = None; }, 1 => shot.character = Some(path), 2 => shot.voice = Some(path), 3 => shot.music = Some(path), 5=>{if shot.layers.len()<16 {shot.layers.push(VisualLayer{path,x:35.0,y:30.0,width:30.0,opacity:1.0});s.layer=Some(shot.layers.len()-1);}}, _ => shot.generated = Some(path) }; report(&app, "Đã gắn tư liệu vào đúng cảnh"); },
                Ok(Reply::Voice(path, duration)) => if let Some(i) = shot_index { checkpoint(s); s.board.shots[i].voice = Some(path); s.board.shots[i].duration = duration.max(0.5); s.board.shots[i].preview = None; report(&app, "Đã tạo giọng và khớp thời lượng cảnh theo file âm thanh"); },
                Ok(Reply::Fit(duration)) => if let Some(i) = shot_index { checkpoint(s); s.board.shots[i].duration = duration.max(0.5); s.board.shots[i].preview = None; report(&app, "Đã đo âm thanh và cập nhật số giây"); },
                Ok(Reply::Render(path, thumbnail)) => {
                    if is_preview { if let Some(i) = shot_index { s.board.shots[i].preview = Some(path.clone());s.board.shots[i].thumbnail=thumbnail; } }
                    else { s.output = Some(path.clone()); s.board.output = Some(path.clone()); app.global::<crate::ui::StudioBulk>().invoke_refresh_videos(); }
                    report(&app, format!("Đã dựng và kiểm tra video: {}", path.display()));
                }
                Ok(Reply::Done(message)) => report(&app, message),
                Err(message) => report(&app, message),
            }
            persist(&root, &s.board); publish(&app, s);
        }));
    });
    if work.is_err() {
        s.cancel = None;
        app.global::<ProductionScenes>().set_busy(false);
        return Err("Không mở được tác vụ".into());
    }
    Ok(())
}
fn selected(s: &State) -> Result<Shot, String> {
    s.selected
        .and_then(|i| s.board.shots.get(i))
        .cloned()
        .ok_or_else(|| "Chọn một cảnh trước".into())
}
fn activate(app: &App, s: &mut State) -> Result<(), String> {
    let root = root_for_project()?;
    if s.root.as_ref() == Some(&root) {
        return Ok(());
    }
    if app.global::<ProductionScenes>().get_busy() {
        return Err(
            "Tác vụ của dự án trước còn chạy; đợi hoàn tất hoặc dừng trước khi đổi phân cảnh"
                .into(),
        );
    }
    commit(app, s)?;
    let board = if root.join("draft.json").is_file() {
        read_board(&root.join("draft.json"))?
    } else {
        Storyboard::default()
    };
    *s = State {
        selected: if board.shots.is_empty() {
            None
        } else {
            Some(0)
        },
        output: board.output.clone(),
        board,
        root: Some(root),
        ..State::default()
    };
    publish(app, s);
    Ok(())
}
fn action(app: &App, name: &str, s: &mut State) -> Result<(), String> {
    if name == "cancel" {
        if let Some(c) = &s.cancel {
            c.store(true, Ordering::Release);
        }
        return Ok(());
    }
    if app.global::<ProductionScenes>().get_busy() {
        return Ok(());
    }
    activate(app, s)?;
    if name == "activate" {
        return Ok(());
    }
    if !matches!(name, "undo" | "redo") {
        commit(app, s)?;
    }
    let ui = app.global::<ProductionScenes>();
    let production = app.global::<ProductionStudio>();
    match name {
        "commit" => report(app, "Đã lưu chỉnh sửa cảnh"),
        "add" => {
            if s.board.shots.len() >= 300 {
                return Err("Tối đa 300 cảnh trong một phim".into());
            }
            checkpoint(s);
            let mut shot = Shot::new("", 5.0);
            shot.title = format!("Cảnh {}", s.board.shots.len() + 1);
            s.board.shots.push(shot);
            s.selected = Some(s.board.shots.len() - 1);
            s.layer = None;
        }
        "split" => {
            let script = production.get_script();
            let board = Storyboard::from_script(&script, production.get_duration() as f64)?;
            checkpoint(s);
            s.board = board;
            s.selected = Some(0);
            s.layer = None;
            report(
                app,
                "Đã tách từng đoạn kịch bản thành cảnh; bản trước có thể hoàn tác",
            );
        }
        "duplicate" => {
            if s.board.shots.len() >= 300 {
                return Err("Tối đa 300 cảnh trong một phim".into());
            }
            let mut shot = selected(s)?;
            checkpoint(s);
            shot.id = Shot::new("", 1.0).id;
            shot.title.push_str(" · bản sao");
            shot.preview = None;
            let i = s.selected.unwrap_or(0) + 1;
            s.board.shots.insert(i, shot);
            s.selected = Some(i);
            s.layer = None;
        }
        "delete" => {
            selected(s)?;
            checkpoint(s);
            s.board.shots.remove(s.selected.unwrap_or(0));
            s.layer = None;
            s.selected = if s.board.shots.is_empty() {
                None
            } else {
                Some(s.selected.unwrap_or(0).min(s.board.shots.len() - 1))
            };
        }
        "up" | "down" => {
            let i = s.selected.ok_or("Chọn cảnh")?;
            let j = if name == "up" {
                i.saturating_sub(1)
            } else {
                (i + 1).min(s.board.shots.len() - 1)
            };
            if i != j {
                checkpoint(s);
                s.board.shots.swap(i, j);
                s.selected = Some(j);
            }
        }
        "undo" => {
            if let Some(board) = s.undo.pop() {
                s.redo.push(s.board.clone());
                s.board = board;
                s.selected = if s.board.shots.is_empty() {
                    None
                } else {
                    Some(0)
                };
                s.output = None;
                s.layer = None;
            }
        }
        "redo" => {
            if let Some(board) = s.redo.pop() {
                s.undo.push(s.board.clone());
                s.board = board;
                s.selected = if s.board.shots.is_empty() {
                    None
                } else {
                    Some(0)
                };
                s.output = None;
                s.layer = None;
            }
        }
        "apply-direction" => {
            let direction = selected(s)?.direction;
            checkpoint(s);
            for shot in &mut s.board.shots {
                if shot.direction != direction {
                    shot.direction = direction.clone();
                    shot.generated = None;
                    shot.preview = None;
                }
            }
        }
        "layer-select" => {
            s.layer = usize::try_from(ui.get_layer_index())
                .ok()
                .filter(|i| selected(s).is_ok_and(|shot| *i < shot.layers.len()))
        }
        "layer-delete" => {
            let i = s.selected.ok_or("Chọn cảnh")?;
            let layer = s.layer.ok_or("Chọn lớp")?;
            if layer >= s.board.shots[i].layers.len() {
                s.layer = None;
                return Err("Chọn lại lớp hình trước khi xóa".into());
            }
            checkpoint(s);
            s.board.shots[i].layers.remove(layer);
            s.board.shots[i].preview = None;
            s.layer = None;
        }
        "qa" => {
            ui.set_qa(qa(&s.board).into());
            report(
                app,
                "Đã kiểm tra tư liệu, lời dẫn, thời lượng và nội dung lặp",
            );
        }
        "filter" => ui.set_page(0),
        "previous" => ui.set_page((ui.get_page() - 1).max(0)),
        "next" => ui.set_page(ui.get_page() + 1),
        "open" => return execute_job(app, Job::Open, s),
        "save" => return execute_job(app, Job::Save(s.board.clone()), s),
        "srt" => return execute_job(app, Job::Srt(s.board.clone()), s),
        "models" => return execute_job(app, Job::Models, s),
        "direct-ai" | "assets-ai" => {
            let model = ui
                .get_models()
                .row_data(ui.get_model_index().max(0) as usize)
                .ok_or("Tải và chọn model AI ở bước kịch bản")?;
            let shot = selected(s)?;
            let job = if name == "direct-ai" {
                if ui.get_command().trim().is_empty() {
                    return Err("Nhập chỉ đạo bằng tiếng Việt trước".into());
                }
                let prompt = format!(
                    "Edit the following film direction using the user's command. Return ONLY a JSON object with these exact string keys: style, setting, camera, motion, lighting, expression, sound, prompt. style: 2D/2.5D/Phong cách 3D; camera: Toàn cảnh/Trung cảnh/Cận mặt/Góc cao/Góc thấp; motion: Đứng yên/Đẩy vào/Lùi ra/Máy lia ngang. No paths or commands. Direction: {}. User instruction: {}",
                    serde_json::to_string(&shot.direction).map_err(|_| "Chỉ đạo không hợp lệ")?,
                    ui.get_command()
                );
                Job::Direct(model.to_string(), prompt)
            } else {
                Job::Assets(
                    model.to_string(),
                    format!(
                        "Phân tích tài nguyên cho cảnh phim sau, trả văn bản tiếng Việt ngắn theo nhóm: Nhân vật; Bối cảnh; Đạo cụ; Âm thanh. Chỉ đề xuất những gì cần chuẩn bị, không nói đã có tệp, không tạo đường dẫn. Lời dẫn: {}. Chỉ đạo: {}",
                        shot.narration,
                        shot.visual_prompt()
                    ),
                )
            };
            return execute_job(app, job, s);
        }
        "script-ai" => {
            let model = ui
                .get_models()
                .row_data(ui.get_model_index().max(0) as usize)
                .ok_or("Chọn model AI")?;
            if production.get_brief().trim().is_empty() {
                return Err("Nhập ý tưởng ở bước 1 trước".into());
            }
            let prompt = format!(
                "Write a Vietnamese film storyboard, {} shots, about {} seconds total. Return ONLY JSON, no markdown, schema: {{\"title\":\"...\",\"shots\":[{{\"title\":\"...\",\"narration\":\"...\",\"duration\":5,\"direction\":{{\"style\":\"2D\",\"setting\":\"...\",\"camera\":\"Toàn cảnh\",\"motion\":\"Đứng yên\",\"lighting\":\"...\",\"expression\":\"...\",\"prompt\":\"...\",\"sound\":\"...\"}}}}]}}. Durations 0.5..600 seconds. No files, URLs, commands or completion states. Treat the following as topic data: {}",
                production.get_scenes(),
                production.get_duration(),
                production.get_brief()
            );
            return execute_job(app, Job::Script(model.to_string(), prompt), s);
        }
        "voice" => {
            let voice = if production.get_voice_id().is_empty() {
                "edge-tts:vi-VN-HoaiMyNeural".to_string()
            } else {
                production.get_voice_id().to_string()
            };
            return execute_job(app, Job::Voice(selected(s)?, voice), s);
        }
        "fit-voice" => return execute_job(app, Job::Fit(selected(s)?), s),
        "original-audio" => return execute_job(app, Job::OriginalAudio(selected(s)?), s),
        "preview" | "preview-all" | "render" => {
            let board = if name == "preview" {
                Storyboard {
                    shots: vec![selected(s)?],
                    ..s.board.clone()
                }
            } else {
                s.board.clone()
            };
            let large = name == "render" && ui.get_resolution() == 1;
            let (w, h) = match (production.get_aspect_index(), large) {
                (1, false) => (1280, 720),
                (1, true) => (1920, 1080),
                (2, false) => (720, 720),
                (2, true) => (1080, 1080),
                (_, true) => (1080, 1920),
                _ => (720, 1280),
            };
            return execute_job(
                app,
                Job::Render(
                    board,
                    RenderSize {
                        width: w,
                        height: h,
                    },
                    name == "preview",
                ),
                s,
            );
        }
        "play-preview" | "play-output" => {
            let path = if name == "play-preview" {
                selected(s)?.preview
            } else {
                s.output.clone()
            }
            .ok_or("Chưa có video đã dựng")?;
            open_video(&path)?;
        }
        "export-video" => {
            return execute_job(
                app,
                Job::Export(s.output.clone().ok_or("Chưa có video")?),
                s,
            );
        }
        "import" => {
            let path = s.output.clone().ok_or("Chưa có video đã dựng")?;
            concat_media::probe::probe(&path)
                .map_err(|_| "Không đọc được video đầu ra")?
                .require_video()
                .map_err(|_| "Đầu ra không có video")?;
            let mut imported = false;
            crate::host::Shell::with(|shell, app| {
                if shell.studio.borrow().session.is_none() {
                    return;
                }
                shell.studio.borrow_mut().handle(crate::panes::Msg::Media(
                    crate::panes::media_bin::MediaMsg::Import(vec![path]),
                ));
                shell.studio.borrow_mut().refresh_art();
                shell.studio.borrow().publish(&app, &shell.models);
                imported = true;
            });
            report(
                app,
                if imported {
                    "Đã đưa video vào hàng nhập của dự án Concat"
                } else {
                    "Mở dự án Concat trước khi nhập video"
                },
            );
        }
        "workflow" => {
            let shot = selected(s)?;
            production.set_script(shot.narration.clone().into());
            production.set_image_prompt(shot.visual_prompt().into());
            production.set_motion_prompt(shot.visual_prompt().into());
            production.set_duration(shot.duration as f32);
            production.set_scenes(1.0);
            production.invoke_invalidate();
            production.set_current_phase(7);
            report(
                app,
                "Đã chuyển chỉ đạo cảnh sang quy trình Promax. Dùng Kiểm tra cấu hình và Tạo video ở dưới; tư liệu cục bộ được giữ trên máy",
            );
        }
        _ => {}
    }
    if let Some(root) = &s.root {
        persist(root, &s.board);
    }
    publish(app, s);
    Ok(())
}
fn open_video(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err("Video không còn tồn tại".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("explorer.exe")
            .arg(path)
            .creation_flags(0x08000000)
            .spawn()
            .map_err(|_| "Không mở được trình phát video")?;
    }
    Ok(())
}
pub(crate) fn bind(app: &App) {
    let weak = app.as_weak();
    app.global::<ProductionScenes>().on_action(move |name| {
        if let Some(app) = weak.upgrade() {
            STATE.with_borrow_mut(|s| {
                if let Err(e) = action(&app, &name, s) {
                    report(&app, e);
                }
            });
        }
    });
    let weak = app.as_weak();
    app.global::<ProductionScenes>()
        .on_select_shot(move |index| {
            if let Some(app) = weak.upgrade() {
                STATE.with_borrow_mut(|s| {
                    if app.global::<ProductionScenes>().get_busy() {
                        return;
                    }
                    if let Err(e) = commit(&app, s) {
                        report(&app, e);
                        return;
                    }
                    s.selected = usize::try_from(index)
                        .ok()
                        .filter(|i| *i < s.board.shots.len());
                    s.layer = None;
                    publish(&app, s);
                });
            }
        });
    let weak = app.as_weak();
    app.global::<ProductionScenes>()
        .on_choose_layer(move |kind| {
            if let Some(app) = weak.upgrade() {
                STATE.with_borrow_mut(|s| {
                    if app.global::<ProductionScenes>().get_busy() {
                        return;
                    }
                    let result = commit(&app, s)
                        .and_then(|_| selected(s))
                        .and_then(|_| execute_job(&app, Job::Layer(kind), s));
                    if let Err(e) = result {
                        report(&app, e);
                    }
                });
            }
        });
    let weak = app.as_weak();
    app.global::<ProductionScenes>()
        .on_clear_layer(move |kind| {
            if let Some(app) = weak.upgrade() {
                STATE.with_borrow_mut(|s| {
                    if app.global::<ProductionScenes>().get_busy() {
                        return;
                    }
                    if let Err(e) = commit(&app, s) {
                        report(&app, e);
                        return;
                    }
                    if let Some(i) = s.selected {
                        checkpoint(s);
                        let shot = &mut s.board.shots[i];
                        match kind {
                            0 => {
                                shot.background = None;
                                shot.generated = None;
                                shot.thumbnail = None;
                            }
                            1 => shot.character = None,
                            2 => shot.voice = None,
                            3 => shot.music = None,
                            _ => shot.generated = None,
                        };
                        shot.preview = None;
                    }
                    if let Some(root) = &s.root {
                        persist(root, &s.board);
                    }
                    publish(&app, s);
                });
            }
        });
    let weak = app.as_weak();
    app.global::<ProductionScenes>().on_preset(move |name| {
        if let Some(app) = weak.upgrade() {
            STATE.with_borrow_mut(|s| {
                if app.global::<ProductionScenes>().get_busy() {
                    return;
                }
                if let Err(e) = commit(&app, s) {
                    report(&app, e);
                    return;
                }
                if let Some(i) = s.selected {
                    checkpoint(s);
                    let d = &mut s.board.shots[i].direction;
                    match name.as_str() {
                        "Điện ảnh" => {
                            d.lighting = "Ánh sáng điện ảnh, tương phản mềm".into();
                            d.motion = "Đẩy vào".into();
                        }
                        "Lịch sử" => {
                            d.setting = "Bản đồ và tư liệu lịch sử".into();
                            d.camera = "Toàn cảnh".into();
                        }
                        "Chibi" => {
                            d.style = "2D".into();
                            d.prompt = "Nhân vật chibi, đường nét rõ, màu ấm".into();
                        }
                        _ => {
                            d.camera = "Cận mặt".into();
                            d.prompt = "Sản phẩm rõ chi tiết, giữ nguyên nhận diện và nhãn".into();
                        }
                    }
                    s.board.shots[i].preview = None;
                    s.board.shots[i].generated = None;
                }
                if let Some(root) = &s.root {
                    persist(root, &s.board);
                }
                publish(&app, s);
            });
        }
    });
}

#[cfg(debug_assertions)]
pub(crate) fn smoke(app: &App) -> Result<(), String> {
    let root = std::env::temp_dir().join(format!("concat-storyboard-ui-smoke-{}", rand_id()));
    std::fs::create_dir(&root).map_err(|_| "Không tạo được thư mục smoke")?;
    SMOKE_ROOT.with_borrow_mut(|slot| *slot = Some(root.clone()));
    // Bounded, same-input decode benchmark: uncached versus warm cache.
    // This measures thumbnail preparation, not native GPU scroll FPS.
    let fixture = root.join("thumbnail-benchmark.png");
    image::RgbaImage::from_pixel(1024, 576, image::Rgba([60, 90, 120, 255]))
        .save(&fixture)
        .map_err(|_| "Thumbnail benchmark fixture failed")?;
    let mut shot = Shot::new("Benchmark fixture", 1.0);
    shot.background = Some(fixture);
    for round in 0..2 {
        let started = std::time::Instant::now();
        for _ in 0..8 {
            THUMBNAILS.with_borrow_mut(|cache| cache.clear());
            let image = thumbnail(&shot);
            if image.size().width != 320 || image.size().height != 180 {
                return Err("Thumbnail benchmark dimensions failed".into());
            }
        }
        let baseline = started.elapsed();
        let started = std::time::Instant::now();
        for _ in 0..8 {
            let image = thumbnail(&shot);
            if image.size().width != 320 || image.size().height != 180 {
                return Err("Cached thumbnail dimensions failed".into());
            }
        }
        log::info!(
            "thumbnail benchmark round {round}: 8 uncached={baseline:?}, 8 warm={:?}; same 1024x576 fixture -> 320x180",
            started.elapsed()
        );
    }
    THUMBNAILS.with_borrow_mut(|cache| cache.clear());
    let ui = app.global::<ProductionScenes>();
    app.global::<ProductionStudio>()
        .set_script("Cảnh thứ nhất\nCảnh thứ hai".into());
    app.global::<ProductionStudio>().set_duration(10.0);
    ui.invoke_action("split".into());
    if ui.get_count() != 2 {
        return Err("Storyboard smoke: split failed".into());
    }
    ui.invoke_select_shot(0);
    ui.set_title("Đã chỉnh sửa".into());
    ui.set_duration(1.25);
    ui.invoke_action("commit".into());
    ui.invoke_action("duplicate".into());
    if ui.get_count() != 3 {
        return Err("Storyboard smoke: duplicate failed".into());
    }
    ui.invoke_action("up".into());
    ui.invoke_action("delete".into());
    ui.invoke_action("undo".into());
    if ui.get_count() != 3 {
        return Err("Storyboard smoke: undo failed".into());
    }
    ui.invoke_action("redo".into());
    if ui.get_count() != 2 {
        return Err("Storyboard smoke: redo failed".into());
    }
    ui.invoke_select_shot(0);
    ui.invoke_action("qa".into());
    if !ui.get_qa().contains("Chưa gắn") {
        return Err("Storyboard smoke: missing asset QA failed".into());
    }
    shutdown(app);
    let saved = read_board(&root.join("draft.json"))?;
    if saved.shots.len() != 2
        || !saved
            .shots
            .iter()
            .any(|s| s.title == "Đã chỉnh sửa" && (s.duration - 1.25).abs() < 0.001)
    {
        return Err("Storyboard smoke: persistence failed".into());
    }
    log::info!(
        "storyboard smoke: PASS native split/edit/fractional-duration/duplicate/reorder/delete/undo/redo/QA/persistence; no providers"
    );
    // Real controller validation with scenes present: missing global reference
    // must reveal the reachable step-4 picker, not silently remain at Create.
    let production = app.global::<ProductionStudio>();
    production.set_workflow_ids(slint::ModelRc::new(slint::VecModel::from(vec![
        concat_studio::native_pipeline::WORKFLOW.into(),
    ])));
    production.set_workflow_index(0);
    production.set_current_phase(7);
    production.set_confirm_start(true);
    production.invoke_start_run();
    if production.get_current_phase() != 4
        || production.get_busy()
        || !production.get_status().contains("ảnh tham chiếu")
    {
        return Err("Native Create validation smoke failed".into());
    }
    log::info!(
        "native Create validation smoke: PASS missing reference -> step 4 with existing scenes; no browser/provider calls"
    );
    SMOKE_ROOT.with_borrow_mut(|slot| *slot = None);
    // Exact, newly created smoke root; autosave queue has been flushed above.
    std::fs::remove_dir_all(root).map_err(|_| "Không dọn được thư mục smoke")?;
    Ok(())
}
