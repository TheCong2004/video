//! Small headless check of the actual changed components, without unrelated routes.
use std::{cell::Cell, rc::Rc};
use slint::{ComponentHandle, LogicalPosition, PhysicalSize, Rgb8Pixel};
use slint::platform::{WindowEvent, PointerEventButton, software_renderer::{MinimalSoftwareWindow, RepaintBufferType}};
slint::include_modules!();
struct Headless(Rc<MinimalSoftwareWindow>);
impl slint::platform::Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> { Ok(self.0.clone()) }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    let mode = args.get(1).ok_or("mode required")?;
    let dest = args.get(2).ok_or("PNG path required")?;
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Headless(window.clone())))?;
    let app = Preview::new()?;
    let width = args.get(3).and_then(|s| s.parse::<u32>().ok()).unwrap_or(1080); let height = args.get(4).and_then(|s| s.parse::<u32>().ok()).unwrap_or(1000);
    let fixture = std::env::var_os("CONCAT_PREVIEW_FIXTURE_IMAGE").and_then(|p| slint::Image::load_from_path(std::path::Path::new(&p)).ok()).unwrap_or_default();
    app.set_viewport_width(width as f32); app.set_viewport_height(height as f32);
    app.global::<Theme>().set_dark(false);
    let calls = Rc::new(Cell::new(0));
    if mode == "library" {
        app.set_library(true);
        let ui = app.global::<StudioBulk>();
        ui.set_library_status("12 video kiểm tra bố cục · không phải dữ liệu thư viện người dùng".into());
        let rows: Vec<_> = (0..12).map(|i| StudioVideoItem { title: format!("Video đã lưu {} · ảnh kiểm tra", i+1).into(), path: format!("D:\\Videos\\video-{i}.mp4").into(), detail: "Tải trực tuyến · 1.8 MB".into(), thumbnail: fixture.clone(), duration: "0:12".into() }).collect();
        ui.set_selected_video(rows[0].clone());
        ui.set_videos(slint::ModelRc::new(slint::VecModel::from(rows)));
        let count = calls.clone();
        ui.on_video_action(move |action, _| { if action == "import" { count.set(count.get() + 1); } });
    } else {
        let ui = app.global::<StudioLux>();
        ui.set_open(true); ui.set_url("https://www.youtube.com/watch?v=fixture".into());
        ui.set_has_info(true); ui.set_video_title("Video kiểm tra giao diện · không gọi mạng".into()); ui.set_video_site("YouTube".into());
        ui.set_stream_options(slint::ModelRc::new(slint::VecModel::from(vec!["Tự động · hình và âm thanh".into()])));
        if mode == "selection" || mode == "selection-busy" {
            ui.set_is_playlist(true); ui.set_can_browser(true);
            ui.set_hide_existing(false);
            ui.set_url("https://www.tiktok.com/@account".into());
            ui.set_selected_count(10);
            ui.set_total_count(100); ui.set_existing_count(1); ui.set_choice_pages(5);
            ui.set_choices(slint::ModelRc::new(slint::VecModel::from((0..20).map(|index| LuxChoice {
                index, title: format!("Video {} · danh sách tài khoản", index + 1).into(),
                detail: if index == 0 { "Đã có trên máy · bỏ qua tải trùng".into() } else { format!("https://www.tiktok.com/@account/video/{index}").into() },
                selected: index > 0 && index <= 10, exists: index == 0,
                thumbnail: fixture.clone(), duration: "0:12".into(),
            }).collect::<Vec<_>>())));
            ui.set_status("Đọc được 100 video · 1 đã có · 10 đã chọn".into());
            if mode == "selection-busy" { ui.set_downloading(true); }
            let count = calls.clone(); ui.on_choose_first(move |n| { assert_eq!(n, 10); count.set(count.get() + 1); });
        }
        else if mode == "empty" { ui.set_is_playlist(true); ui.set_has_info(false); }
        else if mode == "all-existing" { ui.set_is_playlist(true); ui.set_has_info(true); ui.set_total_count(10); ui.set_existing_count(10); }
        else if mode == "error" { ui.set_error("Không đọc được danh sách video công khai của hồ sơ TikTok này. Thử liên kết một video cụ thể.".into()); }
        else { ui.set_downloading(true); ui.set_progress(0.35); ui.set_status("Đang tải: 35%".into()); }
        let count = calls.clone(); ui.on_cancel_download(move || count.set(count.get() + 1));
    }
    app.show()?; window.set_size(PhysicalSize::new(width, height)); app.window().request_redraw();
    let mut buffer = vec![Rgb8Pixel::default(); (width * height) as usize];
    window.draw_if_needed(|r| { r.render(&mut buffer, width as usize); });
    if mode != "error" && mode != "empty" && mode != "all-existing" {
        'click: for y in (80..height - 20).step_by(8) { for x in (20..width - 20).step_by(8) {
            let position = LogicalPosition::new(x as f32, y as f32);
            app.window().dispatch_event(WindowEvent::PointerPressed { position, button: PointerEventButton::Left });
            app.window().dispatch_event(WindowEvent::PointerReleased { position, button: PointerEventButton::Left });
            if calls.get() > 0 { break 'click; }
        }}
        assert_eq!(calls.get(), 1, "Expected native button callback must be reachable exactly once");
    }
    let file = std::fs::File::create(dest)?;
    let mut encoder = png::Encoder::new(file, width, height); encoder.set_color(png::ColorType::Rgb); encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    let bytes = buffer.iter().flat_map(|p| [p.r,p.g,p.b]).collect::<Vec<_>>(); writer.write_image_data(&bytes)?;
    println!("NATIVE_UI={mode}:PASS; callbacks={}; no network or user project mutations", calls.get());
    Ok(())
}
