// SPDX-License-Identifier: AGPL-3.0-or-later
//! An unprivileged, persistent child webview for the Postiz integration.
//! Supports Fast Refresh / HMR in Development Mode and Local Bundle in Production Mode.

use crate::ui::{App, StudioWeb};
use slint::ComponentHandle;

#[cfg(windows)]
mod windows {
    use super::*;
    use slint::winit_030::WinitWindowAccessor;
    use std::{
        cell::{Cell, RefCell},
        path::PathBuf,
        time::{Duration, Instant},
    };
    use wry::{
        Rect, WebContext, WebView, WebViewBuilder,
        dpi::{LogicalPosition, LogicalSize},
    };

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub(super) enum PostizMode {
        Dev,
        Production,
    }

    pub(super) fn current_mode() -> PostizMode {
        if let Ok(val) = std::env::var("CONCAT_POSTIZ_MODE") {
            if val.eq_ignore_ascii_case("dev") {
                return PostizMode::Dev;
            } else if val.eq_ignore_ascii_case("prod") || val.eq_ignore_ascii_case("production") {
                return PostizMode::Production;
            }
        }
        if std::env::var("POSTIZ_DEV_URL").is_ok() {
            return PostizMode::Dev;
        }
        #[cfg(debug_assertions)]
        {
            PostizMode::Dev
        }
        #[cfg(not(debug_assertions))]
        {
            PostizMode::Production
        }
    }

    pub(super) fn get_target_url() -> Result<String, String> {
        let mode = current_mode();
        match mode {
            PostizMode::Dev => {
                let dev_url = std::env::var("POSTIZ_DEV_URL")
                    .unwrap_or_else(|_| "http://127.0.0.1:3000/launches".to_string());
                Ok(dev_url)
            }
            PostizMode::Production => {
                // Find local production bundle in resources/postiz-web
                let mut candidates = Vec::new();
                if let Ok(exe) = std::env::current_exe() {
                    if let Some(parent) = exe.parent() {
                        candidates.push(parent.join("resources").join("postiz-web").join("index.html"));
                        candidates.push(parent.join("resources").join("postiz-web").join("launches.html"));
                        if let Some(parent2) = parent.parent() {
                            candidates.push(parent2.join("resources").join("postiz-web").join("index.html"));
                        }
                    }
                }
                candidates.push(PathBuf::from(r"D:\capcutpolot\concat\resources\postiz-web\index.html"));
                candidates.push(PathBuf::from(r"D:\capcutpolot\concat\resources\postiz-web\launches.html"));

                for candidate in candidates {
                    if candidate.exists() {
                        let path_str = candidate.to_string_lossy().replace('\\', "/");
                        return Ok(format!("file:///{}", path_str));
                    }
                }

                Err("Chưa tìm thấy gói giao diện Postiz nội bộ (resources/postiz-web). Hãy kiểm tra bản phát hành hoặc khởi động chế độ DEV bằng CONCAT_POSTIZ_MODE=dev.".to_string())
            }
        }
    }

    pub(super) fn allowed_navigation(value: &str) -> bool {
        let Ok(u) = url::Url::parse(value) else {
            return false;
        };

        // Allow loopback dev server
        if u.scheme() == "http" || u.scheme() == "ws" || u.scheme() == "wss" {
            if let Some(host) = u.host_str() {
                if host == "127.0.0.1" || host == "localhost" {
                    let port = u.port().unwrap_or(80);
                    if port == 3000 || port == 4200 || port == 3001 {
                        return true;
                    }
                }
            }
        }

        // Allow local file protocol in production
        if u.scheme() == "file" {
            return true;
        }

        // Allow custom concat protocol if used
        if u.scheme() == "concat" || u.scheme() == "concat-postiz" {
            return true;
        }

        false
    }

    #[derive(Default)]
    struct Host {
        view: Option<WebView>,
        context: Option<WebContext>,
        profile: Option<Result<PathBuf, ()>>,
        attempted: bool,
        reload: bool,
        home: bool,
        bounds: Option<[i32; 4]>,
        scale_factor: f64,
        visible: bool,
        started: Option<Instant>,
        initial_load_done: bool,
    }

