// SPDX-License-Identifier: AGPL-3.0-or-later
//! Real Electron child inside a dedicated launcher viewport; no browser mock.
use crate::ui::{App, StudioHotclip};
use slint::ComponentHandle;
use std::{cell::{Cell, RefCell}, time::Duration};
use concat_studio::hotclip::{Event, Process};

#[derive(Default)]
struct Host {
    process: Option<Process>,
    hwnd: usize,
    pid: u32,
    attached: bool,
    visible: bool,
    bounds: Option<[i32;4]>,
}
thread_local! {
    static HOST: RefCell<Host> = RefCell::new(Host::default());
    static ENTERED: Cell<bool> = const { Cell::new(false) };
}
struct TickGuard;
impl Drop for TickGuard { fn drop(&mut self) { let _=ENTERED.try_with(|entered|entered.set(false)); } }

fn start(app: &App) {
    let ui=app.global::<StudioHotclip>();
    ui.set_failed(false); ui.set_loaded(false); ui.set_status("Đang khởi động Hotclip…".into());
    // Path construction only. All filesystem work and launch are in the worker.
    let data=std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("APPDATA"))
        .map(std::path::PathBuf::from).unwrap_or_default().join("app.concat.editor").join("hotclip");
    HOST.with_borrow_mut(|host| { if host.process.is_none() { host.process=Some(Process::start(data)); } });
}

#[cfg(windows)]
fn visible(host: &mut Host, show: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    if host.hwnd!=0 && host.visible!=show {
        if host.attached { if let Some(process)=&host.process { process.set_visible(show); } }
        // SAFETY: owned child HWND validated against the launched PID before attach.
        // ShowWindow(SW_HIDE) activates a different window and can dismiss the
        // host's just-opened menu. Visibility must not change activation/focus.
        unsafe { SetWindowPos(host.hwnd as _,std::ptr::null_mut(),0,0,0,0,
            SWP_NOACTIVATE|SWP_NOZORDER|SWP_NOMOVE|SWP_NOSIZE|if show {SWP_SHOWWINDOW}else{SWP_HIDEWINDOW}); }
    }
    host.visible=show;
}
#[cfg(not(windows))]
fn visible(host: &mut Host, _: bool) { host.visible=false; }

fn hide() {
    if ENTERED.try_with(|entered|entered.replace(true)).unwrap_or(true) { return; }
    let _guard=TickGuard;
    let _=HOST.try_with(|host|visible(&mut host.borrow_mut(),false));
}

#[cfg(windows)]
fn actual_child_bounds(hwnd: usize, origin: [i32;2]) -> Option<[i32;4]> {
    use windows_sys::Win32::{Foundation::RECT, UI::WindowsAndMessaging::GetWindowRect};
    let mut rect=RECT::default();
    // SAFETY: read-only query; the handle belongs to our validated child.
    if unsafe { GetWindowRect(hwnd as _,&mut rect) }==0 { return None; }
    Some([rect.left.saturating_sub(origin[0]),rect.top.saturating_sub(origin[1]),
        rect.right.saturating_sub(rect.left),rect.bottom.saturating_sub(rect.top)])
}

#[cfg(all(windows,debug_assertions))]
fn observed_child_bounds(app: &App) -> Option<[i32;4]> {
    use slint::winit_030::WinitWindowAccessor;
    let origin=app.window().with_winit_window(|w|w.inner_position().ok()).flatten()?;
    HOST.with_borrow(|host|actual_child_bounds(host.hwnd,[origin.x,origin.y]))
}

#[cfg(all(windows,debug_assertions))]
fn observed_child_visible() -> bool {
    HOST.with_borrow(|host| unsafe {
        // SAFETY: read-only query of this host's validated child.
        windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible(host.hwnd as _)!=0
    })
}

