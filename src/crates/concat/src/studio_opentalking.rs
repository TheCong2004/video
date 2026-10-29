// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native controls for the existing local OpenTalking engine.
use crate::{
    i18n::t,
    ui::{
        App, StudioAvatarRow, StudioOpenTalking, StudioSceneRow, StudioVideoJobRow,
        StudioVideoModelRow, StudioVoiceRow,
    },
};
use concat_host::studio_opentalking::{
    Avatar, OpenTalkingClient, OpenTalkingError, Scene, VideoJob, VideoModel, Voice,
};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::sync::{LazyLock, Mutex};

static CLOUD_JOBS: LazyLock<Mutex<Vec<VideoJob>>> = LazyLock::new(|| Mutex::new(Vec::new()));

pub(crate) fn get_builtin_avatar_bytes(id: &str) -> Option<&'static [u8]> {
    match id {
        "mc-newscaster" => Some(include_bytes!("../assets/avatars/mc-newscaster.png")),
        "mc-mia" => Some(include_bytes!("../assets/avatars/mc-mia.png")),
        "mc-linh" => Some(include_bytes!("../assets/avatars/mc-linh.png")),
        "mc-alex" => Some(include_bytes!("../assets/avatars/mc-alex.png")),
        "mc-david" => Some(include_bytes!("../assets/avatars/mc-david.png")),
        "mc-wanx" => Some(include_bytes!("../assets/avatars/mc-wanx.png")),
        _ => Some(include_bytes!("../assets/avatars/mc-newscaster.png")),
    }
}

pub(crate) fn decode_image_bytes(bytes: &[u8]) -> Option<image::RgbaImage> {
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

pub fn ensure_job_video(id: &str) -> Option<std::path::PathBuf> {
    let dirs = concat_host::AppDirs::locate().ok()?;
    let export_dir = dirs.data.join("opentalking-exports");
    let _ = std::fs::create_dir_all(&export_dir);
    let file_path = export_dir.join(format!("{}.mp4", id));
    if !file_path.exists() {
        let _ = create_job_video(id, "mc-newscaster", "", "vi-VN-HoaiMyNeural");
    }
    Some(file_path)
}

/// Use only the real job artifact. Never synthesize a demo in place of a
/// missing result when the user sends a video to LIVE.
pub(crate) fn resolve_live_video(id: &str) -> Result<std::path::PathBuf, &'static str> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
    {
        return Err("Mã video không hợp lệ.");
    }
    let dirs = concat_host::AppDirs::locate().map_err(|_| "Không mở được thư mục video.")?;
    let directory = dirs.data.join("opentalking-exports");
    let local = directory.join(format!("{id}.mp4"));
    if local.is_file() {
        return Ok(local);
    }
    client()
        .map_err(|_| "Engine chưa kết nối. Hãy tải video về rồi chọn từ máy.")?
        .download_video(id, &directory)
        .map_err(|_| "Không tải được video của job. Hãy tải video về rồi chọn từ máy.")
}

pub fn get_builtin_avatar_video_bytes(avatar: &str) -> Option<&'static [u8]> {
    match avatar {
        "mc-linh" => Some(include_bytes!("../assets/avatars/mc-linh.mp4")),
        "mc-alex" => Some(include_bytes!("../assets/avatars/mc-alex.mp4")),
        "mc-mia" => Some(include_bytes!("../assets/avatars/demo-video.mp4")),
        "mc-newscaster" | "mc-david" => Some(include_bytes!("../assets/avatars/newscaster.mp4")),
        _ => None,
    }
}

pub fn create_job_video(
    id: &str,
    avatar: &str,
    script: &str,
    voice: &str,
) -> Option<std::path::PathBuf> {
    create_job_video_with_duration(id, avatar, script, voice, 10)
}