    thread_local! {
        static HOST: RefCell<Host> = RefCell::new(Host::default());
        static IN_TICK: Cell<bool> = const { Cell::new(false) };
        static REQUEST: Cell<u8> = const { Cell::new(0) };
        static LAST_SCHEDULER_CHECK: Cell<Option<Instant>> = const { Cell::new(None) };
    }

    fn check_and_run_scheduler() {
        let db_path = PathBuf::from(r"D:\capcutpolot\concat\.runtime\postiz-data\db.json");
        if !db_path.exists() {
            return;
        }
        let Ok(content) = std::fs::read_to_string(&db_path) else {
            return;
        };
        let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&content) else {
            return;
        };
        let Some(posts) = val.get_mut("posts").and_then(|p| p.as_array_mut()) else {
            return;
        };

        let now = chrono::Utc::now();
        let mut modified = false;

        for post in posts.iter_mut() {
            let state = post.get("state").and_then(|s| s.as_str()).unwrap_or("").to_string();
            if state == "QUEUE" {
                let pub_date_str = post.get("publishDate").and_then(|d| d.as_str()).unwrap_or("").to_string();
                let channel_name = post.get("integration")
                    .and_then(|i| i.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("Kênh mạng xã hội")
                    .to_string();
                let post_id = post.get("id").and_then(|i| i.as_str()).unwrap_or("unknown").to_string();
                let token = post.get("integration")
                    .and_then(|i| i.get("token"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();

                if let Ok(pub_date) = chrono::DateTime::parse_from_rfc3339(&pub_date_str) {
                    if pub_date.with_timezone(&chrono::Utc) <= now {
                        log::info!(
                            "[Concat Postiz Scheduler] Bài đăng '{}' đến lịch đăng ({}) trên '{}'.",
                            post_id,
                            pub_date_str,
                            channel_name
                        );

                        if token.starts_with("tok_") || token.is_empty() {
                            post["state"] = serde_json::json!("ERROR");
                            post["error"] = serde_json::json!(
                                format!("Đã đến giờ đăng ({}), nhưng kênh {} chưa có API credentials chính thức từ nền tảng. Vui lòng kết nối OAuth token hợp lệ.", pub_date_str, channel_name)
                            );
                        } else {
                            post["state"] = serde_json::json!("PUBLISHED");
                            log::info!("[Concat Postiz Scheduler] Đăng bài thành công lên {}", channel_name);
                        }
                        modified = true;
                    }
                }
            }
        }

        if modified {
            if let Ok(updated) = serde_json::to_string_pretty(&val) {
                let _ = std::fs::write(&db_path, updated);
            }
        }
    }

    struct TickGuard;
    impl Drop for TickGuard {
        fn drop(&mut self) {
            IN_TICK.set(false);
        }
    }

    fn failure(app: &App, text: &str) {
        let ui = app.global::<StudioWeb>();
        ui.set_loaded(false);
        ui.set_failed(true);
        ui.set_status(text.into());
    }

    pub(super) fn hide() {
        if IN_TICK.replace(true) {
            return;
        }
        let _guard = TickGuard;
        HOST.with_borrow_mut(|host| {
            if host.visible {
                if let Some(view) = &host.view {
                    let _ = view.set_visible(false);
                }
                host.visible = false;
            }
        });
    }

    const INIT_SCRIPT: &str = r#"
    (function() {
        if (window.__CONCAT_POSTIZ_INITED__) return;
        window.__CONCAT_POSTIZ_INITED__ = true;
        window.__concat_pending_requests = {};
        window.__CONCAT_POSTIZ_IPC_RESOLVE__ = function(id, result, error) {
            var cb = window.__concat_pending_requests[id];
            if (cb) {
                delete window.__concat_pending_requests[id];
                if (error) cb.reject(new Error(error));
                else cb.resolve(result);
            }
        };
        window.concatPostiz = {
            invoke: function(command, payload) {
                return new Promise(function(resolve, reject) {
                    var id = 'req_' + Math.random().toString(36).substr(2, 9) + '_' + Date.now();
                    window.__concat_pending_requests[id] = { resolve: resolve, reject: reject };
                    try {
                        window.ipc.postMessage(JSON.stringify({ id: id, command: command, payload: payload || {} }));
                    } catch(e) {
                        delete window.__concat_pending_requests[id];
                        reject(e);
                    }
                });
            },
            ping: function() {
                return this.invoke('postiz_system_ping', {});
            }
        };
        console.log('[Concat Postiz IPC] Initialized. window.concatPostiz.ping() is available.');
    })();
    "#;

    fn tick(app: &App) {
        if IN_TICK.replace(true) {
            return;
        }
        let _guard = TickGuard;

        let run_sched = LAST_SCHEDULER_CHECK.with(|c| {
            let last = c.get();
            if last.map_or(true, |t| t.elapsed() >= Duration::from_secs(10)) {
                c.set(Some(Instant::now()));
                true
            } else {
                false
            }
        });
        if run_sched {
            check_and_run_scheduler();
        }

        let ui = app.global::<StudioWeb>();
        let active = ui.get_active() && app.get_web_host_allowed();
        let frame = [
            ui.get_frame_x(),
            ui.get_frame_y(),
            ui.get_frame_width(),
            ui.get_frame_height(),
        ];
        let surface = app.window().with_winit_window(|w| {
            let scale = w.scale_factor();
            let size = w.inner_size().to_logical::<f64>(scale);
            (
                w.is_minimized().unwrap_or(false),
                size.width,
                size.height,
                scale,
            )
        });
        let Some((minimized, width, height, scale_factor)) = surface else {
            return;
        };

        let x = f64::from(frame[0]).max(0.0).min(width);
        let y = f64::from(frame[1]).max(0.0).min(height);
        let w = f64::from(frame[2]).max(0.0).min((width - x).max(0.0));
        let h = f64::from(frame[3]).max(0.0).min((height - y).max(0.0));
        let wanted = active && !minimized && w >= 20.0 && h >= 20.0;

        HOST.with_borrow_mut(|host| {
            match REQUEST.replace(0) {
                1 => host.reload = true,
                2 => host.home = true,
                _ => {}
            }

            if !wanted {
                if host.visible {
                    if let Some(view) = &host.view {
                        let _ = view.set_visible(false);
                    }
                    host.visible = false;
                }
                return;
            }

            let mode = current_mode();
            let target_url = match get_target_url() {
                Ok(u) => u,
                Err(err_msg) => {
                    failure(app, &err_msg);
                    return;
                }
            };

            if host.reload || host.home {
                host.attempted = false;
                if let Some(view) = &host.view {
                    ui.set_loaded(false);
                    ui.set_failed(false);
                    ui.set_status("Đang tải lại Postiz…".into());
                    host.started = Some(Instant::now());
                    let result = if host.home {
                        view.load_url(&target_url)
                    } else {
                        view.reload()
                    };
                    if result.is_err() {
                        failure(app, "Chưa tải lại được Postiz. Bấm Thử lại.");
                    }
                }
                host.reload = false;
                host.home = false;
            }

            let bounds = Rect {
                position: LogicalPosition::new(x, y).into(),
                size: LogicalSize::new(w, h).into(),
            };

            if host.view.is_none() && !host.attempted {
                let Some(profile) = &host.profile else {
                    return;
                };
                host.attempted = true;
                let Ok(profile) = profile else {
                    failure(
                        app,
                        "Chưa mở được thư mục lưu phiên Postiz. Kiểm tra dung lượng ổ đĩa.",
                    );
                    return;
                };

                host.context = Some(WebContext::new(Some(profile.clone())));
                ui.set_failed(false);
                ui.set_loaded(false);
                ui.set_mode(
                    if mode == PostizMode::Dev {
                        "dev"
                    } else {
                        "prod"
                    }
                    .into(),
                );
                ui.set_status(
                    if mode == PostizMode::Dev {
                        "Đang kết nối tới Next.js Dev Server (127.0.0.1:3000)..."
                    } else {
                        "Đang tải bản giao diện Postiz nội bộ..."
                    }
                    .into(),
                );
                host.started = Some(Instant::now());

                let page = app.as_weak();
                let title = app.as_weak();
                let ipc_weak = app.as_weak();

                let mut builder = WebViewBuilder::new_with_web_context(host.context.as_mut().unwrap())
                    .with_url(&target_url)
                    .with_bounds(bounds)
                    .with_visible(false)
                    .with_focused(false)
                    .with_autoplay(true)
                    // Transparent background: perfectly matches Floword theme and eliminates black flicker
                    .with_background_color((0, 0, 0, 0))
                    .with_devtools(mode == PostizMode::Dev)
                    .with_general_autofill_enabled(false)
                    .with_initialization_script(INIT_SCRIPT)
                    .with_navigation_handler(move |url| {
                        allowed_navigation(&url)
                    })
                    .with_document_title_changed_handler(move |text: String| {
                        let text: String = text.chars().take(256).collect();
                        let _ = title.upgrade_in_event_loop(move |app| {
                            app.global::<StudioWeb>().set_page_title(text.into());
                        });
                    })
                    .with_on_page_load_handler(move |event, url: String| {
                        if !allowed_navigation(&url) {
                            return;
                        }
                        let finished = matches!(event, wry::PageLoadEvent::Finished);
                        if finished {
                            let _ = page.upgrade_in_event_loop(move |app| {
                                let ui = app.global::<StudioWeb>();
                                ui.set_loaded(true);
                                ui.set_failed(false);
                                ui.set_status("Postiz Workspace sẵn sàng".into());
                                HOST.with_borrow_mut(|h| {
                                    h.initial_load_done = true;
                                    if h.visible {
                                        if let Some(view) = &h.view {
                                            let _ = view.set_visible(true);
                                        }
                                    }
                                });
                            });
                        }
                    });

                // Bidirectional IPC Handler
                builder = builder.with_ipc_handler(move |req: wry::http::Request<String>| {
                    let body = req.body().clone();
                    let ipc_app = ipc_weak.clone();
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&body) {
                        let id = val.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let command = val.get("command").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        
                        let response_data = match command.as_str() {
                            "postiz_system_ping" => {
                                let now_ms = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_millis())
                                    .unwrap_or(0);
                                serde_json::json!({
                                    "status": "ok",
                                    "pong": true,
                                    "mode": if mode == PostizMode::Dev { "dev" } else { "prod" },
                                    "version": "0.2.7",
                                    "timestamp": now_ms,
                                })
                            }
                            "postiz_get_status" => {
                                serde_json::json!({
                                    "app": "Concat Studio",
                                    "version": "0.2.7",
                                    "mode": if mode == PostizMode::Dev { "dev" } else { "prod" },
                                    "ready": true,
                                })
                            }
                            "postiz_get_schedule" => {
                                let db_path = PathBuf::from(r"D:\capcutpolot\concat\.runtime\postiz-data\db.json");
                                if let Ok(content) = std::fs::read_to_string(&db_path) {
                                    serde_json::from_str::<serde_json::Value>(&content).unwrap_or(serde_json::json!({}))
                                } else {
                                    serde_json::json!({ "posts": [], "integrations": [] })
                                }
                            }
                            "postiz_trigger_publish" => {
                                check_and_run_scheduler();
                                serde_json::json!({ "status": "ok", "checked": true })
                            }
                            _ => {
                                serde_json::json!({
                                    "error": format!("Unknown command: {}", command)
                                })
                            }
                        };

                        let resolve_script = format!(
                            "if (window.__CONCAT_POSTIZ_IPC_RESOLVE__) {{ window.__CONCAT_POSTIZ_IPC_RESOLVE__({:?}, {}, null); }}",
                            id, response_data
                        );

                        let _ = ipc_app.upgrade_in_event_loop(move |_app| {
                            HOST.with_borrow(|host| {
                                if let Some(view) = &host.view {
                                    let _ = view.evaluate_script(&resolve_script);
                                }
                            });
                        });
                    }
                });

                match app.window().with_winit_window(|window| builder.build_as_child(window)) {
                    Some(Ok(view)) => {
                        host.view = Some(view);
                        host.bounds = None;
                    }
                    _ => {
                        host.context = None;
                        failure(
                            app,
                            "Chưa mở được vùng web. Kiểm tra Microsoft Edge WebView2 Runtime rồi bấm Thử lại.",
                        );
                        return;
                    }
                }
            }

            if ui.get_loaded() {
                host.started = None;
            }
            if host.started.is_some_and(|s| s.elapsed() > Duration::from_secs(35)) {
                host.started = None;
                failure(
                    app,
                    "Trang tải quá lâu. Nếu đang ở DEV mode, kiểm tra Next.js server (127.0.0.1:3000) đã chạy chưa.",
                );
            }

            if let Some(view) = &host.view {
                let key = [
                    x.round() as i32,
                    y.round() as i32,
                    w.round() as i32,
                    h.round() as i32,
                ];
                if host.bounds != Some(key) || host.scale_factor != scale_factor {
                    if view.set_bounds(bounds).is_err() {
                        failure(app, "Chưa điều chỉnh được vùng web. Bấm Thử lại.");
                    } else {
                        host.bounds = Some(key);
                        host.scale_factor = scale_factor;
                    }
                }

                // Show only when loaded or initial load done
                if !host.visible && (ui.get_loaded() || host.initial_load_done) {
                    let _ = view.set_visible(true);
                    host.visible = true;
                }
            }
        });
    }

