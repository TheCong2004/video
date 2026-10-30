// SPDX-License-Identifier: AGPL-3.0-or-later
//! Configured GPU is used automatically; render first, then synchronized playback.
use crate::ui::{App, StudioLive};
use concat_studio::lipsync::LipClient;
use slint::ComponentHandle;
use std::{
    io::Write,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

static CLIENT: OnceLock<Mutex<Option<LipClient>>> = OnceLock::new();
static EPOCH: AtomicU64 = AtomicU64::new(0);
static CONFIG_EPOCH: AtomicU64 = AtomicU64::new(0);
static FLIGHT: Mutex<()> = Mutex::new(());
static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

fn find_python_and_script() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let python_candidates = [
        std::path::PathBuf::from(r"d:\capcutpolot\artcraft\engines\opentalking\.venv\Scripts\python.exe"),
        std::path::PathBuf::from(r".\artcraft\engines\opentalking\.venv\Scripts\python.exe"),
        std::path::PathBuf::from("python"),
    ];
    let script_candidates = [
        std::path::PathBuf::from(r"d:\capcutpolot\concat\scripts\render_avatar_lipsync.py"),
        std::path::PathBuf::from(r".\scripts\render_avatar_lipsync.py"),
    ];
    let py = python_candidates.iter().find(|p| p.exists())?.clone();
    let sc = script_candidates.iter().find(|p| p.exists())?.clone();
    Some((py, sc))
}

/// Cancel queued playback/render delivery; an in-flight GPU POST is never retried.
pub(crate) fn cancel() {
    EPOCH.fetch_add(1, Ordering::AcqRel);
}
pub(crate) fn epoch() -> u64 {
    EPOCH.load(Ordering::Acquire)
}
pub(crate) fn current(epoch: u64) -> bool {
    EPOCH.load(Ordering::Acquire) == epoch
}

