// SPDX-License-Identifier: AGPL-3.0-or-later
//! Explicit offline generation of reusable motion packs, not per-reply synthesis.
use crate::ui::{App, StudioLive, StudioOpenTalking};
use concat_studio::motion_generation::MotionClient;
use slint::{ComponentHandle, Model};
use std::{
    io::Read,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

#[derive(Default, Clone)]
struct Inputs {
    image: Vec<u8>,
    audio: Option<(Vec<u8>, String)>,
}
static INPUTS: OnceLock<Mutex<Inputs>> = OnceLock::new();
static CLIENT: OnceLock<Mutex<Option<(String, MotionClient)>>> = OnceLock::new();
fn bounded(path: &std::path::Path, max: usize) -> Result<Vec<u8>, &'static str> {
    let file = std::fs::File::open(path).map_err(|_| "Không mở được tệp")?;
    if !file
        .metadata()
        .is_ok_and(|m| m.is_file() && m.len() <= max as u64)
    {
        return Err("Tệp quá lớn");
    }
    let mut bytes = Vec::new();
    file.take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Không đọc được tệp")?;
    if bytes.len() > max {
        return Err("Tệp quá lớn");
    }
    Ok(bytes)
}
fn choose(app: &App, photo: bool) {
    let ui = app.global::<StudioOpenTalking>();
    if ui.get_gesture_busy() || app.global::<StudioLive>().get_director_busy() {
        return;
    }
    ui.set_gesture_busy(true);
    let weak = app.as_weak();
    let spawned = std::thread::Builder::new()
        .name("concat-motion-input".into())
        .spawn(move || {
            let result = (|| {
                let picker = rfd::FileDialog::new();
                let path = if photo {
                    picker
                        .add_filter("Ảnh nhân vật", &["png", "jpg", "jpeg"])
                        .pick_file()
                } else {
                    picker
                        .add_filter("Âm thanh", &["wav", "mp3", "flac", "m4a", "aac"])
                        .pick_file()
                };
                let Some(path) = path else { return Ok(None) };
                let label = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let bytes = bounded(
                    &path,
                    if photo {
                        8 * 1024 * 1024
                    } else {
                        5 * 1024 * 1024
                    },
                )?;
                let mut input = INPUTS
                    .get_or_init(|| Mutex::new(Inputs::default()))
                    .lock()
                    .map_err(|_| "Không mở được mẫu")?;
                let preview = if photo {
                    let image = crate::studio_opentalking::decode_image_bytes(&bytes)
                        .ok_or("Ảnh không hợp lệ; tối đa 4096×4096")?;
                    if image.width() < 128 || image.height() < 128 {
                        return Err("Chọn ảnh nhân vật ít nhất 128×128");
                    }
                    let image = image::DynamicImage::ImageRgba8(image)
                        .resize(1024, 1024, image::imageops::FilterType::Lanczos3)
                        .to_rgba8();
                    let mut buffer = std::io::Cursor::new(Vec::new());
                    image
                        .write_to(&mut buffer, image::ImageFormat::Png)
                        .map_err(|_| "Không chuẩn bị được ảnh")?;
                    if buffer.get_ref().len() > 4 * 1024 * 1024 {
                        return Err("Ảnh sau chuẩn hóa quá lớn; chọn ảnh đơn giản hơn");
                    }
                    input.image = buffer.into_inner();
                    Some(image)
                } else {
                    let ext = path
                        .extension()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_ascii_lowercase();
                    let kind = match ext.as_str() {
                        "wav" => "audio/wav",
                        "mp3" => "audio/mpeg",
                        "flac" => "audio/flac",
                        "m4a" => "audio/mp4",
                        "aac" => "audio/aac",
                        _ => return Err("Định dạng âm thanh không hỗ trợ"),
                    };
                    input.audio = Some((bytes, kind.into()));
                    None
                };
                Ok(Some((label, preview)))
            })();
            let _ = weak.upgrade_in_event_loop(move |app| {
                let ui = app.global::<StudioOpenTalking>();
                ui.set_gesture_busy(false);
                match result {
                    Ok(Some((label, preview))) => {
                        if let Some(image) = preview {
                            ui.set_gesture_image_name(label.into());
                            ui.set_gesture_image(slint::Image::from_rgba8(
                                slint::SharedPixelBuffer::clone_from_slice(
                                    image.as_raw(),
                                    image.width(),
                                    image.height(),
                                ),
                            ));
                        } else {
                            ui.set_gesture_audio_name(label.into());
                        }
                        ui.set_gesture_consent(false);
                        ui.set_gesture_status("Đã chọn mẫu; chỉ gửi tới dịch vụ AI khi bạn bấm Tạo.".into());
                    }
                    Ok(None) => {}
                    Err(e) => ui.set_gesture_status(e.into()),
                }
            });
        });
    if spawned.is_err() {
        ui.set_gesture_busy(false);
        ui.set_gesture_status("Chưa mở được tác vụ chọn mẫu; thử lại khi máy đủ bộ nhớ.".into());
    }
}
struct Outputs {
    files: Vec<PathBuf>,
    folder: PathBuf,
    committed: bool,
}
impl Drop for Outputs {
    fn drop(&mut self) {
        if !self.committed {
            // Only files created exclusively for this attempt; never source media.
            for file in &self.files {
                let _ = std::fs::remove_file(file);
            }
            let _ = std::fs::remove_dir(&self.folder);
        }
    }
}
fn generate(app: &App) {
    let ui = app.global::<StudioOpenTalking>();
    let live = app.global::<StudioLive>();
    if ui.get_gesture_busy() || live.get_director_busy() || live.get_is_speaking() {
        return;
    }
    if live.get_session_active() {
        ui.set_gesture_status(
            "Ngắt phiên TikTok trước khi tạo cử chỉ để không tranh GPU với LIVE.".into(),
        );
        return;
    }
    let Ok(seconds) = ui.get_gesture_seconds().trim().parse::<u32>() else {
        ui.set_gesture_status("Nhập số giây nguyên từ 2 đến 10.".into());
        return;
    };
    if !(2..=10).contains(&seconds) || !ui.get_gesture_consent() {
        ui.set_gesture_status("Chọn 2–10 giây và xác nhận quyền dùng ảnh/giọng.".into());
        return;
    }
    let (url, token) = (
        ui.get_gesture_url().to_string(),
        ui.get_gesture_token().to_string(),
    );
    let voice = ui
        .get_voices()
        .row_data(ui.get_voice_index().max(0) as usize)
        .map(|r| r.id.to_string())
        .unwrap_or_else(|| "vi-VN-HoaiMyNeural".into());
    let input = INPUTS
        .get_or_init(|| Mutex::new(Inputs::default()))
        .lock()
        .map(|v| v.clone())
        .unwrap_or_default();
    if input.image.is_empty() {
        ui.set_gesture_status("Chọn ảnh nhân vật trước.".into());
        return;
    }
    ui.set_gesture_busy(true);
    live.set_director_busy(true);
    ui.set_gesture_status("Đang chuẩn bị tạo cử chỉ; chưa gửi ảnh và giọng…".into());
    let weak = app.as_weak();
    let spawned = std::thread::Builder::new().name("concat-motion-generate".into()).spawn(move || {
        let result: Result<_, &'static str> = (|| {
            let cached = CLIENT
                .get_or_init(|| Mutex::new(None))
                .lock()
                .ok()
                .and_then(|c| {
                    c.as_ref()
                        .filter(|(origin, _)| origin == &url)
                        .map(|(_, c)| c.clone())
                });
            let client = if token.trim().is_empty() {
                cached.ok_or("Tính năng tạo cử chỉ đang được hệ thống chuẩn bị. Vui lòng thử lại sau giây lát.")?
            } else {
                MotionClient::new(&url, &token)
                    .map_err(|_| "Dịch vụ AI chưa sẵn sàng. Vui lòng kiểm tra lại kết nối mạng.")?
            };
            client.readiness().map_err(|_| "Dịch vụ tạo cử chỉ đang bận hoặc chưa sẵn sàng. Vui lòng thử lại sau.")?;
            if let Ok(mut slot) = CLIENT.get().unwrap().lock() {
                *slot = Some((url, client.clone()));
            }
            let _ = weak.upgrade_in_event_loop(|app| {
                app.global::<StudioOpenTalking>()
                    .set_gesture_token("".into())
            });
            let dirs =
                concat_host::AppDirs::locate().map_err(|_| "Không mở được thư mục Concat")?;
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| "Đồng hồ máy không hợp lệ")?
                .as_nanos();
            let folder = dirs
                .data
                .join("gesture-generated")
                .join(format!("{unique}-{}", std::process::id()));
            std::fs::create_dir_all(folder.parent().unwrap())
                .map_err(|_| "Không tạo được thư mục clip")?;
            std::fs::create_dir(&folder).map_err(|_| "Không tạo được bộ clip")?;
            let mut output = Outputs {
                files: Vec::new(),
                folder,
                committed: false,
            };
            let texts = [
                "Mình đang lắng nghe, bạn cần tư vấn gì nhé?",
                "Xin chào, rất vui được gặp bạn!",
                "Mình giới thiệu sản phẩm này, bạn cần biết thêm gì nhé?",
            ];
            for (role, text) in texts.iter().enumerate() {
                let _ = weak.upgrade_in_event_loop(move |app| {
                    app.global::<StudioOpenTalking>().set_gesture_status(
                        format!(
                            "Đang tạo clip {}/3 · {} giây. Có thể mất vài phút, không bấm tạo lại.",
                            role + 1,
                            seconds
                        )
                        .into(),
                    )
                });
                let audio = match &input.audio {
                    Some((bytes, kind)) => concat_host::studio_opentalking::TtsAudio {
                        bytes: bytes.clone(),
                        media_type: kind.clone(),
                    },
                    None => crate::studio_live_director::synthesize(text, &voice).map_err(
                        |_| "Không tạo được giọng dẫn; chọn giọng hợp lệ hoặc tải âm thanh lên.",
                    )?,
                };
                let video = client.render(&input.image, &audio.bytes, &audio.media_type, role, seconds, true)
                    .map_err(|_| "Tạo clip thất bại hoặc quá thời gian. Bộ cũ vẫn giữ nguyên; không tự gửi lại yêu cầu.")?;
                let path = output
                    .folder
                    .join(format!("{}.mp4", ["reply", "greeting", "product"][role]));
                output.files.push(path.clone());
                use std::io::Write;
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .map_err(|_| "Không lưu được clip")?;
                file.write_all(&video)
                    .and_then(|_| file.sync_all())
                    .map_err(|_| "Không lưu được clip")?;
                drop(file);
                let info =
                    concat_media::probe(&path).map_err(|_| "Server trả về video không đọc được")?;
                if info.audio.is_some()
                    || info
                        .duration
                        .is_none_or(|d| (d.as_f64() - seconds as f64).abs() > 0.08)
                {
                    return Err("Clip trả về sai thời lượng hoặc chưa tắt tiếng gốc");
                }
            }
            let files: [PathBuf; 3] = output
                .files
                .clone()
                .try_into()
                .map_err(|_| "Chưa đủ ba clip")?;
            let profile = crate::studio_live_director::install_generated(files)?;
            output.committed = true;
            Ok(profile)
        })();
        let _ = weak.upgrade_in_event_loop(move |app| {
            let ui = app.global::<StudioOpenTalking>(); ui.set_gesture_busy(false);
            app.global::<StudioLive>().set_director_busy(false);
            match result {
                Ok(profile) => {
                    crate::studio_live_director::publish_generated(&app, &profile);
                    if let Some(path) = profile.clips[0].clone() { crate::studio_live_video::select_generated_motion(&app, path); }
                    ui.set_gesture_status(format!("Đã lưu 3 clip, mỗi clip {seconds} giây và tự gán cho LIVE. Lời mới sẽ được khớp môi qua MuseTalk khi server sẵn sàng.").into());
                }
                Err(e) => ui.set_gesture_status(e.into()),
            }
        });
    });
    if spawned.is_err() {
        ui.set_gesture_busy(false);
        live.set_director_busy(false);
        ui.set_gesture_status(
            "Chưa mở được tác vụ tạo cử chỉ; chưa gửi yêu cầu lên server.".into(),
        );
    }
}
pub(crate) fn bind(app: &App) {
    let weak = app.as_weak();
    app.global::<StudioOpenTalking>()
        .on_choose_gesture_image(move || {
            if let Some(app) = weak.upgrade() {
                choose(&app, true);
            }
        });
    let weak = app.as_weak();
    app.global::<StudioOpenTalking>()
        .on_choose_gesture_audio(move || {
            if let Some(app) = weak.upgrade() {
                choose(&app, false);
            }
        });
    let weak = app.as_weak();
    app.global::<StudioOpenTalking>()
        .on_clear_gesture_audio(move || {
            if let Some(app) = weak.upgrade() {
                if app.global::<StudioOpenTalking>().get_gesture_busy() {
                    return;
                }
                if let Ok(mut input) = INPUTS.get_or_init(|| Mutex::new(Inputs::default())).lock() {
                    input.audio = None;
                }
                app.global::<StudioOpenTalking>()
                    .set_gesture_audio_name("".into());
                app.global::<StudioOpenTalking>().set_gesture_consent(false);
            }
        });
    let weak = app.as_weak();
    app.global::<StudioOpenTalking>()
        .on_generate_gestures(move || {
            if let Some(app) = weak.upgrade() {
                generate(&app);
            }
        });
}
