//! Render the production native Factory, with empty real state and no backend fixtures.
use std::rc::Rc;
use slint::{ComponentHandle, PhysicalSize, Rgb8Pixel};
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
slint::include_modules!();
struct Headless(Rc<MinimalSoftwareWindow>);
impl slint::platform::Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> { Ok(self.0.clone()) }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let output = args.get(1).ok_or("output PNG required")?;
    let width: u32 = args.get(2).ok_or("width required")?.parse()?;
    let height: u32 = args.get(3).ok_or("height required")?.parse()?;
    let step: i32 = args.get(4).ok_or("step required")?.parse()?;
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Headless(window.clone())))?;
    let app = Preview::new()?;
    app.global::<Theme>().set_dark(args.iter().any(|s| s == "dark"));
    app.global::<Theme>().set_reduced_motion(true);
    app.global::<KediFactory>().set_step(step);
    if let Some(source) = args.iter().find_map(|a| a.strip_prefix("source=")) {
        let data: serde_json::Value = serde_json::from_slice(&std::fs::read(source)?)?;
        app.global::<KediFactory>().set_product_title(data["title_hint"].as_str().unwrap_or("").into());
        app.global::<KediFactory>().set_product_url_input(data["source_url"].as_str().unwrap_or("").into());
    }
    if let Some(path) = args.iter().find_map(|a| a.strip_prefix("image=")) {
        let path = std::path::Path::new(path);
        if !path.is_file() { return Err("actual imported image required".into()); }
        app.global::<KediFactory>().set_product_image(slint::Image::load_from_path(path)?);
        app.global::<KediFactory>().set_asset_count(1);
        app.global::<KediFactory>().set_local_paths_display(path.display().to_string().into());
    }
    app.window().set_size(PhysicalSize::new(width,height));
    app.show()?;
    let mut buffer = vec![Rgb8Pixel::default(); (width*height) as usize];
    let draw = |buffer: &mut Vec<Rgb8Pixel>| {
        slint::platform::update_timers_and_animations();
        app.window().request_redraw();
        window.draw_if_needed(|renderer| { renderer.render(buffer, width as usize); });
    };
    draw(&mut buffer);
    if args.iter().any(|s| s == "input-test") {
        use slint::{LogicalPosition, platform::{WindowEvent, PointerEventButton}};
        let click = |x,y| {
            let position = LogicalPosition::new(x,y);
            app.window().dispatch_event(WindowEvent::PointerPressed { position, button: PointerEventButton::Left });
            app.window().dispatch_event(WindowEvent::PointerReleased { position, button: PointerEventButton::Left });
        };
        // The three step actions have separate native hit targets.
        assert_eq!(app.global::<KediFactory>().get_asset_count(), 0);
        assert_eq!(app.global::<KediFactory>().get_selected_count(), 0);
        assert!(app.global::<KediFactory>().get_product_title().is_empty());
        'product: for y in (60..200).step_by(4) { for x in (180..400).step_by(8) {
            click(x as f32,y as f32); draw(&mut buffer);
            if app.global::<KediFactory>().get_step() == 1 { break 'product; }
        }}
        assert_eq!(app.global::<KediFactory>().get_step(), 1, "product step has a native hit target");
        // Typing reaches real application state; there is no seeded product.
        let value = "Sản phẩm thật";
        'field: for y in (260..600).step_by(6) {
            click(100., y as f32);
            for ch in value.chars() {
                let text = ch.to_string().into();
                app.window().dispatch_event(WindowEvent::KeyPressed { text });
            }
            if !app.global::<KediFactory>().get_product_url_input().is_empty() { break 'field; }
        }
        assert_eq!(app.global::<KediFactory>().get_product_url_input().as_str(), value);
        app.global::<KediFactory>().set_product_url_input("".into());
        app.global::<KediFactory>().set_step(step);
        println!("FACTORY_INPUT=PASS empty_state=real steps=separate unicode_input=state");
    }
    draw(&mut buffer);
    let bytes: Vec<u8> = buffer.iter().flat_map(|p| [p.r,p.g,p.b]).collect();
    let mut encoder = png::Encoder::new(std::fs::File::create(output)?, width,height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&bytes)?;
    println!("FACTORY_RENDER=PASS {width}x{height} step={step} {output}");
    Ok(())
}