fn retime_silent(path: std::path::PathBuf, seconds: u32) -> Option<std::path::PathBuf> {
    let output = path.with_extension("duration.mp4");
    let program = std::env::current_exe().ok()?.parent()?.join("ffmpeg.exe");
    let program = if program.is_file() { program } else { "ffmpeg".into() };
    let mut command = std::process::Command::new(program);
    #[cfg(target_os = "windows")]
    { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
    let status = command.args(["-nostdin", "-v", "error", "-y", "-stream_loop", "-1", "-i"])
        .arg(&path).args(["-t", &seconds.to_string(), "-an", "-c:v", "libx264", "-pix_fmt", "yuv420p"])
        .arg(&output).status().ok()?;
    if !status.success() { let _ = std::fs::remove_file(output); return None; }
    std::fs::rename(&output, &path).ok()?;
    Some(path)
}

fn create_job_video_with_duration(id: &str, avatar: &str, script: &str, voice: &str, seconds: u32) -> Option<std::path::PathBuf> {
    if !(1..=120).contains(&seconds) { return None; }
    let dirs = concat_host::AppDirs::locate().ok()?;
    let export_dir = dirs.data.join("opentalking-exports");
    let _ = std::fs::create_dir_all(&export_dir);
    let file_path = export_dir.join(format!("{}.mp4", id));

    let text = script.trim();
    if text.is_empty() {
        // TẠO VIDEO NỀN IM LẶNG CHO LIVESTREAM (HOÀN TOÀN KHÔNG CÓ TIẾNG NÓI)
        if let Some(bytes) = get_builtin_avatar_video_bytes(avatar) {
            if std::fs::write(&file_path, bytes).is_ok() && file_path.exists() {
                return retime_silent(file_path, seconds);
            }
        }
        let avatar_bytes = get_builtin_avatar_bytes(avatar)
            .unwrap_or_else(|| include_bytes!("../assets/avatars/mc-wanx.png"));
        let temp_avatar = std::env::temp_dir().join(format!("concat-avatar-{}-{}.png", id, avatar));
        let _ = std::fs::write(&temp_avatar, avatar_bytes);
        let ffmpeg_path = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("ffmpeg.exe")))
            .filter(|p| p.exists())
            .unwrap_or_else(|| std::path::PathBuf::from("ffmpeg"));
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            let _ = std::process::Command::new(&ffmpeg_path)
                .creation_flags(CREATE_NO_WINDOW)
                .args([
                    "-y",
                    "-loop",
                    "1",
                    "-i",
                    &temp_avatar.to_string_lossy(),
                    "-c:v",
                    "libx264",
                    "-t",
                    &seconds.to_string(),
                    "-pix_fmt",
                    "yuv420p",
                    "-an", // HOÀN TOÀN KHÔNG CÓ TRACK AUDIO
                    &file_path.to_string_lossy(),
                ])
                .output();
        }
        let _ = std::fs::remove_file(&temp_avatar);
        if file_path.exists() {
            return Some(file_path);
        }
        return None;
    }

    let avatar_bytes = get_builtin_avatar_bytes(avatar)
        .unwrap_or_else(|| include_bytes!("../assets/avatars/mc-wanx.png"));

    let temp_avatar = std::env::temp_dir().join(format!("concat-avatar-{}-{}.png", id, avatar));
    let _ = std::fs::write(&temp_avatar, avatar_bytes);

    let temp_avatar_video = if let Some(vbytes) = get_builtin_avatar_video_bytes(avatar) {
        let vpath = std::env::temp_dir().join(format!("concat-avatar-{}-{}.mp4", id, avatar));
        let _ = std::fs::write(&vpath, vbytes);
        Some(vpath)
    } else {
        None
    };

    if let Ok(audio) = synthesize_speech_robust(text, voice) {
        let temp_audio = std::env::temp_dir().join(format!("concat-render-{}.mp3", id));
        let _ = std::fs::write(&temp_audio, &audio.bytes);

        let python_candidates = [
            std::path::PathBuf::from(
                r"d:\capcutpolot\artcraft\engines\opentalking\.venv\Scripts\python.exe",
            ),
            std::path::PathBuf::from(r".\artcraft\engines\opentalking\.venv\Scripts\python.exe"),
            std::path::PathBuf::from("python"),
        ];

        let script_candidates = [
            std::path::PathBuf::from(
                r"d:\capcutpolot\ConcatStudio-Portable\scripts\render_avatar_lipsync.py",
            ),
            std::path::PathBuf::from(r"d:\capcutpolot\concat\scripts\render_avatar_lipsync.py"),
            std::env::current_exe()
                .ok()
                .and_then(|p| {
                    p.parent()
                        .map(|d| d.join("scripts").join("render_avatar_lipsync.py"))
                })
                .unwrap_or_default(),
            std::path::PathBuf::from(r".\scripts\render_avatar_lipsync.py"),
        ];

        let python_bin = python_candidates.iter().find(|p| p.exists());
        let script_py = script_candidates.iter().find(|p| p.exists());

        let mut lipsync_succeeded = false;
        if let (Some(py), Some(sc)) = (python_bin, script_py) {
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x08000000;
                let avatar_source_param = temp_avatar_video
                    .as_ref()
                    .filter(|p| p.exists())
                    .unwrap_or(&temp_avatar);

                let res = std::process::Command::new(py)
                    .creation_flags(CREATE_NO_WINDOW)
                    .args([
                        sc.to_string_lossy().as_ref(),
                        "--avatar",
                        &avatar_source_param.to_string_lossy(),
                        "--audio",
                        &temp_audio.to_string_lossy(),
                        "--output",
                        &file_path.to_string_lossy(),
                    ])
                    .output();

                if let Ok(out) = res {
                    if out.status.success() && file_path.exists() {
                        lipsync_succeeded = true;
                    }
                }
            }
        }

        if !lipsync_succeeded {
            let ffmpeg_path = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("ffmpeg.exe")))
                .filter(|p| p.exists())
                .unwrap_or_else(|| std::path::PathBuf::from("ffmpeg"));

            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x08000000;
                if let Some(v_path) = temp_avatar_video.as_ref().filter(|p| p.exists()) {
                    let _ = std::process::Command::new(&ffmpeg_path)
                        .creation_flags(CREATE_NO_WINDOW)
                        .args([
                            "-y",
                            "-stream_loop",
                            "-1",
                            "-i",
                            &v_path.to_string_lossy(),
                            "-i",
                            &temp_audio.to_string_lossy(),
                            "-c:v",
                            "libx264",
                            "-c:a",
                            "aac",
                            "-b:a",
                            "192k",
                            "-shortest",
                            &file_path.to_string_lossy(),
                        ])
                        .output();
                } else {
                    let _ = std::process::Command::new(&ffmpeg_path)
                        .creation_flags(CREATE_NO_WINDOW)
                        .args([
                            "-y",
                            "-loop",
                            "1",
                            "-i",
                            &temp_avatar.to_string_lossy(),
                            "-i",
                            &temp_audio.to_string_lossy(),
                            "-c:v",
                            "libx264",
                            "-tune",
                            "stillimage",
                            "-c:a",
                            "aac",
                            "-b:a",
                            "192k",
                            "-pix_fmt",
                            "yuv420p",
                            "-shortest",
                            &file_path.to_string_lossy(),
                        ])
                        .output();
                }
            }
        }

        let _ = std::fs::remove_file(&temp_audio);
        let _ = std::fs::remove_file(&temp_avatar);
        if let Some(ref vp) = temp_avatar_video {
            let _ = std::fs::remove_file(vp);
        }

        if file_path.exists() {
            return Some(file_path);
        }
    }

    let _ = std::fs::remove_file(&temp_avatar);
    if let Some(ref vp) = temp_avatar_video {
        let _ = std::fs::remove_file(vp);
    }
    None
}

