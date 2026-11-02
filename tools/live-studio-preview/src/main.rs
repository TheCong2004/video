//! Render the actual native Live Studio, with all service callbacks unbound.
use std::rc::Rc;
use slint::{ComponentHandle, PhysicalSize, Rgb8Pixel};
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
mod ui {
    slint::include_modules!();
    #[cfg(feature = "native")]
    pub type App = Preview;
}
use ui::*;
#[cfg(feature = "native")]
#[path = "../../../src/crates/concat/src/studio_temple.rs"]
mod studio_temple;
struct Headless(Rc<MinimalSoftwareWindow>);
impl slint::platform::Platform for Headless {
    fn create_window_adapter(&self)->Result<Rc<dyn slint::platform::WindowAdapter>,slint::PlatformError>{Ok(self.0.clone())}
}
fn main()->Result<(),Box<dyn std::error::Error>>{
    let args:Vec<_>=std::env::args().collect();
    #[cfg(feature = "native")]
    if args.get(1).map(String::as_str) == Some("--native") {
        let app = Preview::new()?;
        app.set_viewport_width(args.get(2).and_then(|s|s.parse().ok()).unwrap_or(1632));
        app.set_viewport_height(args.get(3).and_then(|s|s.parse().ok()).unwrap_or(958));
        app.global::<Theme>().set_dark(true);
        let _controller = studio_temple::bind(&app);
        app.global::<StudioLive>().set_cd_page_active(true);
        app.global::<StudioLive>().set_cd_preview_allowed(true);
        app.global::<StudioLive>().invoke_cd_ensure();
        let weak = app.as_weak();
        let diagnostic = slint::Timer::default();
        diagnostic.start(slint::TimerMode::SingleShot, std::time::Duration::from_secs(3), move || {
            use slint::winit_030::WinitWindowAccessor;
            if let Some(app) = weak.upgrade() {
                let live = app.global::<StudioLive>();
                println!("TEMPLE_PREVIEW_DIAGNOSTIC active={} allowed={} inline={} frame=[{},{},{},{}] loaded={} window={:?}", live.get_cd_page_active(), live.get_cd_preview_allowed(), live.get_cd_preview_inline(), live.get_cd_preview_x(),live.get_cd_preview_y(),live.get_cd_preview_width(),live.get_cd_preview_height(),live.get_cd_preview_loaded(),app.window().with_winit_window(|w|(w.inner_size(),w.scale_factor())));
            }
        });
        return app.run().map_err(Into::into);
    }
    let output=args.get(1).ok_or("PNG output required")?;
    let tab:i32=args.get(2).ok_or("tab 0..4 required")?.parse()?;
    let width:u32=args.get(3).map(|s|s.parse()).transpose()?.unwrap_or(960);
    let height:u32=args.get(4).map(|s|s.parse()).transpose()?.unwrap_or(900);
    if !(0..=4).contains(&tab) || !(340..=1920).contains(&width) || !(600..=1400).contains(&height){return Err("invalid preview arguments".into());}
    let window=MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Headless(window.clone())))?;
    let app=Preview::new()?;
    app.set_viewport_width(width as i32);app.set_viewport_height(height as i32);
    app.global::<Theme>().set_dark(args.get(5).map(String::as_str)==Some("dark"));
    app.global::<StudioLive>().set_cd_bottom_tab(tab.min(4));
    if args.get(6).map(String::as_str)==Some("active") {
        // Visual test state only; no service callbacks or product facts are installed.
        let ui=app.global::<StudioLive>();
        ui.set_kedi_available(true);ui.set_kedi_ready(true);ui.set_kedi_mode("simulation".into());
        ui.set_kedi_state("Live".into());ui.set_kedi_automatic(true);
        ui.set_kedi_status("Bố cục kiểm thử · runtime chưa chạy".into());
    }
    app.window().set_size(PhysicalSize::new(width,height));
    app.show()?;
    let mut pixels=vec![Rgb8Pixel::default();(width*height)as usize];
    window.draw_if_needed(|renderer|{renderer.render(&mut pixels,width as usize);});
    if args.get(6).map(String::as_str)==Some("probe") {
        use slint::{LogicalPosition, platform::{WindowEvent,PointerEventButton}};
        app.global::<StudioLive>().set_cd_bottom_tab(0);
        app.global::<StudioLive>().set_cd_sim_name("Giữ tên sau đổi tab".into());
        app.global::<StudioLive>().set_cd_bpm(61);
        let mut changed=false;
        'find_tab: for y in (300..height.saturating_sub(30)).step_by(10) {
            for x in (24..width.min(1000)).step_by(24) {
                let position=LogicalPosition::new(x as f32,y as f32);
                app.window().dispatch_event(WindowEvent::PointerPressed{position,button:PointerEventButton::Left});
                app.window().dispatch_event(WindowEvent::PointerReleased{position,button:PointerEventButton::Left});
                if app.global::<StudioLive>().get_cd_bottom_tab()==1 {changed=true;break 'find_tab;}
            }
        }
        assert!(changed,"pointer input must navigate to the avatar tab");
        assert_eq!(app.global::<StudioLive>().get_cd_sim_name().as_str(), "Giữ tên sau đổi tab");
        assert_eq!(app.global::<StudioLive>().get_cd_bpm(),61);
        app.global::<StudioLive>().set_cd_bottom_tab(tab.min(4));
        app.window().request_redraw();
        window.draw_if_needed(|renderer|{renderer.render(&mut pixels,width as usize);});
        println!("LIVE_STUDIO_TAB_INPUT=PASS actual pointer events");
    }
    let bytes:Vec<u8>=pixels.iter().flat_map(|p|[p.r,p.g,p.b]).collect();
    let mut encoder=png::Encoder::new(std::fs::File::create(output)?,width,height);
    encoder.set_color(png::ColorType::Rgb);encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&bytes)?;
    assert_eq!(app.global::<StudioLive>().get_cd_bottom_tab(),tab.min(4));
    println!("LIVE_STUDIO_RENDER=PASS tab={tab} size={width}x{height}; actual native component, no backend calls");
    Ok(())
}
