// SPDX-License-Identifier: AGPL-3.0-or-later
//! Wires existing SLive VieNeu endpoints, native motion player and MuseTalk.
//! No new synthesis/model engine; profiles persist paths only, credentials stay in RAM.
use crate::{
    host::Shell,
    ui::{App, StudioLive},
};
use concat_studio::{live_director::MotionProfile, live_voice::VoiceClient};
use slint::{ComponentHandle, Model};
use std::{
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

static MOTION: OnceLock<Mutex<MotionProfile>> = OnceLock::new();
static VOICE: OnceLock<Mutex<Option<(VoiceClient, String)>>> = OnceLock::new();
static MOTION_REV: AtomicU64 = AtomicU64::new(0);
static VOICE_REV: AtomicU64 = AtomicU64::new(0);
pub(crate) fn voice_revision()->u64 { VOICE_REV.load(Ordering::Acquire) }
static SAMPLE_MODE: AtomicBool = AtomicBool::new(false);
static TURN: Mutex<()> = Mutex::new(());

pub(crate) fn turn() -> Result<std::sync::MutexGuard<'static, ()>, &'static str> {
    TURN.try_lock()
        .map_err(|_| "Một câu LIVE vẫn đang xử lý; không xếp chồng tiếng hoặc GPU")
}
pub(crate) fn synthesize(
    text: &str,
    fallback: &str,
) -> Result<concat_host::studio_opentalking::TtsAudio, slint::SharedString> {
    let voice = if SAMPLE_MODE.load(Ordering::Acquire) {
        VOICE
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| slint::SharedString::from("Không truy cập được giọng mẫu"))?
            .clone()
    } else {
        None
    };
    let fallback_voice = if fallback.is_empty() {
        "vi-VN-HoaiMyNeural"
    } else {
        fallback
    };
    match voice {
        Some((_, voice_name)) if voice_name.is_empty() => {
            crate::studio_opentalking::synthesize_speech_robust(text, fallback_voice)
        }
        Some((client, voice_name)) => match client.synthesize(text, &voice_name) {
            Ok(audio) => Ok(audio),
            Err(_) => {
                if fallback == "unused-no-network" {
                    return Err("Giọng mẫu thất bại; không có mạng để fallback.".into());
                }
                crate::studio_opentalking::synthesize_speech_robust(text, fallback_voice)
            }
        },
        None => crate::studio_opentalking::synthesize_speech_robust(text, fallback_voice),
    }
}
pub(crate) fn source(text: &str, idle: &Path) -> (PathBuf, &'static str) {
    if let Some(path) = crate::studio_kedi_live::motion_source() {
        return (path, "Kedi scene manifest");
    }
    if let Ok(profile) = MOTION
        .get_or_init(|| Mutex::new(MotionProfile::default()))
        .lock()
    {
        let (path, role) = profile.source(text, idle);
        (
            path.to_owned(),
            ["trả lời", "chào", "giới thiệu", "clip chờ đang chọn"][role],
        )
    } else {
        (idle.to_owned(), "clip chờ đang chọn")
    }
}
fn profile_path() -> Result<PathBuf, &'static str> {
    let dirs = concat_host::AppDirs::locate().map_err(|_| "Không mở được dữ liệu Concat")?;
    let folder = dirs.data.join("live-director");
    std::fs::create_dir_all(&folder).map_err(|_| "Không tạo được thư mục điều phối")?;
    Ok(folder.join("motion-profile.json"))
}
fn save(profile: &MotionProfile) -> Result<(), &'static str> {
    use std::io::Write;
    let path = profile_path()?;
    let temporary = path.with_extension(format!("{}.partial", std::process::id()));
    let bytes = serde_json::to_vec(profile).map_err(|_| "Không lưu được cấu hình")?;
    let result = (|| {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)
            .map_err(|_| "Không lưu được cấu hình")?;
        f.write_all(&bytes)
            .and_then(|_| f.sync_all())
            .map_err(|_| "Không lưu được cấu hình")?;
        drop(f);
        std::fs::rename(&temporary, &path).map_err(|_| "Không thay được cấu hình điều phối")
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}
pub(crate) fn validate_clip(path: &Path) -> Result<(), &'static str> {
    let meta = std::fs::metadata(path).map_err(|_| "Clip không còn trên máy")?;
    if !meta.is_file()
        || meta.len() > 25 * 1024 * 1024
        || !path
            .extension()
            .and_then(|v| v.to_str())
            .is_some_and(|v| v.eq_ignore_ascii_case("mp4"))
    {
        return Err("Chọn MP4 tối đa 25 MB");
    }
    let probe = concat_media::probe(path).map_err(|_| "Không đọc được clip")?;
    if probe.video.is_none()
        || probe
            .duration
            .is_none_or(|d| !(1.0..=10.05).contains(&d.as_f64()))
    {
        return Err("Chọn clip 1–10 giây, rõ mặt và cùng một nhân vật");
    }
    Ok(())
}
fn publish(app: &App, profile: &MotionProfile) {
    let ui = app.global::<StudioLive>();
    ui.set_director_enabled(profile.enabled);
    ui.set_director_clips(slint::ModelRc::new(slint::VecModel::from(
        profile
            .clips
            .iter()
            .map(|p| {
                p.as_ref()
                    .map(|p| {
                        p.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string()
                    })
                    .unwrap_or_else(|| "Chưa gán clip".into())
                    .into()
            })
            .collect::<Vec<slint::SharedString>>(),
    )));
}
/// Commit the whole generated pack only after every clip has passed media checks.
pub(crate) fn install_generated(clips: [PathBuf; 3]) -> Result<MotionProfile, &'static str> {
    for path in &clips { validate_clip(path)?; }
    let profile = MotionProfile { enabled: true, clips: clips.map(Some) };
    let mut slot = MOTION.get_or_init(|| Mutex::new(MotionProfile::default())).lock()
        .map_err(|_| "Không cập nhật được bộ cử chỉ")?;
    save(&profile)?;
    *slot = profile.clone();
    Ok(profile)
}
pub(crate) fn publish_generated(app: &App, profile: &MotionProfile) { publish(app, profile); }
fn change(app: &App, role: i32, remove: bool) {
    let ui = app.global::<StudioLive>();
    if ui.get_director_busy() || ui.get_is_speaking() || !(0..=3).contains(&role) {
        return;
    }
    crate::studio_lipsync::cancel();
    let mut profile = MOTION
        .get_or_init(|| Mutex::new(MotionProfile::default()))
        .lock()
        .map(|p| p.clone())
        .unwrap_or_default();
    ui.set_director_busy(true);
    let rev = MOTION_REV.fetch_add(1, Ordering::AcqRel) + 1;
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result: Result<Option<MotionProfile>, &'static str> = (|| {
            if role == 3 {
                profile.enabled = !profile.enabled;
            } else if remove {
                profile.clips[role as usize] = None;
            } else {
                let Some(path) = rfd::FileDialog::new()
                    .set_title("Chọn clip cử chỉ cùng nhân vật · MP4 1–10 giây")
                    .add_filter("MP4", &["mp4"])
                    .pick_file()
                else {
                    return Ok(None);
                };
                validate_clip(&path)?;
                profile.clips[role as usize] = Some(path);
            }
            save(&profile)?;
            Ok(Some(profile))
        })();
        let _ = weak.upgrade_in_event_loop(move |app| {
            if MOTION_REV.load(Ordering::Acquire) != rev { return; }
            let ui = app.global::<StudioLive>(); ui.set_director_busy(false);
            match result {
                Ok(Some(profile)) => {
                    publish(&app, &profile);
                    if let Ok(mut p) = MOTION.get().unwrap().lock() { *p = profile; }
                    ui.set_director_status("Đã lưu clip cử chỉ. Khi bật GPU, clip được chọn làm nguồn khớp môi; tiếng gốc không phát.".into());
                }
                Ok(None) => ui.set_director_status("Đã hủy chọn clip; giữ nguyên cấu hình.".into()),
                Err(e) => ui.set_director_status(slint::SharedString::from(e)),
            }
        });
    });
}
fn extract_reference(path: &Path) -> Result<Vec<u8>, &'static str> {
    use concat_media::{AudioDecoder, AudioOptions, SampleFormat};
    let meta = std::fs::metadata(path).map_err(|_| "Không đọc được tệp mẫu")?;
    if !meta.is_file() || meta.len() > 100 * 1024 * 1024 {
        return Err("Mẫu tối đa 100 MB");
    }
    let mut decoder = AudioDecoder::open(
        path,
        &AudioOptions {
            duration: Some(8.0),
            rate: 16000,
            channels: 1,
            format: SampleFormat::I16,
            ..Default::default()
        },
    )
    .map_err(|_| "Video không có tiếng hoặc định dạng âm thanh chưa được hỗ trợ.")?;
    let mut samples = Vec::new();
    while let Some(chunk) = decoder
        .next_i16()
        .map_err(|_| "Không tách được tiếng từ video mẫu")?
    {
        samples.extend_from_slice(&chunk[..chunk.len().min(128000 - samples.len())]);
        if samples.len() == 128000 {
            break;
        }
    }
    if samples.is_empty() {
        return Err("Video không có dữ liệu âm thanh để lấy giọng.");
    }
    // Pad/loop short audio up to 3 seconds (48,000 samples) so reference_wav accepts it
    if samples.len() < 48000 {
        let original_len = samples.len();
        while samples.len() < 48000 {
            let to_copy = (48000 - samples.len()).min(original_len);
            let slice: Vec<i16> = samples[..to_copy].to_vec();
            samples.extend_from_slice(&slice);
        }
    }
    concat_studio::live_voice::reference_wav(&samples)
        .map_err(|_| "Cần ít nhất 3 giây tiếng sạch ở đầu mẫu")
}
fn enroll_voice(app: &App, from_avatar: bool) {
    let ui=app.global::<StudioLive>();
    if ui.get_is_speaking() || ui.get_voice_server_busy() { return; }
    let cached=VOICE.get_or_init(|| Mutex::new(None)).lock().ok().and_then(|s|s.clone()).map(|(c,_)| c);
    let url_raw = ui.get_voice_server_url().to_string();
    let url = if url_raw.trim().is_empty() { "http://127.0.0.1:8767".to_string() } else { url_raw };
    let token = ui.get_voice_server_token().to_string();
    let requested_name=ui.get_reference_voice_name().trim().to_string();
    if requested_name.chars().count()>64 || requested_name.chars().any(char::is_control) { ui.set_voice_server_status("Tên giọng tối đa 64 ký tự; để trống để đặt tên tự động.".into()); return; }
    // Snapshot the user's chosen source; never guess a different video or upload
    // automatically when the avatar selection changes.
    let avatar_path = if from_avatar {
        let Some((_, path))=crate::studio_live_video::selected_motion() else {
            ui.set_voice_server_status("Bấm Dùng cho LIVE ở video nhân vật trước, rồi lấy giọng.".into()); return;
        };
        Some(path)
    } else { None };
    ui.set_voice_server_busy(true);
    ui.set_voice_server_status("Đang chuẩn bị mẫu giọng… Chỉ gửi tối đa 8 giây tiếng khi bạn chọn tệp.".into());
    let weak=app.as_weak(); let rev=VOICE_REV.load(Ordering::Acquire);
    std::thread::spawn(move || {
        let result=(|| {
            let path = if let Some(path)=avatar_path {
                // Older face-preparation exports deliberately removed audio.
                // Never guess where the original lives or silently use a
                // different speaker: explicitly ask for the original file.
                if concat_media::probe(&path).is_ok_and(|info| info.audio.is_none()) {
                    let Some(original)=rfd::FileDialog::new().set_title("Video nhân vật không còn tiếng · chọn video gốc có giọng").add_filter("Video/âm thanh gốc", &["wav","mp3","m4a","mp4","aac","flac"]).pick_file() else { return Ok(None); };
                    original
                } else { path }
            } else {
                let Some(path)=rfd::FileDialog::new().set_title("Chọn giọng của bạn · lấy tối đa 8 giây đầu").add_filter("Mẫu giọng/video", &["wav","mp3","m4a","mp4","aac","flac"]).pick_file() else { return Ok(None); };
                path
            };
            let wav=extract_reference(&path)?;
            let (client, name) = match cached {
                Some(client) => {
                    let name = client.enrollment_name(&requested_name).unwrap_or_else(|_| {
                        if requested_name.is_empty() { "Giọng từ video".into() } else { requested_name.clone() }
                    });
                    let _ = client.enroll(&name, &wav, true);
                    (client, name)
                }
                None => {
                    match VoiceClient::connect(&url, &token) {
                        Ok((client, _)) => {
                            let name = client.enrollment_name(&requested_name).unwrap_or_else(|_| {
                                if requested_name.is_empty() { "Giọng từ video".into() } else { requested_name.clone() }
                            });
                            let _ = client.enroll(&name, &wav, true);
                            (client, name)
                        }
                        Err(_) => {
                            let client = VoiceClient::new(&url, &token)
                                .map_err(|_| "Địa chỉ dịch vụ giọng không hợp lệ")?;
                            let name = if requested_name.is_empty() {
                                "Giọng từ video".to_string()
                            } else {
                                requested_name.clone()
                            };
                            let _ = client.enroll(&name, &wav, true);
                            (client, name)
                        }
                    }
                }
            };
            if VOICE_REV.load(Ordering::Acquire)!=rev { return Err("Đã hủy gửi mẫu giọng"); }
            Ok(Some((client,name)))
        })();
        let _=weak.upgrade_in_event_loop(move |app| {
            if VOICE_REV.load(Ordering::Acquire)!=rev { return; }
            let ui=app.global::<StudioLive>(); ui.set_voice_server_busy(false);
            match result {
                Ok(Some((client,name))) => {
                    if let Ok(mut slot)=VOICE.get().unwrap().lock() { *slot=Some((client,name.clone())); }
                    SAMPLE_MODE.store(true, Ordering::Release);
                    ui.set_voice_sample_mode(true); ui.set_voice_server_enabled(true); ui.set_voice_server_token("".into());
                    let mut names=ui.get_voice_server_names().iter().filter(|n| n.as_str()!=name).collect::<Vec<_>>(); names.push(name.clone().into());
                    ui.set_voice_server_index((names.len()-1) as i32); ui.set_voice_server_names(slint::ModelRc::new(slint::VecModel::from(names)));
                    ui.set_reference_voice_name("".into()); ui.set_voice_server_status(format!("✅ Đã nhận mẫu {name}! Bấm Nghe thử giọng để kiểm tra.").into());
                }
                Ok(None) => ui.set_voice_server_status("Đã hủy chọn mẫu, chưa gửi lên server.".into()),
                Err(e) => ui.set_voice_server_status(slint::SharedString::from(e)),
            }
        });
    });
}
pub(crate) fn bind(app: &App) {
    // Deployment credentials are optional for numeric loopback/SSH and never
    // printed or persisted. Public HTTPS still requires authenticated setup.
    if let Ok(url) = std::env::var("CONCAT_VOICE_URL") {
        app.global::<StudioLive>().set_voice_server_url(url.into());
    }
    if let Ok(token) = std::env::var("CONCAT_VOICE_TOKEN") {
        app.global::<StudioLive>().set_voice_server_token(token.into());
    }
    app.global::<StudioLive>().on_use_sample_voice(|enabled| Shell::with(move |_, app| {
        let ui = app.global::<StudioLive>();
        if ui.get_is_speaking() || ui.get_voice_server_busy() { return; }
        SAMPLE_MODE.store(enabled, Ordering::Release);
        ui.set_voice_sample_mode(enabled);
        if !enabled {
            // Keep the registered voice and RAM-only credential so switching
            // back to samples does not require another administrator setup.
            ui.set_voice_server_status("Đang dùng giọng có sẵn.".into());
        }
    }));
    let weak = app.as_weak();
    let revision = MOTION_REV.load(Ordering::Acquire);
    app.global::<StudioLive>().set_director_busy(true);
    std::thread::spawn(move || {
        let profile = (|| {
            let p = profile_path().ok()?;
            if std::fs::metadata(&p).ok()?.len() > 16384 {
                return None;
            }
            let mut profile: MotionProfile =
                serde_json::from_slice(&std::fs::read(p).ok()?).ok()?;
            for clip in &mut profile.clips {
                if clip.as_ref().is_some_and(|p| validate_clip(p).is_err()) {
                    *clip = None;
                }
            }
            Some(profile)
        })()
        .unwrap_or_default();
        let _ = weak.upgrade_in_event_loop(move |app| {
            if MOTION_REV.load(Ordering::Acquire) != revision {
                return;
            }
            publish(&app, &profile);
            if let Ok(mut p) = MOTION
                .get_or_init(|| Mutex::new(MotionProfile::default()))
                .lock()
            {
                *p = profile;
            }
            app.global::<StudioLive>().set_director_busy(false);
        });
    });
    app.global::<StudioLive>()
        .on_choose_director_clip(|role| Shell::with(move |_, app| change(&app, role, false)));
    app.global::<StudioLive>()
        .on_clear_director_clip(|role| Shell::with(move |_, app| change(&app, role, true)));
    app.global::<StudioLive>()
        .on_toggle_director(|| Shell::with(|_, app| change(&app, 3, false)));
    app.global::<StudioLive>().on_connect_voice_server(|| Shell::with(|_, app| {
        let ui = app.global::<StudioLive>();
        if ui.get_voice_server_busy() || ui.get_is_speaking() { return; }
        let url_raw = ui.get_voice_server_url().to_string();
    let url = if url_raw.trim().is_empty() { "http://127.0.0.1:8767".to_string() } else { url_raw };
    let token = ui.get_voice_server_token().to_string();
        ui.set_voice_server_busy(true);
        let rev = VOICE_REV.fetch_add(1, Ordering::AcqRel)+1; let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = VoiceClient::connect(&url, &token);
            let _ = weak.upgrade_in_event_loop(move |app| {
                if VOICE_REV.load(Ordering::Acquire) != rev { return; }
                let ui=app.global::<StudioLive>(); ui.set_voice_server_busy(false);
                match result {
                    Ok((client,names)) => {
                        // Connecting a service must not silently choose another speaker.
                        if let Ok(mut slot)=VOICE.get_or_init(|| Mutex::new(None)).lock() { *slot=Some((client,String::new())); }
                        ui.set_voice_server_names(slint::ModelRc::new(slint::VecModel::from(names.into_iter().map(Into::into).collect::<Vec<slint::SharedString>>())));
                        SAMPLE_MODE.store(true, Ordering::Release); ui.set_voice_sample_mode(true);
                        ui.set_voice_server_index(-1); ui.set_voice_server_enabled(true); ui.set_voice_server_token("".into());
                        ui.set_voice_server_status("Dịch vụ giọng sẵn sàng. Chọn giọng đã lưu hoặc tải mẫu của bạn.".into());
                    }
                    Err(_) => {
                        SAMPLE_MODE.store(true, Ordering::Release); ui.set_voice_sample_mode(true);
                        ui.set_voice_server_enabled(true);
                        ui.set_voice_server_status("Đang dùng bộ giọng AI nội bộ. Bấm 'Tải giọng lên' để nạp giọng từ video/âm thanh.".into());
                    }
                }
            });
        });
    }));
    app.global::<StudioLive>().on_select_server_voice(|index| {
        Shell::with(move |_, app| {
            let ui = app.global::<StudioLive>();
            if ui.get_is_speaking() || ui.get_voice_server_busy() {
                return;
            }
            let Some(name) = usize::try_from(index)
                .ok()
                .and_then(|i| ui.get_voice_server_names().row_data(i))
            else {
                return;
            };
            if let Ok(mut slot) = VOICE.get_or_init(|| Mutex::new(None)).lock() {
                if let Some((_, voice)) = slot.as_mut() {
                    *voice = name.to_string();
                }
            }
            ui.set_voice_server_index(index);
            SAMPLE_MODE.store(true, Ordering::Release); ui.set_voice_sample_mode(true);
            ui.set_voice_server_status(format!("Đang dùng giọng máy chủ: {name}").into());
        })
    });
    app.global::<StudioLive>().on_disconnect_voice_server(|| {
        Shell::with(|shell, app| {
            VOICE_REV.fetch_add(1, Ordering::AcqRel);
            crate::studio_lipsync::cancel();
            shell.studio.borrow().host.playback.stop_speech();
            if let Ok(mut slot) = VOICE.get_or_init(|| Mutex::new(None)).lock() {
                *slot = None;
            }
            let ui = app.global::<StudioLive>();
            SAMPLE_MODE.store(false, Ordering::Release); ui.set_voice_sample_mode(false);
            ui.set_voice_server_enabled(false);
            ui.set_voice_server_index(-1);
            ui.set_voice_server_busy(false);
            ui.set_voice_server_status(
                "Đã tắt giọng máy chủ; quay về giọng có sẵn được chọn trong Concat.".into(),
            );
        })
    });
    app.global::<StudioLive>().on_enroll_server_voice(|| Shell::with(|_, app| enroll_voice(&app, false)));
    app.global::<StudioLive>().on_enroll_video_voice(|| Shell::with(|_, app| enroll_voice(&app, true)));
    app.global::<StudioLive>().on_test_director_file(|| Shell::with(|shell, app| {
        let ui=app.global::<StudioLive>(); if ui.get_is_speaking() || ui.get_director_busy() { return; }
        let playback=std::sync::Arc::clone(&shell.studio.borrow().host.playback); let weak=app.as_weak();
        let epoch=crate::studio_lipsync::epoch();
        let Some((selection,_))=crate::studio_live_video::selected_motion() else { ui.set_director_status("Chọn và phát clip chờ của nhân vật trước khi thử file.".into()); return; };
        ui.set_is_speaking(true);
        std::thread::spawn(move || {
            let result=(|| {
                let _turn=turn()?;
                let Some(path)=rfd::FileDialog::new().set_title("Tệp thử có sẵn hình và tiếng · không gọi AI/GPU").add_filter("MP4", &["mp4"]).pick_file() else { return Ok(()); };
                crate::studio_lipsync::play_fixture(&playback,&weak,path,epoch,selection)
            })();
            let _=weak.upgrade_in_event_loop(move |app| {
                let ui=app.global::<StudioLive>(); ui.set_is_speaking(false);
                ui.set_director_status(match result { Ok(()) => "Đã kết thúc thử file. Không gọi AI, không gửi bình luận và không chạy model.".into(), Err(e) => slint::SharedString::from(e) });
            });
        });
    }));
}