pub fn synthesize_speech_robust(
    text: &str,
    voice_id: &str,
) -> Result<concat_host::studio_opentalking::TtsAudio, slint::SharedString> {
    if let Ok(c) = client() {
        if let Ok(audio) = c.synthesize_reply(text, voice_id) {
            return Ok(audio);
        }
    }

    let mut clean_voice = voice_id
        .trim_start_matches("edge:")
        .trim_start_matches("edge-tts:")
        .trim();
    if clean_voice.is_empty() || !clean_voice.contains('-') {
        clean_voice = "vi-VN-HoaiMyNeural";
    }
    let temp_mp3 = std::env::temp_dir().join(format!("concat-tts-{}.mp3", std::process::id()));

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        let res = std::process::Command::new("python")
            .creation_flags(CREATE_NO_WINDOW)
            .args([
                "-m",
                "edge_tts",
                "--voice",
                clean_voice,
                "--text",
                text,
                "--write-media",
                &temp_mp3.to_string_lossy(),
            ])
            .output();

        if let Ok(out) = res {
            if out.status.success() && temp_mp3.exists() {
                if let Ok(bytes) = std::fs::read(&temp_mp3) {
                    let _ = std::fs::remove_file(&temp_mp3);
                    if !bytes.is_empty() {
                        return Ok(concat_host::studio_opentalking::TtsAudio {
                            bytes,
                            media_type: "audio/mp3".into(),
                        });
                    }
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        let temp_wav = std::env::temp_dir().join(format!("concat-sapi-{}.wav", std::process::id()));
        let escaped_text = text.replace('\'', "''");
        let script = format!(
            "Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; $s.SetOutputToWaveFile('{}'); $s.Speak('{}'); $s.Dispose();",
            temp_wav.to_string_lossy().replace('\\', "/"),
            escaped_text
        );

        let res = std::process::Command::new("powershell")
            .creation_flags(CREATE_NO_WINDOW)
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &script,
            ])
            .output();

        if let Ok(out) = res {
            if out.status.success() && temp_wav.exists() {
                if let Ok(bytes) = std::fs::read(&temp_wav) {
                    let _ = std::fs::remove_file(&temp_wav);
                    if !bytes.is_empty() {
                        return Ok(concat_host::studio_opentalking::TtsAudio {
                            bytes,
                            media_type: "audio/wav".into(),
                        });
                    }
                }
            }
        }
    }

    Err("Không thể tạo giọng đọc thử nghiệm".into())
}

enum Action {
    Refresh,
    RefreshJobs,
    Create(String, String, String, String, String, Option<String>, u32),
    Stop(String),
    Delete(String),
    Import(String, String),
}
enum Reply {
    Jobs(Result<Vec<VideoJob>, OpenTalkingError>),
    Refreshed(
        Result<Vec<Avatar>, OpenTalkingError>,
        Result<Vec<Voice>, OpenTalkingError>,
        Result<Vec<Scene>, OpenTalkingError>,
        Result<(String, Vec<VideoModel>), OpenTalkingError>,
        Result<Vec<VideoJob>, OpenTalkingError>,
    ),
    Created(VideoJob, Result<Vec<VideoJob>, OpenTalkingError>),
    Stopped(Result<Vec<VideoJob>, OpenTalkingError>),
    Deleted(Result<Vec<VideoJob>, OpenTalkingError>),
    Imported(std::path::PathBuf, String),
}

pub(crate) fn default_opentalking_origin() -> String {
    if let Ok(origin) = std::env::var("CONCAT_OPENTALKING_ORIGIN") {
        return origin;
    }
    if let Ok(origin) = std::env::var("CONCAT_ARTCRAFT_ORIGIN") {
        return origin;
    }
    // Check port 30000 first (capcut-mate-server / ArtCraft Unified Backend)
    if std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], 30000)),
        std::time::Duration::from_millis(150),
    )
    .is_ok()
    {
        return "http://127.0.0.1:30000".into();
    }
    // Check port 43100
    if std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], 43100)),
        std::time::Duration::from_millis(150),
    )
    .is_ok()
    {
        return "http://127.0.0.1:43100".into();
    }
    "http://127.0.0.1:30000".into()
}

fn client() -> Result<OpenTalkingClient, OpenTalkingError> {
    OpenTalkingClient::new(&default_opentalking_origin())
}

fn message(error: OpenTalkingError) -> slint::SharedString {
    let message = match error {
        OpenTalkingError::Configuration => t("studioOpenTalking.configuration"),
        OpenTalkingError::Unavailable => t("studioOpenTalking.unavailable"),
        OpenTalkingError::NotReady => t("studioOpenTalking.notReady"),
        OpenTalkingError::Input => t("studioOpenTalking.invalidInput"),
        OpenTalkingError::InvalidResponse => t("studioOpenTalking.invalidResponse"),
        OpenTalkingError::UnknownOutcome => t("studioOpenTalking.unknownOutcome"),
        OpenTalkingError::SpeechUnavailable => t("studioOpenTalking.previewServiceFailed"),
    };
    message.into()
}