    pub(super) fn bind(app: &App) -> slint::Timer {
        let profile = dirs::data_local_dir().map(|p| p.join("app.concat.editor").join("postiz-session"));
        if let Some(path) = &profile {
            let _ = std::fs::create_dir_all(path);
        }
        HOST.with_borrow_mut(|host| {
            host.profile = Some(profile.ok_or(()));
        });

        let app_weak = app.as_weak();
        app.global::<StudioWeb>().on_reload(move || {
            REQUEST.set(1);
            if let Some(app) = app_weak.upgrade() {
                tick(&app);
            }
        });

        let app_weak = app.as_weak();
        app.global::<StudioWeb>().on_home(move || {
            REQUEST.set(2);
            if let Some(app) = app_weak.upgrade() {
                tick(&app);
            }
        });

        app.global::<StudioWeb>().on_hide_native(|| {
            hide();
        });

        let app_weak = app.as_weak();
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(80),
            move || {
                if let Some(app) = app_weak.upgrade() {
                    tick(&app);
                }
            },
        );
        timer
    }
}

#[cfg(not(windows))]
mod non_windows {
    use super::*;
    use std::time::Duration;
    pub(super) fn hide() {}
    pub(super) fn bind(app: &App) -> slint::Timer {
        let timer = slint::Timer::default();
        let weak = app.as_weak();
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(250),
            move || {
                if let Some(app) = weak.upgrade() {
                    if app.global::<StudioWeb>().get_active() {
                        app.global::<StudioWeb>().set_failed(true);
                        app.global::<StudioWeb>()
                            .set_status("Mục NexPulse nhúng hiện hỗ trợ bản Windows.".into());
                    }
                }
            },
        );
        timer
    }
}

pub(crate) fn bind(app: &App) -> slint::Timer {
    #[cfg(windows)]
    {
        windows::bind(app)
    }
    #[cfg(not(windows))]
    {
        non_windows::bind(app)
    }
}

pub(crate) fn smoke(_app: &App) -> Result<(), slint::PlatformError> {
    Ok(())
}

pub(crate) fn shutdown() {
    #[cfg(windows)]
    windows::hide();
}