#[cfg(windows)]
fn tick(app: &App) {
    use slint::winit_030::{WinitWindowAccessor, winit::raw_window_handle::{HasWindowHandle, RawWindowHandle}};
    use windows_sys::Win32::{Foundation::{GetLastError, SetLastError}, UI::WindowsAndMessaging::*};
    if ENTERED.replace(true) { return; }
    let _guard=TickGuard;
    let ui=app.global::<StudioHotclip>();
    let surface=app.window().with_winit_window(|window| {
        let RawWindowHandle::Win32(handle)=window.window_handle().ok()?.as_raw() else { return None; };
        let origin=window.inner_position().ok()?;
        Some((handle.hwnd.get(),window.is_minimized().unwrap_or(false),window.scale_factor(),window.inner_size(),[origin.x,origin.y]))
    }).flatten();
    HOST.with_borrow_mut(|host| {
        if let Some(process)=&host.process { if let Some(event)=process.poll() { match event {
            Event::Ready{hwnd,pid} => { host.hwnd=hwnd; host.pid=pid; },
            Event::Failed(error) => { visible(host,false); host.hwnd=0; host.attached=false; ui.set_loaded(false); ui.set_failed(true); ui.set_status(error.into()); }
        } } }
        let Some((parent,minimized,scale,size,origin))=surface else { return; };
        if host.hwnd==0 { return; }
        let hwnd=host.hwnd as *mut std::ffi::c_void;
        if !host.attached {
            let mut actual_pid=0;
            // SAFETY: query does not dereference HWND; PID must match owned child.
            if unsafe { GetWindowThreadProcessId(hwnd,&mut actual_pid) }==0 || actual_pid!=host.pid { ui.set_failed(true); ui.set_status("Cửa sổ Hotclip không còn thuộc phiên này.".into()); host.hwnd=0; return; }
            // SAFETY: both live HWNDs belong to this host/its owned Electron process.
            let ok=unsafe {
                let style=GetWindowLongPtrW(hwnd,GWL_STYLE) as u32;
                SetWindowLongPtrW(hwnd,GWL_STYLE,((style & !(WS_POPUP|WS_CAPTION|WS_THICKFRAME|WS_MINIMIZEBOX|WS_MAXIMIZEBOX)) | WS_CHILD | WS_CLIPSIBLINGS | WS_CLIPCHILDREN) as isize);
                SetLastError(0); let previous=SetParent(hwnd,parent as _);
                !previous.is_null() || GetLastError()==0
            };
            if !ok { ui.set_failed(true); ui.set_status("Windows chưa ghép được cửa sổ Hotclip. Hãy mở hai ứng dụng cùng quyền truy cập.".into()); host.hwnd=0; return; }
            host.attached=true; ui.set_loaded(true); ui.set_status("Hotclip đang chạy trong Concat · nhập, phân tích và xuất video tại đây.".into());
        }
        let x=(f64::from(ui.get_frame_x())*scale).round().clamp(0.,size.width as f64) as i32;
        let y=(f64::from(ui.get_frame_y())*scale).round().clamp(0.,size.height as f64) as i32;
        let width=(f64::from(ui.get_frame_width())*scale).round().clamp(0.,(size.width as i32-x).max(0) as f64) as i32;
        let height=(f64::from(ui.get_frame_height())*scale).round().clamp(0.,(size.height as i32-y).max(0) as f64) as i32;
        let wanted=ui.get_active() && app.get_web_host_allowed() && !minimized && width>1 && height>1;
        let bounds=[x,y,width,height];
        let actual=actual_child_bounds(host.hwnd,origin);
        if wanted && (host.bounds!=Some(bounds) || actual!=Some(bounds)) {
            // SAFETY: validated attached child; physical coordinates clamped to host.
            // Compare native geometry, not just our last request: Electron can
            // restore old bounds when shown, or a native drag can move the HWND.
            if unsafe { SetWindowPos(hwnd,std::ptr::null_mut(),x,y,width,height,SWP_NOACTIVATE|SWP_NOZORDER|SWP_FRAMECHANGED) }!=0 {
                host.bounds=Some(bounds);
            }
        }
        visible(host,wanted);
    });
}

/// Start with Concat, even before the Hotclip page is selected. Smoke runs opt in.
pub(crate) fn bind(app: &App, auto_start: bool) -> slint::Timer {
    if auto_start { start(app); }
    app.global::<StudioHotclip>().on_hide_native(hide);
    let weak=app.as_weak();
    app.global::<StudioHotclip>().on_retry(move || { if let Some(app)=weak.upgrade() {
        if !app.global::<StudioHotclip>().get_failed() { return; }
        shutdown(); start(&app);
    } });
    let timer=slint::Timer::default(); let weak=app.as_weak();
    timer.start(slint::TimerMode::Repeated,Duration::from_millis(100),move || {
        if let Some(app)=weak.upgrade() {
            #[cfg(windows)] tick(&app);
            #[cfg(not(windows))] { app.global::<StudioHotclip>().set_failed(true); app.global::<StudioHotclip>().set_status("Hotclip nhúng hiện hỗ trợ Windows.".into()); }
        }
    });
    timer
}
/// Called after the event loop, before the host's explicit process exit.
pub(crate) fn shutdown() {
    let process=HOST.with_borrow_mut(|host| { visible(host,false); let process=host.process.take(); *host=Host::default(); process });
    if let Some(process)=process { process.shutdown(); }
}

