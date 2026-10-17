// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native silent motion clip -> preview and clean LIVE output, one decoder.
use crate::ui::{App, StudioLive, StudioOpenTalking};
use concat_media::live_video::LiveVideo;
use slint::{ComponentHandle, Model};
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

thread_local! {
    static PLAYER: RefCell<Option<(Rc<LiveVideo>, slint::Timer)>> = const { RefCell::new(None) };
}
static SELECTION: AtomicU64 = AtomicU64::new(0);

fn select(app: &App, path: PathBuf, title: String) {
    PLAYER.with(|slot| {
        if let Some((player, _)) = slot.borrow().as_ref() {
            player.load(path);
            let live = app.global::<StudioLive>();
            live.set_video_frame(slint::Image::default());
            live.set_motion_title(title.into());
            live.set_motion_playing(false);
            live.set_motion_loading(true);
            live.set_motion_status("Đang mở video chuyển động…".into());
        }
    });
}

pub fn trigger_avatar_motion(app: &App, index: i32) {
    let live = app.global::<StudioLive>();
    let talking = app.global::<StudioOpenTalking>();
    let avatars = talking.get_avatars();
    let avatar_row = usize::try_from(index).ok().and_then(|i| avatars.row_data(i));
    let (avatar_id, avatar_label) = match avatar_row {
        Some(row) => (row.id.to_string(), row.label.to_string()),
        None => ("mc-newscaster".to_string(), "MC An An".to_string()),
    };
    live.set_motion_loading(true);
    live.set_motion_status(format!("Đang tải chuyển động cho {}…", avatar_label).into());
    let selection = SELECTION.fetch_add(1, Ordering::AcqRel) + 1;
    let weak = app.as_weak();
    let error_weak = weak.clone();
    let spawned = std::thread::Builder::new()
        .name("live-video-avatar-motion".into())
        .spawn(move || {
            let result = (|| {
                let dirs = concat_host::AppDirs::locate()
                    .map_err(|_| "Không mở được thư mục video.")?;
                let directory = dirs.data.join("live-motion");
                std::fs::create_dir_all(&directory)
                    .map_err(|_| "Không tạo được thư mục video.")?;
                let path = if let Some(vbytes) = crate::studio_opentalking::get_builtin_avatar_video_bytes(&avatar_id) {
                    let path = directory.join(format!("builtin-{}-v2.mp4", avatar_id));
                    if !path.is_file() {
                        std::fs::write(&path, vbytes)
                            .map_err(|_| "Không lưu được clip mẫu.")?;
                    }
                    path
                } else {
                    let path = directory.join(format!("builtin-{}-v2.mp4", avatar_id));
                    if !path.is_file() {
                        if let Some(bytes) = crate::studio_opentalking::get_builtin_avatar_bytes(&avatar_id) {
                            let temp_img = directory.join(format!("temp-{}.png", avatar_id));
                            let _ = std::fs::write(&temp_img, bytes);
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
                                        "-loop", "1",
                                        "-i", &temp_img.to_string_lossy(),
                                        "-c:v", "libx264",
                                        "-t", "10",
                                        "-pix_fmt", "yuv420p",
                                        "-an",
                                        &path.to_string_lossy(),
                                    ])
                                    .output();
                            }
                            let _ = std::fs::remove_file(&temp_img);
                        }
                    }
                    path
                };
                let final_path = if !path.is_file() {
                    let default_path = directory.join("builtin-newscaster-v1.mp4");
                    if !default_path.is_file() {
                        let _ = std::fs::write(&default_path, include_bytes!("../assets/avatars/newscaster.mp4"));
                    }
                    default_path
                } else {
                    path
                };
                Ok::<_, &'static str>(final_path)
            })();
            let _ = weak.upgrade_in_event_loop(move |app| {
                if SELECTION.load(Ordering::Acquire) != selection {
                    return;
                }
                app.global::<StudioLive>().set_motion_loading(false);
                match result {
                    Ok(path) => select(&app, path, format!("{} · Chuyển động lặp", avatar_label)),
                    Err(error) => app.global::<StudioLive>().set_motion_status(error.into()),
                }
            });
        });
    if spawned.is_err() {
        if let Some(app) = error_weak.upgrade() {
            app.global::<StudioLive>().set_motion_loading(false);
        }
    }
}