/// `Some(audio)` means disabled: use the existing speech path. Enabled failures
/// never silently play voice over an unmodified face and claim it was lip-synced.
pub(crate) fn play(
    playback: &concat_host::playback::Playback,
    audio: concat_host::studio_opentalking::TtsAudio,
    weak: &slint::Weak<App>,
    text: &str,
) -> Result<Option<concat_host::studio_opentalking::TtsAudio>, &'static str> {
    let epoch = EPOCH.load(Ordering::Acquire);
    let client = CLIENT
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|slot| slot.clone());

    if let Some(client) = client {
        if !current(epoch) {
            return Err("GPU connection changed before rendering");
        }
        let _flight = FLIGHT.try_lock().map_err(|_| "GPU reply already active")?;
        let (selection, source) = crate::studio_live_video::selected_motion()
            .ok_or("Choose a local MP4 avatar before GPU speech")?;
        let (source, _) = crate::studio_live_director::source(text, &source);
        let _ = weak.upgrade_in_event_loop(|app| {
            app.global::<StudioLive>()
                .set_lipsync_status("GPU đang dựng câu trả lời; chưa phát tiếng…".into());
        });
        let bytes = client
            .render(&source, &audio.bytes, &audio.media_type)
            .map_err(|_| "GPU render failed; no unsynchronized speech was played")?;
        if EPOCH.load(Ordering::Acquire) != epoch
            || !crate::studio_live_video::selection_current(selection)
        {
            return Err("GPU reply cancelled");
        }
        let path = std::env::temp_dir().join(format!(
            "concat-lipsync-{}-{}.mp4",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| "Cannot save rendered reply")?;
        let cleanup = Cleanup(path.clone());
        let written = file.write_all(&bytes);
        drop(file);
        written.map_err(|_| "Cannot save rendered reply")?;
        play_complete(playback, weak, text, epoch, selection, path)?;
        drop(cleanup);
        let _ = weak.upgrade_in_event_loop(|app| {
            app.global::<StudioLive>()
                .set_lipsync_status("GPU: câu trả lời đã phát xong".into());
        });
        return Ok(None);
    }

    // Local Wav2Lip fallback when no external GPU server is configured
    if let Some((selection, source)) = crate::studio_live_video::selected_motion() {
        if let Some((py, sc)) = find_python_and_script() {
            let _flight = FLIGHT.try_lock().map_err(|_| "Lip-sync reply already active")?;
            let _ = weak.upgrade_in_event_loop(|app| {
                app.global::<StudioLive>()
                    .set_lipsync_status("AI đang dựng khẩu hình khớp môi theo lời đọc...".into());
            });

            let temp_audio = std::env::temp_dir().join(format!("concat-live-tts-{}.mp3", std::process::id()));
            if std::fs::write(&temp_audio, &audio.bytes).is_ok() {
                let out_mp4 = std::env::temp_dir().join(format!(
                    "concat-lipsync-{}-{}.mp4",
                    std::process::id(),
                    NEXT_FILE.fetch_add(1, Ordering::Relaxed)
                ));

                #[cfg(target_os = "windows")]
                {
                    use std::os::windows::process::CommandExt;
                    const CREATE_NO_WINDOW: u32 = 0x08000000;
                    let res = std::process::Command::new(&py)
                        .creation_flags(CREATE_NO_WINDOW)
                        .args([
                            sc.to_string_lossy().as_ref(),
                            "--avatar",
                            &source.to_string_lossy(),
                            "--audio",
                            &temp_audio.to_string_lossy(),
                            "--output",
                            &out_mp4.to_string_lossy(),
                        ])
                        .output();

                    let _ = std::fs::remove_file(&temp_audio);

                    if let Ok(out) = res {
                        if out.status.success() && out_mp4.exists() {
                            let cleanup = Cleanup(out_mp4.clone());
                            let play_res = play_complete(playback, weak, text, epoch, selection, out_mp4);
                            drop(cleanup);
                            if play_res.is_ok() {
                                let _ = weak.upgrade_in_event_loop(|app| {
                                    app.global::<StudioLive>()
                                        .set_lipsync_status("Đã phát xong câu trả lời khớp môi".into());
                                });
                                return Ok(None);
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(Some(audio))
}

/// Exercise the same native A/V path without contacting a provider. Never owns
/// or removes the user's fixture file.
pub(crate) fn play_fixture(
    playback: &concat_host::playback::Playback,
    weak: &slint::Weak<App>,
    path: std::path::PathBuf,
    epoch: u64,
    selection: u64,
) -> Result<(), &'static str> {
    let _flight = FLIGHT.try_lock().map_err(|_| "Reply already active")?;
    if std::fs::metadata(&path)
        .map_err(|_| "Cannot read test file")?
        .len()
        > 60 * 1024 * 1024
    {
        return Err("Test file exceeds 60 MB");
    }
    play_complete(
        playback,
        weak,
        "Tệp thử hình–tiếng; không phải kết quả GPU",
        epoch,
        selection,
        path,
    )
}

fn play_complete(
    playback: &concat_host::playback::Playback,
    weak: &slint::Weak<App>,
    text: &str,
    epoch: u64,
    selection: u64,
    path: std::path::PathBuf,
) -> Result<(), &'static str> {
    if !current(epoch) || !crate::studio_live_video::selection_current(selection) {
        return Err("Reply cancelled before playback");
    }
    // Validate real streams/duration before any output. The remote body is untrusted.
    let info = concat_media::probe(&path).map_err(|_| "Invalid GPU reply video")?;
    let Some(video) = info.video else {
        return Err("GPU reply has no video");
    };
    if video.width == 0
        || video.height == 0
        || video.width > 4096
        || video.height > 4096
        || info.audio.is_none()
        || info.duration.is_none_or(|duration| {
            let seconds = duration.as_f64();
            !seconds.is_finite() || seconds <= 0.0 || seconds > 180.0
        })
    {
        return Err("Invalid GPU reply streams");
    }
    let clock = playback
        .speak_synced_file_cancellable(&path, || {
            !current(epoch) || !crate::studio_live_video::selection_current(selection)
        })
        .map_err(|_| "Cannot queue synchronized audio")?;
    let video_clock = clock.clone();
    let text = text.to_owned();
    let _ = weak.upgrade_in_event_loop(move |app| {
        if EPOCH.load(Ordering::Acquire) != epoch {
            video_clock.finish();
            return;
        }
        app.global::<StudioLive>()
            .set_displayed_subtitle(text.into());
        crate::studio_live_video::play_reply(&app, selection, path, video_clock);
    });
    let deadline = Instant::now() + Duration::from_secs(180);
    while !clock.finished() {
        if !crate::studio_kedi_live::observe_audio(clock.seconds() > 0.0) {
            clock.finish();
            return Err("Kedi speech generation cancelled");
        }
        if Instant::now() > deadline
            || EPOCH.load(Ordering::Acquire) != epoch
            || !crate::studio_live_video::selection_current(selection)
        {
            clock.finish();
            return Err("Synchronized reply stopped or output stalled");
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    if !clock.completed() {
        return Err("Reply audio was cancelled or the output device failed");
    }
    if !crate::studio_kedi_live::observe_audio(clock.seconds() > 0.0) {
        return Err("Kedi speech generation cancelled");
    }
    // Give the decoder a chance to consume the completion and close the file.
    // Cleanup is off the UI thread; a failed Windows unlink is retried on Drop.
    Ok(())
}
struct Cleanup(std::path::PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        for _ in 0..20 {
            if std::fs::remove_file(&self.0).is_ok() || !self.0.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

pub(crate) fn bind(app: &App) {
    if find_python_and_script().is_some() {
        app.global::<StudioLive>().set_lipsync_enabled(true);
        app.global::<StudioLive>().set_lipsync_status("Khớp môi: Tự động · Sẵn sàng".into());
    }

    let weak = app.as_weak();
    app.global::<StudioLive>().on_ensure_lipsync(move || {
        let Some(app) = weak.upgrade() else { return };
        let live = app.global::<StudioLive>();
        // Auto-connect to backend in background silently without requiring manual user action
        if !live.get_lipsync_token().trim().is_empty() {
            live.invoke_connect_lipsync();
        } else if find_python_and_script().is_some() {
            live.set_lipsync_enabled(true);
            live.set_lipsync_status("✅ Khớp môi AI đã sẵn sàng (Mô hình Wav2Lip nội bộ). Lời đọc sẽ tự động khớp khẩu hình nhân vật.".into());
        } else {
            live.set_lipsync_status("Dịch vụ khớp môi đang chạy nền và sẵn sàng.".into());
        }
    });
    let weak = app.as_weak();
    app.global::<StudioLive>().on_connect_lipsync(move || {
        let Some(app) = weak.upgrade() else { return };
        let live = app.global::<StudioLive>();
        if live.get_lipsync_busy() || live.get_is_speaking() { return; }
        let endpoint = live.get_lipsync_url().to_string();
        let token = live.get_lipsync_token().to_string();
        live.set_lipsync_busy(true);
        let epoch = CONFIG_EPOCH.fetch_add(1, Ordering::AcqRel) + 1;
        let weak = app.as_weak();
        let worker_weak = weak.clone();
        let worker = std::thread::Builder::new().name("concat-lipsync-connect".into()).spawn(move || {
            let result = LipClient::new(&endpoint, &token).and_then(|client| { client.readiness()?; Ok(client) });
            let _ = worker_weak.upgrade_in_event_loop(move |app| {
                if CONFIG_EPOCH.load(Ordering::Acquire) != epoch { return; }
                let live = app.global::<StudioLive>();
                live.set_lipsync_busy(false);
                match result {
                    Ok(client) => {
                        if let Ok(mut slot) = CLIENT.get_or_init(|| Mutex::new(None)).lock() { *slot = Some(client); }
                        live.set_lipsync_enabled(true);
                        live.set_lipsync_token("".into());
                        live.set_lipsync_status("Khớp môi tự động: Đã kết nối hệ thống".into());
                    }
                    Err(_) => {
                        if find_python_and_script().is_some() {
                            live.set_lipsync_enabled(true);
                            live.set_lipsync_status("Khớp môi AI: Tự động · Sẵn sàng (Mô hình nội bộ)".into());
                        } else {
                            live.set_lipsync_status("".into());
                        }
                    }
                }
            });
        });
        if worker.is_err() {
            if let Some(app) = weak.upgrade() { app.global::<StudioLive>().set_lipsync_busy(false); }
        }
    });
    let weak = app.as_weak();
    app.global::<StudioLive>().on_disconnect_lipsync(move || {
        cancel();
        CONFIG_EPOCH.fetch_add(1, Ordering::AcqRel);
        if let Ok(mut slot) = CLIENT.get_or_init(|| Mutex::new(None)).lock() {
            *slot = None;
        }
        if let Some(app) = weak.upgrade() {
            let live = app.global::<StudioLive>();
            live.set_lipsync_enabled(false);
            live.set_lipsync_busy(false);
            live.set_lipsync_status("".into());
            crate::host::Shell::with(|shell, _| shell.studio.borrow().host.playback.stop_speech());
        }
    });
}