#[cfg(all(windows,debug_assertions))]
pub(crate) fn smoke(app: &App) -> Result<(),slint::PlatformError> {
    use std::{rc::Rc,time::Instant};
    use slint::winit_030::WinitWindowAccessor;
    app.window().set_size(slint::LogicalSize::new(1200.,800.)); app.show()?;
    let stage=Rc::new(Cell::new(0)); let changed=Rc::new(Cell::new(Instant::now()));
    let passed=Rc::new(Cell::new(false)); let result=passed.clone();
    let old_width=Rc::new(Cell::new(0)); let deadline=Instant::now(); let weak=app.as_weak();
    let timer=slint::Timer::default();
    timer.start(slint::TimerMode::Repeated,Duration::from_millis(200),move || {
        let Some(app)=weak.upgrade() else { return; }; let ui=app.global::<StudioHotclip>();
        if deadline.elapsed()>Duration::from_secs(45) || ui.get_failed() {
            eprintln!("HOTCLIP_NATIVE_SMOKE=FAIL stage={} status={} menu={} allowed={} active={} visible={}",stage.get(),ui.get_status(),app.get_open_menu(),app.get_web_host_allowed(),ui.get_active(),HOST.with_borrow(|host|host.visible)); let _=slint::quit_event_loop(); return;
        }
        let (visible,bounds,pid)=HOST.with_borrow(|host|(host.visible,host.bounds,host.pid));
        match stage.get() {
            0 if ui.get_loaded() && !visible => { println!("HOTCLIP_OWNED_PID={pid}"); ui.set_open_request(ui.get_open_request()+1); stage.set(1); },
            1 if visible && observed_child_bounds(&app)==bounds && bounds.is_some_and(|b|b[0]>=200 && b[1]>=32 && b[2]>=300 && b[3]>=300) => {
                let b=bounds.unwrap();
                let perturbed=HOST.with_borrow(|host| unsafe {
                    // QA only: perturb our own validated child without changing
                    // the host's cached desired bounds. The next tick must fix it.
                    windows_sys::Win32::UI::WindowsAndMessaging::SetWindowPos(host.hwnd as _,std::ptr::null_mut(),b[0]+137,b[1]+29,b[2]-71,b[3]-43,
                        windows_sys::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE|windows_sys::Win32::UI::WindowsAndMessaging::SWP_NOZORDER)!=0
                });
                if !perturbed || observed_child_bounds(&app)==bounds {
                    eprintln!("HOTCLIP_GEOMETRY_SMOKE=FAIL could not displace owned child");
                    let _=slint::quit_event_loop(); return;
                }
                changed.set(Instant::now()); stage.set(10);
            },
            10 if changed.get().elapsed()>Duration::from_millis(500) && visible && observed_child_bounds(&app)==bounds => {
                println!("HOTCLIP_GEOMETRY_SMOKE=PASS actual-rect, deliberate-drift-recovery");
                // Use the real menu callback: direct property assignment is
                // overwritten by the Studio state publication on the next tick.
                old_width.set(bounds.unwrap()[2]); app.window().with_winit_window(|w|w.focus_window()); app.invoke_menu_opened(0); changed.set(Instant::now()); stage.set(2);
            },
            2 if changed.get().elapsed()>Duration::from_millis(300) && !visible && !observed_child_visible() => {
                app.invoke_menu_opened(-1); app.window().set_size(slint::LogicalSize::new(900.,600.)); changed.set(Instant::now()); stage.set(3);
            },
            3 if changed.get().elapsed()>Duration::from_millis(500) && visible && observed_child_bounds(&app)==bounds && bounds.is_some_and(|b|b[2]<old_width.get()) => {
                app.window().with_winit_window(|w|w.set_minimized(true)); changed.set(Instant::now()); stage.set(4);
            },
            4 if changed.get().elapsed()>Duration::from_millis(300) && !visible && !observed_child_visible() => {
                app.window().with_winit_window(|w|w.set_minimized(false)); changed.set(Instant::now()); stage.set(5);
            },
            5 if changed.get().elapsed()>Duration::from_millis(500) && visible && observed_child_bounds(&app)==bounds => {
                let web=app.global::<crate::ui::StudioWeb>(); web.set_open_request(web.get_open_request()+1); changed.set(Instant::now()); stage.set(6);
            },
            6 if changed.get().elapsed()>Duration::from_millis(300) && !visible && !observed_child_visible() && !ui.get_active() => {
                passed.set(true); println!("HOTCLIP_NATIVE_SMOKE=PASS autostart-hidden, real-Electron-IPC-media, child-bounds, menu-hide, resize, minimize-restore, route-hide"); let _=slint::quit_event_loop();
            },
            _=>{}
        }
    });
    slint::run_event_loop()?; timer.stop(); app.hide()?;
    if result.get() { Ok(()) } else { Err(slint::PlatformError::Other("Hotclip native integration smoke failed".into())) }
}