pub(crate) fn bind(app: &App) {
    let player = Rc::new(LiveVideo::new());
    let decoder = player.clone();
    let weak = app.as_weak();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(16),
        move || {
            let Some(app) = weak.upgrade() else { return };
            let live = app.global::<StudioLive>();
            if let Some(update) = decoder.take() {
                if let Some(frame) = update.frame {
                    live.set_video_frame(slint::Image::from_rgba8(slint::SharedPixelBuffer::<
                        slint::Rgba8Pixel,
                    >::clone_from_slice(
                        frame.pixels(),
                        frame.width(),
                        frame.height(),
                    )));
                    if !live.get_motion_playing() {
                        live.set_motion_status("Đang phát lặp video · tiếng gốc đã tắt".into());
                    }
                    live.set_motion_playing(true);
                } else {
                    live.set_video_frame(slint::Image::default());
                    live.set_motion_playing(false);
                    if let Some(error) = update.error {
                        live.set_motion_status(error.into());
                    }
                }
                live.set_motion_loading(false);
            }
            // Slint globals belong to each component, not to the whole process.
            // Explicitly mirror the frame AND overlay state into the second window.
            crate::studio_live::sync_output(&app);
        },
    );
    PLAYER.with(|slot| *slot.borrow_mut() = Some((player, timer)));

    let weak = app.as_weak();
    app.global::<StudioLive>().on_select_avatar_motion(move |index| {
        if let Some(app) = weak.upgrade() {
            trigger_avatar_motion(&app, index);
        }
    });

    let weak = app.as_weak();
    app.global::<StudioLive>().on_use_builtin_motion(move || {
        let Some(app) = weak.upgrade() else { return };
        trigger_avatar_motion(&app, 0);
    });

    let weak = app.as_weak();
    app.global::<StudioLive>().on_stop_motion(move || {
        SELECTION.fetch_add(1, Ordering::AcqRel);
        let Some(app) = weak.upgrade() else { return };
        PLAYER.with(|slot| {
            if let Some((player, _)) = slot.borrow().as_ref() {
                player.stop();
            }
        });
        let live = app.global::<StudioLive>();
        live.set_video_frame(slint::Image::default());
        live.set_motion_title("".into());
        live.set_motion_loading(false);
        live.set_motion_playing(false);
        live.set_motion_status("Đã dừng video chuyển động.".into());
        crate::studio_live::sync_output(&app);
    });

    let weak = app.as_weak();
    app.global::<StudioLive>().on_choose_motion(move || {
        let Some(app) = weak.upgrade() else { return };
        let live = app.global::<StudioLive>();
        if live.get_motion_loading() {
            return;
        }
        live.set_motion_loading(true);
        let selection = SELECTION.fetch_add(1, Ordering::AcqRel) + 1;
        let weak = app.as_weak();
        let error_weak = weak.clone();
        let spawned = std::thread::Builder::new()
            .name("live-video-choose".into())
            .spawn(move || {
                let path = rfd::FileDialog::new()
                    .set_title("Chọn video chuyển động LIVE")
                    .add_filter("Video", &["mp4", "mov", "mkv", "webm", "avi"])
                    .pick_file();
                let _ = weak.upgrade_in_event_loop(move |app| {
                    if SELECTION.load(Ordering::Acquire) != selection {
                        return;
                    }
                    app.global::<StudioLive>().set_motion_loading(false);
                    if let Some(path) = path {
                        let title = path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        select(&app, path, title);
                    }
                });
            });
        if spawned.is_err() {
            if let Some(app) = error_weak.upgrade() {
                app.global::<StudioLive>().set_motion_loading(false);
            }
        }
    });

    let weak = app.as_weak();
    app.global::<StudioOpenTalking>()
        .on_use_live_video(move |id| {
            let Some(app) = weak.upgrade() else { return };
            let live = app.global::<StudioLive>();
            if live.get_motion_loading() {
                return;
            }
            let talking = app.global::<StudioOpenTalking>();
            let Some(job) = talking
                .get_jobs()
                .iter()
                .find(|job| job.id == id && job.status == "completed")
            else {
                live.set_motion_status("Chỉ dùng video đã tạo xong cho LIVE.".into());
                return;
            };
            let title = job.title.to_string();
            let id = id.to_string();
            live.set_motion_loading(true);
            live.set_motion_status("Đang lấy video đã tạo…".into());
            let selection = SELECTION.fetch_add(1, Ordering::AcqRel) + 1;
            let weak = app.as_weak();
            let error_weak = weak.clone();
            let spawned = std::thread::Builder::new()
                .name("live-video-history".into())
                .spawn(move || {
                    let result = crate::studio_opentalking::resolve_live_video(&id);
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        if SELECTION.load(Ordering::Acquire) != selection {
                            return;
                        }
                        app.global::<StudioLive>().set_motion_loading(false);
                        match result {
                            Ok(path) => select(&app, path, title),
                            Err(error) => {
                                app.global::<StudioLive>().set_motion_status(error.into())
                            }
                        }
                    });
                });
            if spawned.is_err() {
                if let Some(app) = error_weak.upgrade() {
                    app.global::<StudioLive>().set_motion_loading(false);
                }
            }
        });
}