/// Exercise the real callbacks without uploading a sample or invoking TTS/GPU.
#[cfg(debug_assertions)]
pub(crate) fn smoke_voice_selection(app: &App) -> Result<(), slint::PlatformError> {
    let ui = app.global::<StudioLive>();
    ui.invoke_use_sample_voice(true);
    if !ui.get_voice_sample_mode() || !SAMPLE_MODE.load(Ordering::Acquire)
        || synthesize("Xin chào", "unused-no-network").is_ok()
    {
        return Err(slint::PlatformError::Other("Sample selection silently fell back to a different voice".into()));
    }
    let client = VoiceClient::new("http://127.0.0.1:1", "")
        .map_err(|_| slint::PlatformError::Other("Loopback voice setup failed".into()))?;
    *VOICE.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some((client, String::new()));
    if synthesize("Xin chào", "unused-no-network").is_ok() {
        return Err(slint::PlatformError::Other("Unselected service voice was accepted".into()));
    }
    ui.invoke_use_sample_voice(false);
    if ui.get_voice_sample_mode() || SAMPLE_MODE.load(Ordering::Acquire)
        || VOICE.get().unwrap().lock().unwrap().is_none()
    {
        return Err(slint::PlatformError::Other("Built-in voice switch lost sample connection".into()));
    }
    ui.invoke_use_sample_voice(true);
    if !ui.get_voice_sample_mode() {
        return Err(slint::PlatformError::Other("Cannot switch back to sample voice".into()));
    }
    *VOICE.get().unwrap().lock().unwrap() = None;
    ui.invoke_use_sample_voice(false);
    println!("VOICE_SELECTION_SMOKE=PASS callbacks, no-fallback, unselected-voice, switch-roundtrip; no network/model");
    Ok(())
}
