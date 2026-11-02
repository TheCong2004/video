// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native Chánh Điện controls and a persistent view of the real LIVE overlay.
use crate::ui::{App, StudioLive, TempleViewerRow};
use concat_studio::temple_live::{self, Endpoint, Event};
use serde_json::{Value, json};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

pub(crate) struct Controller {
    _timer: slint::Timer,
    stop: Arc<AtomicBool>,
}
impl Drop for Controller {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        preview::shutdown();
    }
}

/// Release our native child while its parent window/message loop still exists.
pub(crate) fn shutdown() {
    preview::shutdown();
}

fn timestamp() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!(
        "{:02}:{:02}:{:02}",
        (seconds / 3600 + 7) % 24,
        seconds / 60 % 60,
        seconds % 60
    )
}

fn append_log(app: &App, line: &str, deduplicate: bool) {
    let live = app.global::<StudioLive>();
    let rows = live.get_cd_logs();
    let Some(rows) = rows.as_any().downcast_ref::<VecModel<SharedString>>() else {
        return;
    };
    let line: SharedString = line.chars().take(1800).collect::<String>().into();
    if deduplicate && rows.iter().any(|existing| existing == line) {
        return;
    }
    if rows.row_count() >= 500 {
        rows.remove(0);
    }
    rows.push(line);
    live.set_cd_log_revision(live.get_cd_log_revision().wrapping_add(1));
}

fn local_log(app: &App, text: &str) {
    append_log(app, &format!("[{}] {text}", timestamp()), false);
}

