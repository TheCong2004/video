// SPDX-License-Identifier: AGPL-3.0-or-later
//! An unprivileged, lazy child webview for the explicitly requested NexPulse page.
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

    pub(super) const HOME: &str = "https://deloy-internet-frontend.vercel.app/launches";

    pub(super) fn allowed_navigation(value: &str) -> bool {
        url::Url::parse(value).is_ok_and(|u| {
            u.scheme() == "https"
                && u.host_str() == Some("deloy-internet-frontend.vercel.app")
                && u.port_or_known_default() == Some(443)
                && u.username().is_empty()
                && u.password().is_none()
        })
    }

    #[derive(Default)]
    struct Host {
        // Drop the view before its context. Both stay on the Slint UI thread.
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
    }
    thread_local! {
        static HOST: RefCell<Host> = RefCell::new(Host::default());
        // WebView2 initialization pumps messages: never re-enter RefCell borrows.
        static IN_TICK: Cell<bool> = const { Cell::new(false) };
        static REQUEST: Cell<u8> = const { Cell::new(0) };
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

    fn tick(app: &App) {
        if IN_TICK.replace(true) {
            return;
        }
        let _guard = TickGuard;
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
        let wanted = active && !minimized && w >= 2.0 && h >= 2.0;
        HOST.with_borrow_mut(|host| {
            match REQUEST.replace(0) {
                1 => host.reload = true,
                2 => host.home = true,
                _ => {}
            }
            if !wanted {
                if host.visible {
                    if let Some(view) = &host.view { let _ = view.set_visible(false); }
                    host.visible = false;
                }
                return;
            }
            if host.reload || host.home {
                host.attempted = false;
                if let Some(view) = &host.view {
                    ui.set_loaded(false); ui.set_failed(false);
                    ui.set_status("Đang tải NexPulse…".into());
                    host.started = Some(Instant::now());
                    let result = if host.home { view.load_url(HOME) } else { view.reload() };
                    if result.is_err() { failure(app, "Chưa tải lại được NexPulse. Bấm Thử lại."); }
                }
                host.reload = false; host.home = false;
            }
            let bounds = Rect {
                position: LogicalPosition::new(x, y).into(),
                size: LogicalSize::new(w, h).into(),
            };
            if host.view.is_none() && !host.attempted {
                let Some(profile) = &host.profile else { return; };
                host.attempted = true;
                let Ok(profile) = profile else {
                    failure(app, "Chưa mở được vùng lưu phiên NexPulse. Kiểm tra dung lượng ổ đĩa rồi mở lại app.");
                    return;
                };
                host.context = Some(WebContext::new(Some(profile.clone())));
                ui.set_failed(false); ui.set_loaded(false);
                ui.set_status("Đang mở NexPulse…".into());
                host.started = Some(Instant::now());
                let page = app.as_weak();
                let title = app.as_weak();
                let blocked = app.as_weak();
                let popup = app.as_weak();
                let builder = WebViewBuilder::new_with_web_context(host.context.as_mut().unwrap())
                    .with_url(HOME)
                    .with_bounds(bounds)
                    .with_visible(false)
                    .with_focused(false)
                    .with_autoplay(false)
                    .with_background_color((255, 255, 255, 255))
                    .with_devtools(false)
                    .with_general_autofill_enabled(false)
                    // No IPC handler, initialization script, custom scheme or host objects.
                    .with_permission_handler(|_| wry::PermissionResponse::Deny)
                    .with_download_started_handler(|_, _| false)
                    .with_navigation_handler(move |url| {
                        let allowed = allowed_navigation(&url);
                        if !allowed {
                            let _ = blocked.upgrade_in_event_loop(|app| app.global::<StudioWeb>().set_status(
                                "Liên kết ngoài NexPulse không được mở trong vùng này.".into()));
                        }
                        allowed
                    })
                    .with_new_window_req_handler(move |_, _| {
                        let _ = popup.upgrade_in_event_loop(|app| app.global::<StudioWeb>().set_status(
                            "Trang yêu cầu cửa sổ ngoài. Đăng nhập liên kết mạng xã hội chưa được hỗ trợ trong mục nhúng này.".into()));
                        wry::NewWindowResponse::Deny
                    })
                    .with_document_title_changed_handler(move |text| {
                        let text: String = text.chars().take(256).collect();
                        let _ = title.upgrade_in_event_loop(move |app| {
                            app.global::<StudioWeb>().set_page_title(text.into());
                        });
                    })
                    .with_on_page_load_handler(move |event, url| {
                        if !allowed_navigation(&url) { return; }
                        let finished = matches!(event, wry::PageLoadEvent::Finished);
                        let _ = page.upgrade_in_event_loop(move |app| {
                            let ui = app.global::<StudioWeb>();
                            ui.set_loaded(finished);
                            ui.set_failed(false);
                            ui.set_status(if finished { "NexPulse · deloy-internet-frontend.vercel.app" } else { "Đang tải NexPulse…" }.into());
                        });
                    });
                match app.window().with_winit_window(|window| builder.build_as_child(window)) {
                    Some(Ok(view)) => { host.view = Some(view); host.bounds = None; }
                    _ => {
                        host.context = None;
                        failure(app, "Chưa mở được vùng web. Kiểm tra Microsoft Edge WebView2 Runtime rồi bấm Thử lại.");
                        return;
                    }
                }
            }
            if ui.get_loaded() { host.started = None; }
            if host.started.is_some_and(|s| s.elapsed() > Duration::from_secs(30)) {
                host.started = None;
                failure(app, "Trang tải quá lâu. Kiểm tra mạng rồi bấm Thử lại.");
            }
            if let Some(view) = &host.view {
                let key = [x.round() as i32, y.round() as i32, w.round() as i32, h.round() as i32];
                if host.bounds != Some(key) || host.scale_factor != scale_factor {
                    if view.set_bounds(bounds).is_err() {
                        failure(app, "Chưa điều chỉnh được vùng web. Bấm Thử lại.");
                    } else { host.bounds = Some(key); host.scale_factor = scale_factor; }
                }
                let show = app.get_web_host_allowed() && ui.get_active() && !ui.get_failed();
                if host.visible != show {
                    if view.set_visible(show).is_ok() { host.visible = show; }
                }
            }
        });
    }

    pub(super) fn bind(app: &App) -> slint::Timer {
        let weak = app.as_weak();
        let worker = std::thread::Builder::new()
            .name("nexpulse-profile".into())
            .spawn(move || {
                let path = std::env::var_os("LOCALAPPDATA")
                    .map(PathBuf::from)
                    .map(|p| p.join("app.concat.editor").join("web").join("nexpulse"))
                    .ok_or(())
                    .and_then(|path| {
                        std::fs::create_dir_all(&path).map_err(|_| ())?;
                        Ok(path)
                    });
                let _ = weak.upgrade_in_event_loop(move |_| {
                    HOST.with_borrow_mut(|host| host.profile = Some(path))
                });
            });
        if worker.is_err() {
            HOST.with_borrow_mut(|host| host.profile = Some(Err(())));
        }
        app.global::<StudioWeb>().on_hide_native(hide);
        app.global::<StudioWeb>().on_reload(|| REQUEST.set(1));
        app.global::<StudioWeb>().on_home(|| REQUEST.set(2));
        let weak = app.as_weak();
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(100),
            move || {
                if let Some(app) = weak.upgrade() {
                    tick(&app);
                }
            },
        );
        timer
    }

    #[cfg(debug_assertions)]
    pub(crate) fn smoke_state() -> (bool, Option<[i32; 4]>) {
        HOST.with_borrow(|host| (host.visible, host.bounds))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn allows_only_the_supplied_https_origin() {
            assert!(allowed_navigation(HOME));
            assert!(allowed_navigation(
                "https://deloy-internet-frontend.vercel.app/login?next=/launches"
            ));
            for value in [
                "http://deloy-internet-frontend.vercel.app/",
                "https://deloy-internet-frontend.vercel.app.evil.test/",
                "https://evil.test/",
                "file:///C:/secret",
                "javascript:alert(1)",
                "https://127.0.0.1/",
                "https://user@deloy-internet-frontend.vercel.app/",
                "https://deloy-internet-frontend.vercel.app:8080/",
                "bad url",
            ] {
                assert!(!allowed_navigation(value), "must reject {value}");
            }
        }
    }
}