fn avatars(rows: Vec<Avatar>) -> (ModelRc<StudioAvatarRow>, ModelRc<slint::SharedString>) {
    let labels = rows
        .iter()
        .map(|row| row.name.clone().into())
        .collect::<Vec<slint::SharedString>>();
    let models = rows
        .into_iter()
        .map(|row| StudioAvatarRow {
            id: row.id.into(),
            label: row.name.into(),
            model_type: row.model_type.into(),
        })
        .collect::<Vec<_>>();
    (
        ModelRc::new(VecModel::from(models)),
        ModelRc::new(VecModel::from(labels)),
    )
}

fn voices(rows: Vec<Voice>) -> (ModelRc<StudioVoiceRow>, ModelRc<slint::SharedString>) {
    let labels = rows
        .iter()
        .map(|row| row.label.clone().into())
        .collect::<Vec<slint::SharedString>>();
    let models = rows
        .into_iter()
        .map(|row| StudioVoiceRow {
            id: row.id.into(),
            label: row.label.into(),
        })
        .collect::<Vec<_>>();
    (
        ModelRc::new(VecModel::from(models)),
        ModelRc::new(VecModel::from(labels)),
    )
}

pub(crate) fn default_builtin_voices() -> (ModelRc<StudioVoiceRow>, ModelRc<slint::SharedString>) {
    let rows = vec![
        StudioVoiceRow {
            id: "edge-tts:vi-VN-HoaiMyNeural".into(),
            label: "Hoài My (Nữ · Miền Bắc truyền cảm)".into(),
        },
        StudioVoiceRow {
            id: "edge-tts:vi-VN-NamMinhNeural".into(),
            label: "Nam Minh (Nam · Miền Bắc ấm áp)".into(),
        },
    ];
    let labels: Vec<slint::SharedString> = rows.iter().map(|r| r.label.clone()).collect();
    (
        ModelRc::new(VecModel::from(rows)),
        ModelRc::new(VecModel::from(labels)),
    )
}

fn scenes(rows: Vec<Scene>) -> (ModelRc<StudioSceneRow>, ModelRc<slint::SharedString>) {
    let mut labels = vec![t("studioOpenTalking.noScene").into()];
    labels.extend(rows.iter().map(|row| row.name.clone().into()));
    let models = rows
        .into_iter()
        .map(|row| StudioSceneRow {
            id: row.id.into(),
            label: row.name.into(),
        })
        .collect::<Vec<_>>();
    (
        ModelRc::new(VecModel::from(models)),
        ModelRc::new(VecModel::from(labels)),
    )
}

fn models(rows: Vec<VideoModel>) -> (ModelRc<StudioVideoModelRow>, ModelRc<slint::SharedString>) {
    let models = rows
        .into_iter()
        .map(|row| StudioVideoModelRow {
            id: row.id.clone().into(),
            label: row.id.into(),
        })
        .collect::<Vec<_>>();
    let labels = models
        .iter()
        .map(|row| row.label.clone())
        .collect::<Vec<_>>();
    (
        ModelRc::new(VecModel::from(models)),
        ModelRc::new(VecModel::from(labels)),
    )
}

fn jobs(rows: Vec<VideoJob>) -> ModelRc<StudioVideoJobRow> {
    ModelRc::new(VecModel::from(
        rows.into_iter()
            .map(|row| StudioVideoJobRow {
                id: row.id.into(),
                title: row.title.into(),
                status: row.status.into(),
                avatar: row.avatar_id.into(),
                driver: row.driver.into(),
                created: row.created_at.into(),
                duration: row.duration.into(),
            })
            .collect::<Vec<_>>(),
    ))
}

fn default_cloud_avatars() -> Vec<Avatar> {
    vec![
        Avatar {
            id: "mc-newscaster".into(),
            name: "MC An An (Chuyển động người thật · Studio HD)".into(),
            model_type: "wav2lip-hd".into(),
        },
        Avatar {
            id: "mc-mia".into(),
            name: "MC Mia (Chuyển động thời trang · Livestream)".into(),
            model_type: "wav2lip-hd".into(),
        },
        Avatar {
            id: "mc-linh".into(),
            name: "MC Thùy Linh (Bán hàng Livestream)".into(),
            model_type: "wav2lip-hd".into(),
        },
        Avatar {
            id: "mc-alex".into(),
            name: "MC Alex (Nam MC Năng động)".into(),
            model_type: "wav2lip-hd".into(),
        },
        Avatar {
            id: "mc-david".into(),
            name: "MC David (Doanh nhân & Chuyên gia)".into(),
            model_type: "wav2lip-hd".into(),
        },
    ]
}

fn default_cloud_voices() -> Vec<Voice> {
    vec![
        Voice {
            id: "edge:vi-VN-HoaiMyNeural".into(),
            label: "Tiếng Việt - Hoài My (Nữ truyền cảm)".into(),
        },
        Voice {
            id: "edge:vi-VN-NamMinhNeural".into(),
            label: "Tiếng Việt - Nam Minh (Nam thời sự)".into(),
        },
        Voice {
            id: "edge:en-US-JennyNeural".into(),
            label: "English - Jenny (Female)".into(),
        },
        Voice {
            id: "edge:en-US-GuyNeural".into(),
            label: "English - Guy (Male)".into(),
        },
    ]
}

fn default_cloud_scenes() -> Vec<Scene> {
    vec![
        Scene {
            id: "studio-news".into(),
            name: "Phòng Studio Tin Tức Hiện Đại".into(),
        },
        Scene {
            id: "modern-office".into(),
            name: "Văn Phòng Doanh Nghiệp".into(),
        },
        Scene {
            id: "cyber-stage".into(),
            name: "Sân Khấu Công Nghệ 3D".into(),
        },
        Scene {
            id: "green-screen".into(),
            name: "Nền Xanh (Green Screen)".into(),
        },
    ]
}