/// Opt-in --smoke integration: actual decode -> Slint frame -> output global.
/// No TikTok connection, AI call, microphone or provider credentials.
pub(crate) fn smoke(app: &App, path: PathBuf) -> Result<(), slint::PlatformError> {
    use std::cell::Cell;
    let frames = Rc::new(Cell::new(0u32));
    let complete = Rc::new(Cell::new(false));
    let observed = frames.clone();
    let finished = complete.clone();
    let previous = Rc::new(RefCell::new(slint::Image::default()));
    let pixel_hashes = Rc::new(RefCell::new(std::collections::HashSet::new()));
    let weak = app.as_weak();
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    let timer = slint::Timer::default();
    select(app, path, "Motion smoke fixture".into());
    app.global::<StudioLive>().invoke_toggle_output_window();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(20),
        move || {
            let Some(app) = weak.upgrade() else { return };
            let live = app.global::<StudioLive>();
            let frame = live.get_video_frame();
            if live.get_motion_playing() && frame.size().width > 0 && frame != *previous.borrow() {
                // Both components must hold the exact same latest picture.
                if crate::studio_live::output_frame().as_ref() != Some(&frame) {
                    let _ = slint::quit_event_loop();
                    return;
                }
                *previous.borrow_mut() = frame;
                if let Some(pixels) = previous.borrow().to_rgba8() {
                    use std::hash::{Hash, Hasher};
                    let mut hash = std::collections::hash_map::DefaultHasher::new();
                    for byte in pixels.as_bytes().iter().step_by(97) {
                        byte.hash(&mut hash);
                    }
                    pixel_hashes.borrow_mut().insert(hash.finish());
                }
                observed.set(observed.get() + 1);
                if observed.get() >= 12 {
                    live.invoke_stop_motion();
                }
            }
            if observed.get() >= 12
                && !live.get_motion_playing()
                && live.get_video_frame().size().width == 0
            {
                finished.set(
                    pixel_hashes.borrow().len() >= 3
                        && crate::studio_live::output_frame().is_some_and(|f| f.size().width == 0),
                );
                live.invoke_toggle_output_window();
                let _ = slint::quit_event_loop();
            } else if std::time::Instant::now() >= deadline {
                let _ = slint::quit_event_loop();
            }
        },
    );
    slint::run_event_loop_until_quit()?;
    drop(timer);
    if complete.get() {
        println!("LIVE_MOTION_SMOKE=PASS frames={}", frames.get());
        Ok(())
    } else {
        Err(slint::PlatformError::Other(
            "LIVE_MOTION_SMOKE_FAILED".into(),
        ))
    }
}
