//! Opt-in real WebView2 smoke; public page only, no login/post/project access.
mod ui {
    slint::include_modules!();
}
#[path = "../../../src/crates/concat/src/studio_web.rs"]
mod studio_web;
#[path = "../../../src/crates/concat/src/studio_hotclip.rs"]
mod studio_hotclip;
use slint::ComponentHandle;
use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = ui::App::new()?;
    if std::env::args().any(|a| a == "--hotclip" || a == "--hotclip-smoke") {
        app.set_hotclip_mode(true);
        let _controller = studio_hotclip::bind(&app, true);
        let result = if std::env::args().any(|a|a=="--hotclip-smoke") {
            studio_hotclip::smoke(&app)
        } else {
            app.global::<ui::StudioHotclip>().set_active(true);
            app.run()
        };
        studio_hotclip::shutdown();
        return result.map_err(Into::into);
    }
    let _controller = studio_web::bind(&app);
    app.global::<ui::StudioWeb>().set_active(true);
    let smoke = std::env::args().any(|a| a == "--smoke");
    let verify = slint::Timer::default();
    if smoke {
        let stage = Rc::new(Cell::new(0));
        let before_width = Rc::new(Cell::new(0));
        let changed = Rc::new(Cell::new(Instant::now()));
        let deadline = Instant::now();
        let weak = app.as_weak();
        verify.start(slint::TimerMode::Repeated, Duration::from_millis(200), move || {
            let Some(app) = weak.upgrade() else { return; };
            let ui = app.global::<ui::StudioWeb>();
            if deadline.elapsed() > Duration::from_secs(55) {
                eprintln!("NEXPULSE_WEB_SMOKE=FAIL timeout stage={} loaded={} failed={} status={}", stage.get(), ui.get_loaded(), ui.get_failed(), ui.get_status());
                std::process::exit(1);
            }
            use slint::winit_030::WinitWindowAccessor;
            match stage.get() {
                0 if ui.get_loaded() && ui.get_page_title().contains("NexPulse") => {
                    let (shown, bounds) = studio_web::smoke_state();
                    if !shown { return; }
                    let bounds = bounds.expect("child bounds");
                    assert!(bounds[0] >= 200 && bounds[1] > 0 && bounds[2] > 400 && bounds[3] > 300);
                    before_width.set(bounds[2]);
                    ui.set_active(false);
                    ui.invoke_hide_native();
                    assert!(!studio_web::smoke_state().0, "route change hides immediately");
                    changed.set(Instant::now()); stage.set(1);
                }
                1 if changed.get().elapsed() > Duration::from_millis(300) => {
                    assert!(!studio_web::smoke_state().0);
                    app.window().set_size(slint::LogicalSize::new(800.0, 600.0));
                    ui.set_active(true);
                    changed.set(Instant::now()); stage.set(2);
                }
                2 if changed.get().elapsed() > Duration::from_millis(600) => {
                    let (shown, bounds) = studio_web::smoke_state();
                    assert!(shown);
                    assert!(bounds.unwrap()[2] < before_width.get(), "child resizes with native viewport");
                    app.set_web_host_allowed(false);
                    ui.invoke_hide_native();
                    assert!(!studio_web::smoke_state().0, "dialog/editor hides immediately");
                    changed.set(Instant::now()); stage.set(3);
                }
                3 if changed.get().elapsed() > Duration::from_millis(300) => {
                    assert!(!studio_web::smoke_state().0);
                    app.set_web_host_allowed(true);
                    app.window().with_winit_window(|w| w.set_minimized(true));
                    changed.set(Instant::now()); stage.set(4);
                }
                4 if changed.get().elapsed() > Duration::from_millis(600) => {
                    assert!(!studio_web::smoke_state().0, "minimized child hidden");
                    app.window().with_winit_window(|w| w.set_minimized(false));
                    changed.set(Instant::now()); stage.set(5);
                }
                5 if changed.get().elapsed() > Duration::from_millis(600) => {
                    assert!(studio_web::smoke_state().0, "restore shows the same child");
                    println!("NEXPULSE_WEB_SMOKE=PASS real-page-title, child-bounds, route-hide, resize, dialog-hide, minimize-restore; no login/post/IPC");
                    slint::quit_event_loop().unwrap();
                }
                _ => {}
            }
        });
    }
    app.run()?;
    Ok(())
}