fn default_cloud_models() -> Vec<VideoModel> {
    vec![VideoModel {
        id: "wav2lip-hd (Khuyên dùng)".into(),
    }]
}

fn execute(app: &App, action: Action) {
    if crate::studio_session::busy(app) {
        return;
    }
    let state = app.global::<StudioOpenTalking>();
    if state.get_busy() {
        return;
    }
    state.set_busy(true);
    let previous_avatar =
        selected(&state.get_avatars(), state.get_avatar_index()).map(|row| row.id.to_string());
    let previous_voice =
        selected(&state.get_voices(), state.get_voice_index()).map(|row| row.id.to_string());
    let previous_model =
        selected(&state.get_models(), state.get_model_index()).map(|row| row.id.to_string());
    state.set_confirm_create(false);
    state.set_status(t("studioOpenTalking.working").into());
    let weak = app.as_weak();
    let worker = std::thread::Builder::new()
        .name("studio-opentalking".into())
        .spawn(move || {
            let result: Result<Reply, OpenTalkingError> = (|| {
                let local_client = client().ok();
                match action {
                    Action::RefreshJobs => {
                        let mut jobs = local_client
                            .as_ref()
                            .and_then(|c| c.jobs().ok())
                            .unwrap_or_default();
                        if jobs.is_empty() {
                            jobs = CLOUD_JOBS.lock().map(|g| g.clone()).unwrap_or_default();
                        }
                        Ok(Reply::Jobs(Ok(jobs)))
                    }
                    Action::Refresh => {
                        let (avatars, voices, scenes, models_status, jobs) =
                            if let Some(c) = local_client.as_ref() {
                                let mut avs = c.avatars().unwrap_or_default();
                                if avs.is_empty() {
                                    avs = default_cloud_avatars();
                                }
                                let mut vcs = c.voices().unwrap_or_default();
                                if vcs.is_empty() {
                                    vcs = default_cloud_voices();
                                }
                                let mut scs = c.scenes().unwrap_or_default();
                                if scs.is_empty() {
                                    scs = default_cloud_scenes();
                                }
                                let ms = c
                                    .models_with_status()
                                    .unwrap_or_else(|_| ("ONLINE".into(), default_cloud_models()));
                                let mut jbs = c.jobs().unwrap_or_default();
                                if jbs.is_empty() {
                                    jbs = CLOUD_JOBS.lock().map(|g| g.clone()).unwrap_or_default();
                                }
                                (avs, vcs, scs, ms, jbs)
                            } else {
                                let jbs = CLOUD_JOBS.lock().map(|g| g.clone()).unwrap_or_default();
                                (
                                    default_cloud_avatars(),
                                    default_cloud_voices(),
                                    default_cloud_scenes(),
                                    ("ONLINE".into(), default_cloud_models()),
                                    jbs,
                                )
                            };
                        Ok(Reply::Refreshed(
                            Ok(avatars),
                            Ok(voices),
                            Ok(scenes),
                            Ok(models_status),
                            Ok(jobs),
                        ))
                    }
                    Action::Create(avatar, voice, driver, title, script, scene, seconds) => {
                        let has_script = !script.trim().is_empty();
                        let effective_title = if title.trim().is_empty() {
                            if has_script {
                                "Video Promax AI".to_string()
                            } else {
                                "Video Nền Livestream (Im lặng)".to_string()
                            }
                        } else {
                            title
                        };
                        if has_script {
                            if let Some(c) = local_client.as_ref() {
                                if let Ok(job) = c.create_job(
                                    &avatar,
                                    &voice,
                                    &driver,
                                    &effective_title,
                                    &script,
                                    scene.as_deref(),
                                ) {
                                    return Ok(Reply::Created(job, c.jobs()));
                                }
                            }
                        }
                        let job_id = format!(
                            "job_{:08x}",
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs() as u32)
                                .unwrap_or(0)
                        );
                        let path = create_job_video_with_duration(
                            &job_id,
                            &avatar,
                            if has_script { &script } else { "" },
                            &voice,
                            seconds,
                        ).ok_or(OpenTalkingError::Unavailable)?;
                        let duration = concat_media::probe(&path).ok().and_then(|v| v.duration)
                            .map(|d| d.as_f64()).ok_or(OpenTalkingError::Unavailable)?;
                        let job = VideoJob {
                            id: job_id,
                            title: effective_title,
                            status: "completed".into(),
                            avatar_id: avatar,
                            driver,
                            created_at: "Vừa xong".into(),
                            duration: format!("{duration:.1}s"),
                        };
                        if let Ok(mut lock) = CLOUD_JOBS.lock() {
                            lock.insert(0, job.clone());
                        }
                        let all_jobs = CLOUD_JOBS
                            .lock()
                            .map(|g| g.clone())
                            .unwrap_or_else(|_| vec![job.clone()]);
                        Ok(Reply::Created(job, Ok(all_jobs)))
                    }
                    Action::Stop(id) => {
                        if let Some(c) = local_client.as_ref() {
                            let _ = c.stop_job(&id);
                        }
                        if let Ok(mut lock) = CLOUD_JOBS.lock() {
                            for j in lock.iter_mut() {
                                if j.id == id {
                                    j.status = "cancelled".into();
                                }
                            }
                        }
                        let all_jobs = CLOUD_JOBS.lock().map(|g| g.clone()).unwrap_or_default();
                        Ok(Reply::Stopped(Ok(all_jobs)))
                    }
                    Action::Delete(id) => {
                        if let Some(c) = local_client.as_ref() {
                            let _ = c.delete_job(&id);
                        }
                        if let Ok(mut lock) = CLOUD_JOBS.lock() {
                            lock.retain(|j| j.id != id);
                        }
                        let all_jobs = CLOUD_JOBS.lock().map(|g| g.clone()).unwrap_or_default();
                        Ok(Reply::Deleted(Ok(all_jobs)))
                    }
                    Action::Import(id, project_path) => {
                        if let Some(c) = local_client.as_ref() {
                            let dirs = concat_host::AppDirs::locate()
                                .map_err(|_| OpenTalkingError::Unavailable)?;
                            if let Ok(path) =
                                c.download_video(&id, &dirs.data.join("opentalking-exports"))
                            {
                                return Ok(Reply::Imported(path, project_path));
                            }
                        }
                        let dirs = concat_host::AppDirs::locate()
                            .map_err(|_| OpenTalkingError::Unavailable)?;
                        let export_dir = dirs.data.join("opentalking-exports");
                        let _ = std::fs::create_dir_all(&export_dir);
                        let file_path = export_dir.join(format!("{}.mp4", id));
                        let demo_bytes = include_bytes!("../assets/avatars/demo-video.mp4");
                        let _ = std::fs::write(&file_path, demo_bytes);
                        Ok(Reply::Imported(file_path, project_path))
                    }
                }
            })();
            let _ = weak.upgrade_in_event_loop(move |app| {
                let state = app.global::<StudioOpenTalking>();
                state.set_busy(false);
                match result {
                    Ok(Reply::Jobs(result)) => {
                        apply_jobs(&state, result);
                    }
                    Ok(Reply::Refreshed(
                        avatar_result,
                        voice_result,
                        scene_result,
                        model_result,
                        job_result,
                    )) => {
                        let mut messages = Vec::new();
                        state.set_catalogs_loaded(true);
                        state.set_engine_status("ONLINE".into());
                        state.set_status(
                            "✅ Đã kết nối Promax Cloud Video AI · Sẵn sàng sản xuất video.".into(),
                        );
                        match avatar_result {
                            Ok(rows) => {
                                let index =
                                    rows.iter()
                                        .position(|row| Some(&row.id) == previous_avatar.as_ref())
                                        .unwrap_or(0) as i32;
                                if !rows.is_empty() {
                                    let (rows, options) = avatars(rows);
                                    state.set_avatars(rows);
                                    state.set_avatar_options(options);
                                    state.set_avatar_index(index);
                                }
                            }
                            Err(error) => {
                                state.set_avatars(ModelRc::default());
                                state.set_avatar_options(ModelRc::default());
                                messages.push(message(error));
                            }
                        }
                        match voice_result {
                            Ok(rows) => {
                                let index =
                                    rows.iter()
                                        .position(|row| Some(&row.id) == previous_voice.as_ref())
                                        .unwrap_or(0) as i32;
                                if !rows.is_empty() {
                                    let (rows, options) = voices(rows);
                                    state.set_voices(rows);
                                    state.set_voice_options(options);
                                    state.set_voice_index(index);
                                } else {
                                    let (rows, options) = default_builtin_voices();
                                    state.set_voices(rows);
                                    state.set_voice_options(options);
                                    state.set_voice_index(0);
                                }
                            }
                            Err(_) => {
                                let (rows, options) = default_builtin_voices();
                                state.set_voices(rows);
                                state.set_voice_options(options);
                                state.set_voice_index(0);
                            }
                        }
                        match scene_result {
                            Ok(rows) => {
                                let (rows, options) = scenes(rows);
                                state.set_scenes(rows);
                                state.set_scene_options(options);
                                state.set_scene_index(0);
                            }
                            Err(error) => {
                                state.set_scenes(ModelRc::default());
                                state.set_scene_options(ModelRc::default());
                                messages.push(message(error));
                            }
                        }
                        match model_result {
                            Ok((_, rows)) => {
                                let index =
                                    rows.iter()
                                        .position(|row| Some(&row.id) == previous_model.as_ref())
                                        .unwrap_or(0) as i32;
                                if !rows.is_empty() {
                                    let (rows, options) = models(rows);
                                    state.set_models(rows);
                                    state.set_model_options(options);
                                    state.set_model_index(index);
                                }
                            }
                            Err(error) => {
                                state.set_models(ModelRc::default());
                                state.set_model_options(ModelRc::default());
                                messages.push(message(error));
                            }
                        }
                        match job_result {
                            Ok(rows) => {
                                state.set_jobs(jobs(rows));
                            }
                            Err(error) => messages.push(message(error)),
                        }
                        state.set_status(
                            "✅ Đã kết nối Promax Cloud Video AI · Sẵn sàng sản xuất video.".into(),
                        );
                        state.set_jobs_running(
                            state
                                .get_jobs()
                                .iter()
                                .any(|row| matches!(row.status.as_str(), "queued" | "processing")),
                        );
                        refresh_preview(&app);
                    }
                    Ok(Reply::Created(job, job_result)) => {
                        state.set_active_tab(4);
                        state.set_status(
                            format!("✅ Đã tạo video thành công · Job {}", job.id).into(),
                        );
                        apply_jobs(&state, job_result);
                    }
                    Ok(Reply::Stopped(job_result)) => {
                        state.set_status(t("studioOpenTalking.jobStopped").into());
                        state.set_stop_id("".into());
                        apply_jobs(&state, job_result);
                    }
                    Ok(Reply::Deleted(job_result)) => {
                        state.set_status(t("studioOpenTalking.jobDeleted").into());
                        state.set_delete_id("".into());
                        apply_jobs(&state, job_result);
                    }
                    Ok(Reply::Imported(path, expected_project)) => {
                        let mut imported = false;
                        let mut project_changed = false;
                        crate::host::Shell::with(|shell, app| {
                            let studio = shell.studio.borrow();
                            let Some(session) = studio.session.as_ref() else {
                                return;
                            };
                            if session.path() != expected_project.as_str() {
                                project_changed = true;
                                return;
                            }
                            drop(studio);
                            shell.studio.borrow_mut().handle(crate::panes::Msg::Media(
                                crate::panes::media_bin::MediaMsg::Import(vec![path]),
                            ));
                            shell.studio.borrow_mut().refresh_art();
                            shell.studio.borrow().publish(&app, &shell.models);
                            imported = true;
                        });
                        state.set_status(if imported {
                            t("studioOpenTalking.imported").into()
                        } else if project_changed {
                            t("studioOpenTalking.projectChanged").into()
                        } else {
                            t("studioOpenTalking.projectRequired").into()
                        });
                    }
                    Err(error) => state.set_status(message(error)),
                }
            });
        });
    if worker.is_err() {
        state.set_busy(false);
        state.set_status(t("studioOpenTalking.unavailable").into());
    }
}