fn viewer(value: &Value) -> TempleViewerRow {
    TempleViewerRow {
        id: value["id"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(256)
            .collect::<String>()
            .into(),
        name: value["name"]
            .as_str()
            .unwrap_or("Đạo hữu")
            .chars()
            .take(120)
            .collect::<String>()
            .into(),
        skin: value["skin"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(30)
            .collect::<String>()
            .into(),
        coins: value["coins"]
            .as_i64()
            .unwrap_or(0)
            .clamp(0, i32::MAX as i64) as i32,
        simulated: value["demo"].as_bool().unwrap_or(false),
    }
}

fn settings(app: &App, value: &Value) {
    let live = app.global::<StudioLive>();
    if live.get_cd_settings_pending() {
        return;
    }
    if let Some(on) = value["mo"].as_bool() {
        live.set_cd_mo_enabled(on);
    }
    if let Some(on) = value["music"].as_bool() {
        live.set_cd_music_enabled(on);
    }
    if let Some(on) = value["autoChant"].as_bool() {
        live.set_cd_chant_enabled(on);
    }
    if let Some(bpm) = value["moBpm"].as_i64() {
        live.set_cd_bpm(bpm.clamp(20, 100) as i32);
    }
    if let Some(voice) = value["voice"].as_str() {
        live.set_cd_voice_index(i32::from(voice == "vi-VN-HoaiMyNeural"));
    }
    if let Some(seats) = value["seatMode"].as_str() {
        live.set_cd_seats_index(match seats {
            "24" => 1,
            "36" => 2,
            "64" => 3,
            "100" => 4,
            _ => 0,
        });
    }
}

fn tiktok(app: &App, value: &Value) {
    let live = app.global::<StudioLive>();
    let status = value["status"].as_str().unwrap_or("idle");
    live.set_cd_tt_state(status.into());
    let label = match status {
        "connected" => format!(
            "TikTok: @{}",
            value["uniqueId"]
                .as_str()
                .unwrap_or("")
                .trim_start_matches('@')
        ),
        "connecting" => "TikTok: Đang kết nối…".to_string(),
        _ if value["error"].as_str().is_some_and(|s| !s.is_empty()) => {
            "TikTok: Lỗi kết nối · xem nhật ký".to_string()
        }
        _ => "TikTok: Chưa kết nối".to_string(),
    };
    live.set_cd_tiktok_status(label.into());
}

fn apply_event(app: &App, event: Event) {
    let live = app.global::<StudioLive>();
    match event {
        Event::Connected => {
            live.set_cd_connected(false);
            live.set_cd_server_status("Server: Đang đồng bộ…".into());
            local_log(app, "Đã kết nối WebSocket điều khiển Chánh Điện.");
        }
        Event::Disconnected => {
            if live.get_cd_connected() {
                local_log(
                    app,
                    "Mất kết nối máy chủ LIVE; đang nối lại với thời gian chờ tăng dần.",
                );
            } else if live.get_cd_server_status().contains("Đang kết nối") {
                local_log(
                    app,
                    "Không kết nối được máy chủ LIVE. Kiểm tra máy chủ hoặc CONCAT_TEMPLE_URL.",
                );
            }
            live.set_cd_connected(false);
            live.set_cd_server_status("Server: Đang nối lại…".into());
        }
        Event::WriteFailed => local_log(
            app,
            "Không gửi được thao tác LIVE. Yêu cầu chưa xác định kết quả sẽ không được gửi lại tự động.",
        ),
        Event::Message(value) => {
            let kind = value["t"].as_str().unwrap_or("");
            match kind {
                "state" => {
                    live.set_cd_connected(true);
                    live.set_cd_server_status("Server: Kết nối tốt".into());
                    settings(app, &value["settings"]);
                    tiktok(app, &value["tiktok"]);
                    if let Some(on) = value["bots"]["on"].as_bool() {
                        live.set_cd_bot_running(on);
                    }
                    let people = value["viewers"]
                        .as_array()
                        .map(|list| list.iter().take(100).map(viewer).collect::<Vec<_>>())
                        .unwrap_or_default();
                    live.set_cd_people(ModelRc::new(VecModel::from(people)));
                }
                "settings" => settings(app, &value["settings"]),
                "tiktok" => tiktok(
                    app,
                    if value["state"].is_object() {
                        &value["state"]
                    } else {
                        &value
                    },
                ),
                "bots" => {
                    if let Some(on) = value["on"].as_bool() {
                        live.set_cd_bot_running(on);
                    }
                }
                "logs" => {
                    if let Some(lines) = value["lines"].as_array() {
                        for line in lines
                            .iter()
                            .rev()
                            .take(500)
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                        {
                            if let Some(line) = line.as_str() {
                                append_log(app, line, true);
                            }
                        }
                    }
                }
                "log" => {
                    if let Some(line) = value["line"].as_str() {
                        append_log(app, line, false);
                    }
                }
                "join" | "leave" | "skin" | "gift" => {
                    let model = live.get_cd_people();
                    if let Some(rows) = model.as_any().downcast_ref::<VecModel<TempleViewerRow>>() {
                        if kind == "join" {
                            let person = viewer(&value["v"]);
                            if let Some(index) = rows.iter().position(|row| row.id == person.id) {
                                rows.set_row_data(index, person);
                            } else if rows.row_count() < 100 {
                                rows.push(person);
                            }
                        } else if let Some(index) = rows
                            .iter()
                            .position(|row| row.id.as_str() == value["id"].as_str().unwrap_or(""))
                        {
                            if kind == "leave" {
                                rows.remove(index);
                            } else if let Some(mut row) = rows.row_data(index) {
                                if kind == "skin" {
                                    row.skin = value["skin"].as_str().unwrap_or("").into();
                                }
                                if kind == "gift" {
                                    row.coins = value["total"]
                                        .as_i64()
                                        .unwrap_or(0)
                                        .clamp(0, i32::MAX as i64)
                                        as i32;
                                }
                                rows.set_row_data(index, row);
                            }
                        }
                    }
                }
                _ => {}
            }
            if let Some(capacity) = value["cap"].as_i64() {
                live.set_cd_capacity(capacity.clamp(1, 100) as i32);
            }
            live.set_cd_viewers_count(
                format!(
                    "{}/{} trong cảnh",
                    live.get_cd_people().row_count(),
                    live.get_cd_capacity()
                )
                .into(),
            );
        }
    }
}

fn open_url(url: String) {
    std::thread::spawn(move || {
        #[cfg(windows)]
        {
            use windows_sys::Win32::UI::Shell::ShellExecuteW;
            let operation: Vec<u16> = "open\0".encode_utf16().collect();
            let url: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
            unsafe {
                ShellExecuteW(
                    std::ptr::null_mut(),
                    operation.as_ptr(),
                    url.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    1,
                );
            }
        }
        #[cfg(not(windows))]
        {
            let _ = std::process::Command::new(if cfg!(target_os = "macos") {
                "open"
            } else {
                "xdg-open"
            })
            .arg(url)
            .spawn();
        }
    });
}

pub(crate) fn bind(app: &App) -> Controller {
    let configured =
        std::env::var("CONCAT_TEMPLE_URL").unwrap_or_else(|_| "http://127.0.0.1:3088".to_string());
    let endpoint = Endpoint::parse(&configured);
    let live = app.global::<StudioLive>();
    live.set_cd_logs(ModelRc::new(VecModel::<SharedString>::default()));
    live.set_cd_people(ModelRc::new(VecModel::<TempleViewerRow>::default()));
    let (sender, receiver) = mpsc::sync_channel::<Value>(64);
    // A bounded UI channel prevents a burst of clicks from growing memory.
    let receiver = Rc::new(std::cell::RefCell::new(Some(receiver)));
    let stop = Arc::new(AtomicBool::new(false));
    let due = Rc::new(Cell::new(None::<Instant>));

    let endpoint = match endpoint {
        Ok(endpoint) => {
            live.set_cd_origin(endpoint.origin.clone().into());
            Some(endpoint)
        }
        Err(message) => {
            live.set_cd_server_status("Server: Cấu hình không hợp lệ".into());
            local_log(app, message);
            None
        }
    };
    let weak = app.as_weak();
    let endpoint_start = endpoint.clone();
    let stop_worker = Arc::clone(&stop);
    let pending = Rc::clone(&receiver);
    live.on_cd_ensure(move || {
        let Some(endpoint) = endpoint_start.clone() else {
            return;
        };
        let Some(receiver) = pending.borrow_mut().take() else {
            return;
        };
        let weak = weak.clone();
        let stop = Arc::clone(&stop_worker);
        if let Some(app) = weak.upgrade() {
            app.global::<StudioLive>()
                .set_cd_server_status("Server: Đang kết nối…".into());
        }
        std::thread::spawn(move || {
            // Bridge the bounded channel to the protocol worker without extra
            // threads: std's Receiver is the same for sync/unbounded senders.
            let startup = concat_studio::temple_runtime::ensure(&endpoint);
            let _ = weak.upgrade_in_event_loop(move |app| match startup {
                Ok(true) => local_log(&app, "Đã khởi động runtime Chánh Điện đi kèm Concat."),
                Err(message) => local_log(&app, message),
                _ => {}
            });
            temple_live::run(endpoint, receiver, stop, move |event| {
                weak.upgrade_in_event_loop(move |app| apply_event(&app, event))
                    .is_ok()
            });
        });
    });
    let settings_due = Rc::clone(&due);
    let weak = app.as_weak();
    live.on_cd_settings(move || {
        settings_due.set(Some(Instant::now() + Duration::from_millis(180)));
        if let Some(app) = weak.upgrade() {
            app.global::<StudioLive>().set_cd_settings_pending(true);
        }
    });
    let browser = endpoint.as_ref().map(|e| e.studio.clone());
    live.on_open_chanh_dien(move || {
        if let Some(url) = &browser {
            open_url(url.clone());
        }
    });
    let browser = endpoint.as_ref().map(|e| e.overlay.clone());
    live.on_open_chanh_dien_overlay(move || {
        if let Some(url) = &browser {
            open_url(url.clone());
        }
    });

    let weak = app.as_weak();
    let actions = sender.clone();
    let overlay_url = endpoint.as_ref().map(|e| e.overlay.clone());
    let studio_url = endpoint.as_ref().map(|e| e.studio.clone());
    live.on_cd_action(move |action, name, text, n| {
        let Some(app) = weak.upgrade() else {
            return;
        };
        if action == "open-popup" || action == "open-studio" {
            let target = if action == "open-popup" {
                &overlay_url
            } else {
                &studio_url
            };
            if let Some(url) = target {
                open_url(url.clone());
            }
            return;
        }
        if action == "audio" {
            preview::audio(&app, n > 0);
            return;
        }
        if action == "copy-obs" {
            let Some(url) = overlay_url.clone() else {
                return;
            };
            let weak = app.as_weak();
            std::thread::spawn(move || {
                #[cfg(windows)]
                let copied = {
                    use std::{
                        io::Write,
                        os::windows::process::CommandExt,
                        process::{Command, Stdio},
                    };
                    let result = (|| {
                        let mut child = Command::new("powershell.exe")
                            .args([
                                "-NoProfile",
                                "-NonInteractive",
                                "-Command",
                                "$value=[Console]::In.ReadToEnd(); Set-Clipboard -Value $value",
                            ])
                            .creation_flags(0x08000000)
                            .stdin(Stdio::piped())
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .spawn()?;
                        if let Some(mut input) = child.stdin.take() {
                            input.write_all(url.as_bytes())?;
                        }
                        child.wait().map(|status| status.success())
                    })();
                    result.unwrap_or(false)
                };
                #[cfg(not(windows))]
                let copied = false;
                let _ = weak.upgrade_in_event_loop(move |app| {
                    local_log(
                        &app,
                        if copied {
                            "Đã copy URL overlay cho OBS Browser Source · 1080×1920."
                        } else {
                            "Chưa copy được URL OBS vào clipboard."
                        },
                    )
                });
            });
            return;
        }
        let live = app.global::<StudioLive>();
        if !live.get_cd_connected() {
            local_log(&app, "Máy chủ chưa kết nối; thao tác chưa được gửi.");
            return;
        }
        if action == "connect"
            && matches!(
                live.get_cd_tt_state().as_str(),
                "connecting" | "connected" | "disconnecting"
            )
        {
            return;
        }
        if action == "disconnect"
            && !matches!(live.get_cd_tt_state().as_str(), "connecting" | "connected")
        {
            return;
        }
        if let Some(command) =
            temple_live::command(action.as_str(), name.as_str(), text.as_str(), n)
        {
            if actions.try_send(command).is_err() {
                local_log(&app, "Hàng đợi LIVE đang bận; thao tác chưa được gửi.");
            } else if action == "connect" {
                live.set_cd_tt_state("connecting".into());
                live.set_cd_tiktok_status("TikTok: Đang kết nối…".into());
            } else if action == "disconnect" {
                live.set_cd_tt_state("disconnecting".into());
                live.set_cd_tiktok_status("TikTok: Đang ngắt…".into());
            }
        }
    });
    let weak = app.as_weak();
    live.on_cd_hide_preview(move || preview::hide());
    live.on_cd_reload_preview(move || {
        if let Some(app) = weak.upgrade() {
            preview::reload(&app);
        }
    });
    let weak = app.as_weak();
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(80), move || {
        let Some(app) = weak.upgrade() else { return; };
        let live = app.global::<StudioLive>();
        if due.get().is_some_and(|time| Instant::now() >= time) {
            due.set(None);
            live.set_cd_settings_pending(false);
            let payload = json!({"t":"ctl", "cmd":"settings", "settings":{
                "mo":live.get_cd_mo_enabled(), "music":live.get_cd_music_enabled(), "autoChant":live.get_cd_chant_enabled(),
                "moBpm":live.get_cd_bpm(), "voice":if live.get_cd_voice_index() == 1 { "vi-VN-HoaiMyNeural" } else { "vi-VN-NamMinhNeural" },
                "seatMode":match live.get_cd_seats_index() {1=>"24",2=>"36",3=>"64",4=>"100",_=>"auto"}
            }});
            if !live.get_cd_connected() || sender.try_send(payload).is_err() { local_log(&app, "Chưa lưu được thiết lập âm thanh vào máy chủ LIVE."); }
        }
        if let Some(endpoint) = &endpoint { preview::tick(&app, endpoint); }
    });
    Controller {
        _timer: timer,
        stop,
    }
}

#[cfg(windows)]
mod preview {
    use super::*;
    use slint::winit_030::WinitWindowAccessor;
    use std::cell::{Cell, RefCell};
    use wry::{
        Rect, WebContext, WebView, WebViewBuilder,
        dpi::{PhysicalPosition, PhysicalSize},
    };
    #[derive(Default)]
    struct Host {
        view: Option<WebView>,
        context: Option<WebContext>,
        visible: bool,
        attempted: bool,
        bounds: Option<[i32; 4]>,
        started: Option<Instant>,
    }
    thread_local! { static HOST: RefCell<Host> = RefCell::new(Host::default()); static BUSY: Cell<bool> = const { Cell::new(false) }; }
    pub(super) fn shutdown() {
        BUSY.set(true);
        let (view, context) = HOST.with_borrow_mut(|host| {
            host.visible = false;
            host.attempted = true;
            host.started = None;
            (host.view.take(), host.context.take())
        });
        if let Some(view) = &view {
            let _ = view.set_visible(false);
        }
        drop(view);
        drop(context);
        BUSY.set(false);
    }
    const SCRIPT: &str = r#"
        window.__concatTempleAudio = async function(on) {
            try {
                if (on && typeof AC !== 'undefined' && !AC) document.getElementById('unmute')?.click();
                if (typeof AC !== 'undefined' && AC && on) await AC.resume();
                if (typeof master !== 'undefined' && master) master.gain.value = on ? .9 : 0;
                const active = !!(on && typeof AC !== 'undefined' && AC && AC.state === 'running');
                window.ipc.postMessage(JSON.stringify({audio:active,failed:!!(on&&!active)}));
            } catch (_) { window.ipc.postMessage(JSON.stringify({audio:false,failed:true})); }
        };
        document.addEventListener('click', e => {
            if(e.target.closest('#unmute')) queueMicrotask(() => window.__concatTempleAudio(true));
        }, true);
        document.addEventListener('DOMContentLoaded', () => {
            const c = document.querySelector('#c');
            window.ipc.postMessage(JSON.stringify({ready:!!(c && c.width===1080 && c.height===1920)}));
        }, {once:true});
    "#;
    pub(super) fn hide() {
        if BUSY.get() {
            return;
        }
        HOST.with_borrow_mut(|host| {
            if let Some(view) = &host.view {
                let _ = view.set_visible(false);
            }
            host.visible = false;
        });
    }
    pub(super) fn audio(app: &App, on: bool) {
        HOST.with_borrow(|host| {
            if let Some(view) = &host.view {
                let _ = view.evaluate_script(if on {
                    "window.__concatTempleAudio?.(true)"
                } else {
                    "window.__concatTempleAudio?.(false)"
                });
            } else {
                local_log(app, "Mở preview overlay trước khi bật âm thanh.");
            }
        });
    }
    pub(super) fn reload(app: &App) {
        HOST.with_borrow_mut(|host| {
            if let Some(view) = &host.view {
                let _ = view.reload();
            } else {
                host.attempted = false;
            }
            host.started = Some(Instant::now());
        });
        app.global::<StudioLive>().set_cd_preview_loaded(false);
        app.global::<StudioLive>()
            .set_cd_preview_status("Đang tải lại overlay LIVE…".into());
        app.global::<StudioLive>().set_cd_audio_enabled(false);
    }
    pub(super) fn tick(app: &App, endpoint: &Endpoint) {
        if BUSY.replace(true) {
            return;
        }
        struct Guard;
        impl Drop for Guard {
            fn drop(&mut self) {
                BUSY.set(false);
            }
        }
        let _guard = Guard;
        let live = app.global::<StudioLive>();
        let Some((minimized, size)) = app.window().with_winit_window(|window| {
            (window.is_minimized().unwrap_or(false), window.inner_size())
        }) else {
            return;
        };
        let frame = [
            live.get_cd_preview_x(),
            live.get_cd_preview_y(),
            live.get_cd_preview_width(),
            live.get_cd_preview_height(),
        ];
        // The Slint QA scale override may differ from the Windows DPI scale.
        // Position the child using Slint's physical pixels in both cases.
        let physical = frame.map(|n| n * app.window().scale_factor());
        let bounds_key = physical.map(|n| n.round() as i32);
        let visible = live.get_cd_page_active()
            && live.get_cd_preview_allowed()
            && live.get_cd_preview_inline()
            && app.get_web_host_allowed()
            && !minimized
            && frame[0] >= 0.0
            && frame[1] >= 0.0
            && frame[2] > 20.0
            && frame[3] > 20.0
            && physical[0] + physical[2] <= size.width as f32 + 2.0
            && physical[1] + physical[3] <= size.height as f32 + 2.0;
        HOST.with_borrow_mut(|host| {
            if !visible { if host.visible { if let Some(view) = &host.view { let _ = view.set_visible(false); } host.visible = false; } return; }
            let bounds = Rect { position: PhysicalPosition::new(bounds_key[0], bounds_key[1]).into(), size: PhysicalSize::new(bounds_key[2].max(1) as u32, bounds_key[3].max(1) as u32).into() };
            if host.view.is_none() && !host.attempted {
                // A cold launch starts Node on the worker. Wait for its real
                // state before navigating, avoiding a cached connection error.
                if !live.get_cd_connected() { return; }
                host.attempted = true;
                host.started = Some(Instant::now());
                host.context = Some(WebContext::new(None));
                let allowed = endpoint.overlay.clone();
                let weak_audio = app.as_weak();
                let builder = WebViewBuilder::new_with_web_context(host.context.as_mut().unwrap())
                    .with_url(&endpoint.overlay).with_bounds(bounds).with_visible(false).with_focused(false)
                    .with_background_color((8, 13, 22, 255)).with_autoplay(true).with_devtools(false)
                    .with_general_autofill_enabled(false).with_initialization_script(SCRIPT)
                    .with_navigation_handler(move |url| url == allowed)
                    .with_ipc_handler(move |request: wry::http::Request<String>| {
                        if request.body().len() > 128 { return; }
                        if let Ok(value) = serde_json::from_str::<Value>(request.body()) {
                            if let Some(active) = value["audio"].as_bool() { let _ = weak_audio.upgrade_in_event_loop(move |app| app.global::<StudioLive>().set_cd_audio_enabled(active)); }
                            if value["failed"].as_bool() == Some(true) { let _ = weak_audio.upgrade_in_event_loop(|app| local_log(&app, "Chưa bật được âm preview. Kiểm tra thiết bị âm thanh hoặc bật âm trực tiếp trong overlay.")); }
                            if let Some(ready) = value["ready"].as_bool() { let _ = weak_audio.upgrade_in_event_loop(move |app| {
                                let live = app.global::<StudioLive>();
                                live.set_cd_preview_loaded(ready);
                                if !ready { live.set_cd_preview_status("Máy chủ không trả về overlay 1080×1920. Kiểm tra cấu hình rồi tải lại.".into()); }
                            }); }
                        }
                    });
                match app.window().with_winit_window(|window| builder.build_as_child(window)) {
                    Some(Ok(view)) => host.view = Some(view),
                    _ => { live.set_cd_preview_status("Chưa mở được overlay. Kiểm tra WebView2 rồi tải lại preview.".into()); host.context = None; return; }
                }
            }
            if live.get_cd_preview_loaded() { host.started = None; }
            if host.started.is_some_and(|time| time.elapsed() > Duration::from_secs(15)) {
                host.started = None;
                live.set_cd_preview_status("Không tải được overlay. Kiểm tra máy chủ LIVE rồi bấm Tải lại preview.".into());
            }
            if let Some(view) = &host.view {
                if host.bounds != Some(bounds_key) { let _ = view.set_bounds(bounds); host.bounds = Some(bounds_key); }
                if !host.visible && live.get_cd_preview_loaded() { let _ = view.set_visible(true); host.visible = true; }
            }
        });
    }
}

#[cfg(not(windows))]
mod preview {
    use super::*;
    pub(super) fn shutdown() {}
    pub(super) fn hide() {}
    pub(super) fn audio(app: &App, _: bool) {
        local_log(app, "Mở overlay trên trình duyệt để nghe âm thanh.");
    }
    pub(super) fn reload(_: &App) {}
    pub(super) fn tick(app: &App, _: &Endpoint) {
        app.global::<StudioLive>()
            .set_cd_preview_status("Mở overlay LIVE trên trình duyệt bằng nút Cửa Sổ Phát.".into());
    }
}
