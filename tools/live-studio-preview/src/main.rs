//! Render the actual native Live Studio, with all service callbacks unbound.
use std::rc::Rc;
use slint::{ComponentHandle, PhysicalSize, Rgb8Pixel};
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
slint::include_modules!();
struct Headless(Rc<MinimalSoftwareWindow>);
impl slint::platform::Platform for Headless {
    fn create_window_adapter(&self)->Result<Rc<dyn slint::platform::WindowAdapter>,slint::PlatformError>{Ok(self.0.clone())}
}
fn main()->Result<(),Box<dyn std::error::Error>>{
    let args:Vec<_>=std::env::args().collect();
    let output=args.get(1).ok_or("PNG output required")?;
    let tab:i32=args.get(2).ok_or("tab 0..6 required")?.parse()?;
    let width:u32=args.get(3).map(|s|s.parse()).transpose()?.unwrap_or(960);
    let height:u32=args.get(4).map(|s|s.parse()).transpose()?.unwrap_or(900);
    if !(0..=6).contains(&tab) || !(340..=1920).contains(&width) || !(600..=1400).contains(&height){return Err("invalid preview arguments".into());}
    let window=MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Headless(window.clone())))?;
    let app=Preview::new()?;
    app.set_viewport_width(width as i32);app.set_viewport_height(height as i32);
    app.global::<Theme>().set_dark(args.get(5).map(String::as_str)==Some("dark"));
    app.global::<StudioLive>().set_studio_tab(tab);
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
        app.global::<StudioLive>().set_studio_tab(0);
        let mut changed=false;
        'find_tab: for y in (110..300).step_by(10) {
            for x in (24..width.min(420)).step_by(24) {
                let position=LogicalPosition::new(x as f32,y as f32);
                app.window().dispatch_event(WindowEvent::PointerPressed{position,button:PointerEventButton::Left});
                app.window().dispatch_event(WindowEvent::PointerReleased{position,button:PointerEventButton::Left});
                if app.global::<StudioLive>().get_studio_tab()==1 {changed=true;break 'find_tab;}
            }
        }
        assert!(changed,"pointer input must navigate to the avatar tab");
        app.global::<StudioLive>().set_studio_tab(tab);
        app.window().request_redraw();
        window.draw_if_needed(|renderer|{renderer.render(&mut pixels,width as usize);});
        println!("LIVE_STUDIO_TAB_INPUT=PASS actual pointer events");
    }
    let bytes:Vec<u8>=pixels.iter().flat_map(|p|[p.r,p.g,p.b]).collect();
    let mut encoder=png::Encoder::new(std::fs::File::create(output)?,width,height);
    encoder.set_color(png::ColorType::Rgb);encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&bytes)?;
    assert_eq!(app.global::<StudioLive>().get_studio_tab(),tab);
    println!("LIVE_STUDIO_RENDER=PASS tab={tab} size={width}x{height}; actual native component, no backend calls");
    Ok(())
}