/// Keep this timer alive for the application event loop; non-Windows is explicit.
pub(crate) fn bind(app: &App) -> slint::Timer {
    #[cfg(windows)]
    {
        windows::bind(app)
    }
    #[cfg(not(windows))]
    {
        let weak = app.as_weak();
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(250),
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

#[cfg(all(windows, debug_assertions))]
pub(crate) use windows::smoke_state;

/// Opt-in smoke of the actual launcher and native renderer, with an isolated profile.
#[cfg(all(windows, debug_assertions))]
pub(crate) fn smoke(app: &App) -> Result<(), slint::PlatformError> {
    use slint::winit_030::WinitWindowAccessor;
    use std::{
        cell::Cell,
        rc::Rc,
        time::{Duration, Instant},
    };
    app.window()
        .set_size(slint::LogicalSize::new(1200.0, 800.0));
    let ui = app.global::<StudioWeb>();
    ui.set_open_request(ui.get_open_request() + 1);
    app.show()?;
    let stage = Rc::new(Cell::new(0));
    let previous_width = Rc::new(Cell::new(0));
    let changed = Rc::new(Cell::new(Instant::now()));
    let passed = Rc::new(Cell::new(false));
    let result = passed.clone();
    let weak = app.as_weak();
    let deadline = Instant::now();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(200),
        move || {
            let Some(app) = weak.upgrade() else {
                return;
            };
            let ui = app.global::<StudioWeb>();
            if deadline.elapsed() > Duration::from_secs(50) {
                eprintln!(
                    "NEXPULSE_WEB_SMOKE=FAIL native-launcher stage={} loaded={} failed={}",
                    stage.get(),
                    ui.get_loaded(),
                    ui.get_failed()
                );
                let _ = slint::quit_event_loop();
                return;
            }
            match stage.get() {
                0 if ui.get_loaded()
                    && ui.get_page_title().contains("NexPulse")
                    && smoke_state().0 =>
                {
                    let bounds = smoke_state().1.unwrap();
                    if bounds[0] < 200 || bounds[1] < 32 || bounds[2] < 300 || bounds[3] < 300 {
                        return;
                    }
                    previous_width.set(bounds[2]);
                    app.window().with_winit_window(|w| w.focus_window());
                    app.invoke_menu_opened(0);
                    changed.set(Instant::now());
                    stage.set(1);
                }
                1 if changed.get().elapsed() > Duration::from_millis(300) => {
                    if smoke_state().0 {
                        let _ = slint::quit_event_loop();
                        return;
                    }
                    app.invoke_menu_opened(-1);
                    app.window().set_size(slint::LogicalSize::new(900.0, 600.0));
                    changed.set(Instant::now());
                    stage.set(2);
                }
                2 if changed.get().elapsed() > Duration::from_millis(600) => {
                    if !smoke_state().0 || smoke_state().1.unwrap()[2] >= previous_width.get() {
                        let _ = slint::quit_event_loop();
                        return;
                    }
                    app.window().with_winit_window(|w| w.set_minimized(true));
                    changed.set(Instant::now());
                    stage.set(3);
                }
                3 if changed.get().elapsed() > Duration::from_millis(600) => {
                    if smoke_state().0 {
                        let _ = slint::quit_event_loop();
                        return;
                    }
                    app.window().with_winit_window(|w| w.set_minimized(false));
                    changed.set(Instant::now());
                    stage.set(4);
                }
                4 if changed.get().elapsed() > Duration::from_millis(600) && smoke_state().0 => {
                    ui.set_active(false);
                    ui.invoke_hide_native();
                    result.set(!smoke_state().0);
                    let _ = slint::quit_event_loop();
                }
                _ => {}
            }
        },
    );
    slint::run_event_loop_until_quit()?;
    drop(timer);
    if passed.get() {
        println!(
            "NEXPULSE_WEB_SMOKE=PASS actual-Concat-launcher, real-public-page, child-bounds, menu-hide, resize, minimize-restore, route-hide; no login/post/IPC"
        );
        Ok(())
    } else {
        Err(slint::PlatformError::Other(
            "NEXPULSE_WEB_SMOKE_FAILED".into(),
        ))
    }
}
