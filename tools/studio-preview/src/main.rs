//! Offscreen rendering of the real native components, not an HTML mock.
use std::rc::Rc;
use std::cell::RefCell;
use slint::{ComponentHandle, PhysicalSize, Rgb8Pixel};
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
slint::include_modules!();

struct Headless(Rc<MinimalSoftwareWindow>, Rc<RefCell<String>>);
impl slint::platform::Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> { Ok(self.0.clone()) }
    fn set_clipboard_text(&self, text: &str, _: slint::platform::Clipboard) { *self.1.borrow_mut()=text.to_string(); }
    fn clipboard_text(&self, _: slint::platform::Clipboard) -> Option<String> { Some(self.1.borrow().clone()) }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let output = args.get(1).ok_or("output PNG path required")?;
    let route = match args.get(2).map(String::as_str).unwrap_or("production") {
        "production" => StudioRoute::Production, "overview" => StudioRoute::Overview,
        "scripts" => StudioRoute::Scripts, "jobs" => StudioRoute::Jobs,
        "keys" | "keys-sample" | "keys-form" | "ai-edit" => StudioRoute::Keys,
        "settings" => StudioRoute::Settings,
        _ => return Err("unsupported preview route".into()),
    };
    let width: u32 = args.get(3).map(|s|s.parse()).transpose()?.unwrap_or(1440);
    let height: u32 = args.get(4).map(|s|s.parse()).transpose()?.unwrap_or(1000);
    if !(320..=2400).contains(&width) || !(480..=2000).contains(&height) { return Err("invalid viewport".into()); }
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    let clipboard=Rc::new(RefCell::new(String::new()));
    slint::platform::set_platform(Box::new(Headless(window.clone(), clipboard.clone())))?;
    let app = Preview::new()?;
    let words: std::collections::HashMap<String,serde_json::Value> = serde_json::from_str(include_str!("../../../src/crates/concat/locales/vi.json"))?;
    let words = Rc::new(words);
    let dict = words.clone();
    app.global::<I18n>().on_lookup(move |_,key| dict.get(key.as_str()).and_then(|v|v.as_str()).unwrap_or(key.as_str()).into());
    let dict = words.clone();
    app.global::<I18n>().on_lookup1(move |_,key,a| dict.get(key.as_str()).and_then(|v|v.as_str()).unwrap_or(key.as_str()).replace("{0}",&a).into());
    app.global::<I18n>().on_lookup2(move |_,key,a,b| words.get(key.as_str()).and_then(|v|v.as_str()).unwrap_or(key.as_str()).replace("{0}",&a).replace("{1}",&b).into());
    app.set_route(route);
    if args.get(2).map(String::as_str) == Some("ai-edit") {
        app.set_editing(true);
        let ai = app.global::<AiEditing>();
        ai.set_project_open(true);
        ai.set_project_info("Timeline mẫu: 2 clip · 2 tư liệu · không gọi AI".into());
    }
    if args.get(2).map(String::as_str) == Some("keys-form") {
        let ai = app.global::<StudioAi>();
        ai.set_new_open(true);
        ai.set_new_provider("gemini".into());
        ai.set_new_name("Kết nối mẫu · không phải key thật".into());
    }
    if args.get(2).map(String::as_str) == Some("keys-sample") {
        // Explicit synthetic layout fixture. Never calls providers or reads keys.
        let ai=app.global::<StudioAi>();
        ai.set_providers_loaded(true);
        ai.set_providers(slint::ModelRc::new(slint::VecModel::from(vec![
            StudioProviderRow {id:"fixture-connection-a".into(),name:"Kết nối mẫu · không phải key thật".into(),provider:"groq".into(),active:true,model:"fixture/model".into()},
            StudioProviderRow {id:"fixture-connection-b".into(),name:"Kết nối mẫu đã tắt".into(),provider:"gemini".into(),active:false,model:"".into()},
        ])));
        ai.set_models_loaded(true);
        ai.set_models(slint::ModelRc::new(slint::VecModel::from(vec![slint::SharedString::from("fixture/model")])));
        ai.set_answer("Đây là nội dung giả lập để kiểm tra bố cục Slint, không phải phản hồi từ nhà cung cấp. Các thao tác thật chỉ chạy trong ứng dụng sau khi người dùng xác nhận.".into());
        ai.set_answered_model("fixture/model · dữ liệu giả lập".into());
    }
    app.set_viewport_width(width as i32);
    app.set_viewport_height(height as i32);
    app.show()?;
    window.set_size(PhysicalSize::new(width,height));
    app.window().request_redraw();
    let mut buffer = vec![Rgb8Pixel::default(); (width*height) as usize];
    if !window.draw_if_needed(|renderer| { renderer.render(&mut buffer,width as usize); }) { return Err("no rendered frame".into()); }
    if args.get(5).map(String::as_str)==Some("input-smoke") {
        if width!=360 || height!=900 || !app.get_editing() { return Err("input fixture requires ai-edit 360 900".into()); }
        use slint::{LogicalPosition, platform::{WindowEvent, PointerEventButton}};
        let click=|x,y| {
            let position=LogicalPosition::new(x,y);
            app.window().dispatch_event(WindowEvent::PointerPressed {position,button:PointerEventButton::Left});
            app.window().dispatch_event(WindowEvent::PointerReleased {position,button:PointerEventButton::Left});
        };
        let typed="Cắt 2 giây đầu clip";
        // Find the editable prompt using native pointer routing; don't depend
        // on a particular font's line height or translated paragraph wrapping.
        for y in (250..480).step_by(8) {
            click(48.,y as f32);
            for c in typed.chars() {
                let text=slint::SharedString::from(c.to_string());
                app.window().dispatch_event(WindowEvent::KeyPressed {text:text.clone()});
                app.window().dispatch_event(WindowEvent::KeyReleased {text});
            }
            if !app.global::<AiEditing>().get_prompt().is_empty() { break; }
        }
        assert_eq!(app.global::<AiEditing>().get_prompt().as_str(),typed,"typed prompt must reach request state");
        let weak=app.as_weak();
        app.global::<AiEditing>().on_prepare_web_context(move || weak.unwrap().global::<AiEditing>().set_web_context("fixture-context-not-a-provider-call".into()));
        for y in (350..720).step_by(6) {
            click(120.,y as f32);
            if clipboard.borrow().as_str()=="fixture-context-not-a-provider-call" { break; }
        }
        assert_eq!(&*clipboard.borrow(),"fixture-context-not-a-provider-call","copy uses synchronously prepared context");
        println!("INPUT_SMOKE=PASS (real native pointer/key events, prompt state and isolated clipboard; no website/provider)");
    }
    let bytes: Vec<u8> = buffer.iter().flat_map(|p|[p.r,p.g,p.b]).collect();
    let file = std::fs::File::create(output)?;
    let mut encoder = png::Encoder::new(file,width,height);
    encoder.set_color(png::ColorType::Rgb); encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&bytes)?;
    println!("Rendered {output} ({width}x{height}); disconnected state, no backend calls.");
    Ok(())
}