fn apply_jobs(state: &StudioOpenTalking, result: Result<Vec<VideoJob>, OpenTalkingError>) {
    match result {
        Ok(rows) => {
            state.set_jobs_running(
                rows.iter()
                    .any(|row| matches!(row.status.as_str(), "queued" | "processing")),
            );
            state.set_jobs(jobs(rows));
            state.set_catalogs_loaded(true);
        }
        Err(error) => {
            state.set_jobs_running(false);
            // The mutation already succeeded. Keep the explicit success result,
            // but require an intentional refresh to reconcile history.
            state.set_catalogs_loaded(false);
            log::warn!("OpenTalking mutation succeeded but job history refresh failed: {error:?}");
        }
    }
}

fn refresh_preview(app: &App) {
    // A legacy catalogue refresh must not replace the selected Promax face.
    if !app
        .global::<crate::ui::StudioPromaxAvatars>()
        .get_live_active_id()
        .is_empty()
    {
        return;
    }
    let state = app.global::<StudioOpenTalking>();
    let id = selected(&state.get_avatars(), state.get_avatar_index())
        .map(|avatar| avatar.id.to_string())
        .unwrap_or_else(|| "mc-mia".to_string());
    let weak = app.as_weak();
    let _ = std::thread::Builder::new()
        .name("studio-avatar-preview".into())
        .spawn(move || {
            let pixels = (|| {
                if let Some(bytes) = client().ok().and_then(|c| c.avatar_preview(&id).ok()) {
                    if let Some(rgba) = decode_image_bytes(&bytes) {
                        return Some(rgba);
                    }
                }
                if let Some(bytes) = get_builtin_avatar_bytes(&id) {
                    return decode_image_bytes(bytes);
                }
                None
            })();
            let _ = weak.upgrade_in_event_loop(move |app| {
                let state = app.global::<StudioOpenTalking>();
                state.set_avatar_preview(match pixels {
                    Some(pixels) => slint::Image::from_rgba8(slint::SharedPixelBuffer::<
                        slint::Rgba8Pixel,
                    >::clone_from_slice(
                        pixels.as_raw(),
                        pixels.width(),
                        pixels.height(),
                    )),
                    None => slint::Image::default(),
                });
            });
        });
}

fn selected<T: Clone>(model: &ModelRc<T>, index: i32) -> Option<T> {
    usize::try_from(index)
        .ok()
        .and_then(|index| model.row_data(index))
}

pub(crate) fn bind(app: &App) {
    refresh_preview(app);
    let state = app.global::<StudioOpenTalking>();
    if state.get_voice_options().row_count() == 0 {
        let (rows, options) = default_builtin_voices();
        state.set_voices(rows);
        state.set_voice_options(options);
        state.set_voice_index(0);
        state.set_engine_status("ONLINE".into());
    }
    let weak = app.as_weak();
    app.global::<StudioOpenTalking>().on_refresh_jobs(move || {
        if let Some(app) = weak.upgrade() {
            execute(&app, Action::RefreshJobs);
        }
    });
    let weak = app.as_weak();
    app.global::<StudioOpenTalking>().on_refresh(move || {
        if let Some(app) = weak.upgrade() {
            execute(&app, Action::Refresh);
        }
    });
    let weak = app.as_weak();
    app.global::<StudioOpenTalking>().on_create(move || {
        let Some(app) = weak.upgrade() else { return };
        let state = app.global::<StudioOpenTalking>();
        if state.get_busy() {
            return;
        }
        let seconds = match state.get_duration_seconds().trim().parse::<u32>() {
            Ok(value) if (1..=120).contains(&value) => value,
            _ => { state.set_status("Nhập thời lượng từ 1 đến 120 giây.".into()); return; }
        };
        let Some(avatar) = selected(&state.get_avatars(), state.get_avatar_index()) else {
            return;
        };
        let Some(voice) = selected(&state.get_voices(), state.get_voice_index()) else {
            return;
        };
        let Some(model) = selected(&state.get_models(), state.get_model_index()) else {
            return;
        };
        let scene_index = state.get_scene_index();
        let scene = if scene_index <= 0 {
            None
        } else {
            selected(&state.get_scenes(), scene_index - 1).map(|row| row.id.to_string())
        };
        execute(
            &app,
            Action::Create(
                avatar.id.to_string(),
                voice.id.to_string(),
                model.id.to_string(),
                state.get_title().to_string(),
                state.get_script().to_string(),
                scene,
                seconds,
            ),
        );
    });
    let weak = app.as_weak();
    app.global::<StudioOpenTalking>().on_stop(move |id| {
        if let Some(app) = weak.upgrade() {
            if !app.global::<StudioOpenTalking>().get_busy() {
                execute(&app, Action::Stop(id.to_string()));
            }
        }
    });
    let weak = app.as_weak();
    app.global::<StudioOpenTalking>().on_delete(move |id| {
        if let Some(app) = weak.upgrade() {
            if !app.global::<StudioOpenTalking>().get_busy() {
                execute(&app, Action::Delete(id.to_string()));
            }
        }
    });
    let weak = app.as_weak();
    app.global::<StudioOpenTalking>()
        .on_import_video(move |id| {
            let Some(app) = weak.upgrade() else { return };
            if app.global::<StudioOpenTalking>().get_busy() {
                return;
            }
            let mut project_path = None;
            crate::host::Shell::with(|shell, _| {
                project_path = shell
                    .studio
                    .borrow()
                    .session
                    .as_ref()
                    .map(|session| session.path().to_owned());
            });
            let Some(project_path) = project_path else {
                if let Some(path) = ensure_job_video(&id.to_string()) {
                    #[cfg(target_os = "windows")]
                    {
                        use std::os::windows::process::CommandExt;
                        let _ = std::process::Command::new("cmd")
                            .creation_flags(0x08000000)
                            .args(["/c", "start", "", &path.to_string_lossy()])
                            .spawn();
                    }
                }
                app.global::<StudioOpenTalking>()
                    .set_status("Chưa mở dự án Concat. Đã mở video để bạn xem trước!".into());
                return;
            };
            execute(&app, Action::Import(id.to_string(), project_path));
        });
    let weak = app.as_weak();
    app.global::<StudioOpenTalking>().on_play_video(move |id| {
        let Some(app) = weak.upgrade() else { return };
        let id_str = id.to_string();
        if let Some(path) = ensure_job_video(&id_str) {
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                let _ = std::process::Command::new("cmd")
                    .creation_flags(0x08000000)
                    .args(["/c", "start", "", &path.to_string_lossy()])
                    .spawn();
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = std::process::Command::new("xdg-open").arg(&path).spawn();
            }
            app.global::<StudioOpenTalking>()
                .set_status("▶ Đang phát video trong trình phát media...".into());
        }
    });
    let weak = app.as_weak();
    app.global::<StudioOpenTalking>()
        .on_open_export_folder(move || {
            let Some(app) = weak.upgrade() else { return };
            if let Ok(dirs) = concat_host::AppDirs::locate() {
                let export_dir = dirs.data.join("opentalking-exports");
                let _ = std::fs::create_dir_all(&export_dir);
                #[cfg(target_os = "windows")]
                {
                    let _ = std::process::Command::new("explorer")
                        .arg(&export_dir)
                        .spawn();
                }
                app.global::<StudioOpenTalking>()
                    .set_status("📁 Đã mở thư mục video.".into());
            }
        });
    let weak = app.as_weak();
    app.global::<StudioOpenTalking>().on_set_tab(move |tab| {
        if let Some(app) = weak.upgrade() {
            app.global::<StudioOpenTalking>().set_active_tab(tab);
        }
    });
    let weak = app.as_weak();
    app.global::<StudioOpenTalking>()
        .on_select_avatar(move |index| {
            if let Some(app) = weak.upgrade() {
                let state = app.global::<StudioOpenTalking>();
                state.set_avatar_index(index);
                state.set_active_tab(0);
                app.global::<crate::ui::StudioPromaxAvatars>()
                    .set_live_active_id("".into());
                refresh_preview(&app);
                crate::studio_live_video::trigger_avatar_motion(&app, index);
            }
        });
    let weak = app.as_weak();
    app.global::<StudioOpenTalking>()
        .on_select_voice(move |index| {
            if let Some(app) = weak.upgrade() {
                let state = app.global::<StudioOpenTalking>();
                state.set_voice_index(index);
                state.set_active_tab(0);
            }
        });
    let weak = app.as_weak();
    app.global::<StudioOpenTalking>()
        .on_test_voice(move |index| {
            let Some(app) = weak.upgrade() else { return };
            let state = app.global::<StudioOpenTalking>();
            if state.get_busy() {
                return;
            }
            let Some(voice) = selected(&state.get_voices(), index) else {
                return;
            };
            let voice_id = voice.id.to_string();
            let mut playback = None;
            crate::host::Shell::with(|shell, _| {
                playback = Some(std::sync::Arc::clone(&shell.studio.borrow().host.playback));
            });
            let Some(playback) = playback else {
                state.set_status(t("studioOpenTalking.previewFailed").into());
                return;
            };
            state.set_busy(true);
            state.set_status("Đang phát thử giọng nói...".into());
            let weak_app = app.as_weak();
            let worker = std::thread::Builder::new()
                .name("studio-voice-preview".into())
                .spawn(move || {
                    let sample_text = "Xin chào, đây là giọng đọc thử nghiệm trong Concat Studio.";
                    let result =
                        synthesize_speech_robust(sample_text, &voice_id).and_then(|audio| {
                            crate::studio_live::play_tts(&playback, audio)
                                .map_err(|e| slint::SharedString::from(e))
                        });
                    let _ = weak_app.upgrade_in_event_loop(move |app| {
                        let state = app.global::<StudioOpenTalking>();
                        state.set_busy(false);
                        state.set_status(match result {
                            Ok(()) => "✅ Đã phát thử giọng nói thành công!".into(),
                            Err(error) => error,
                        });
                    });
                });
            if worker.is_err() {
                state.set_busy(false);
                state.set_status(t("studioOpenTalking.previewFailed").into());
            }
        });
    refresh_preview(app);
}
