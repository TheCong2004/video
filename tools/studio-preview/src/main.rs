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
        "production-scenes" | "production-director" | "production-render" | "kedi-shell" | "kedi-sidebar" | "launcher-resize" | "launcher-production" | "production" | "production-local" | "production-script" | "production-assets" | "production-anchor" | "production-visuals" | "production-voice" | "production-export" | "production-busy" | "production-error" | "production-completed" => StudioRoute::Production,
        "film" => StudioRoute::Film, "scripts" => StudioRoute::Scripts, "jobs" => StudioRoute::Jobs,
        "history" => StudioRoute::History,
        "hotclip-source" | "hotclip-transcript" => StudioRoute::Production,
        "keys" | "keys-sample" | "keys-form" | "ai-edit" | "ai-edit-api" | "ai-edit-manual" => StudioRoute::Keys,
        "settings" => StudioRoute::Settings,
        "live" | "live-kedi" | "live-connected" | "live-loading" | "live-error" | "live-partial" | "live-advanced" | "live-voice" => StudioRoute::Live,
        "workflows" => StudioRoute::Workflows, "bulk" => StudioRoute::Bulk, "channels" => StudioRoute::Channels, "publish" => StudioRoute::Publish,
        "auto-shorts" | "shorts-short" | "shorts-empty" | "shorts-busy" | "shorts-result" | "shorts-advanced" | "shorts-transcript" => StudioRoute::AutoShorts,
        "mc" | "promax-avatars" | "promax-offline" | "avatar-picker-input" => StudioRoute::Opentalking,
        "production-create" | "production-navigation" | "lux-download" | "lux-error" | "lux-cancel" => StudioRoute::Production,
        "kedi-factory" | "factory-product" | "factory-export" => StudioRoute::KediFactory,
        "kedi-providers" => StudioRoute::KediProviders,
        _ => return Err("unsupported preview route".into()),
    };
    let width: u32 = args.get(3).map(|s|s.parse()).transpose()?.unwrap_or(1440);
    let height: u32 = args.get(4).map(|s|s.parse()).transpose()?.unwrap_or(1000);
    if !(320..=2400).contains(&width) || !(480..=2000).contains(&height) { return Err("invalid viewport".into()); }
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    let clipboard=Rc::new(RefCell::new(String::new()));
    slint::platform::set_platform(Box::new(Headless(window.clone(), clipboard.clone())))?;
    let app = Preview::new()?;
    if args[2].starts_with("lux-") {
        app.set_lux_dialog(true);
        let ui = app.global::<StudioLux>();
        ui.set_open(true); ui.set_url("https://www.youtube.com/watch?v=fixture".into());
        ui.set_has_info(true); ui.set_video_title("Video kiểm tra giao diện · không gọi mạng".into());
        ui.set_video_site("YouTube".into());
        ui.set_stream_options(slint::ModelRc::new(slint::VecModel::from(vec!["Tự động · hình và âm thanh".into()])));
        if args[2] == "lux-error" { ui.set_error("Không đọc được danh sách video công khai của hồ sơ TikTok này. Thử liên kết một video cụ thể.".into()); }
        else { ui.set_downloading(true); ui.set_progress(0.35); ui.set_status("Đang tải: 35%".into()); }
    }
    if args[2] == "bulk" {
        let ui = app.global::<StudioBulk>();
        ui.set_library_status("2 video trên máy · mới nhất ở trên".into());
        ui.set_videos(slint::ModelRc::new(slint::VecModel::from(vec![
            StudioVideoItem { title: "Phim đã dựng.mp4".into(), path: "D:\\Videos\\Page A\\pipeline-123\\assembly\\film.mp4".into(), detail: "Xưởng phim AI · 24.5 MB".into(), ..Default::default() },
            StudioVideoItem { title: "Video tải trực tuyến.mp4".into(), path: "D:\\Videos\\downloads\\download-456\\video.mp4".into(), detail: "Tải trực tuyến · 8.2 MB".into(), ..Default::default() },
        ])));
    }
    let words: std::collections::HashMap<String,serde_json::Value> = serde_json::from_str(include_str!("../../../src/crates/concat/locales/vi.json"))?;
    let words = Rc::new(words);
    let dict = words.clone();
    app.global::<I18n>().on_lookup(move |_,key| dict.get(key.as_str()).and_then(|v|v.as_str()).unwrap_or(key.as_str()).into());
    let dict = words.clone();
    app.global::<I18n>().on_lookup1(move |_,key,a| dict.get(key.as_str()).and_then(|v|v.as_str()).unwrap_or(key.as_str()).replace("{0}",&a).into());
    app.global::<I18n>().on_lookup2(move |_,key,a,b| words.get(key.as_str()).and_then(|v|v.as_str()).unwrap_or(key.as_str()).replace("{0}",&a).replace("{1}",&b).into());
    app.set_route(route);
    if args[2] == "factory-product" { app.global::<KediFactory>().set_step(1); }
    if args[2] == "factory-export" { app.global::<KediFactory>().set_step(2); }
    if args[2] == "live-kedi" {
        let live=app.global::<StudioLive>();
        live.set_kedi_available(true);live.set_kedi_state("Live".into());live.set_kedi_automatic(true);
        live.set_kedi_status("Mô phỏng offline · 0 tác vụ · 0 ms chờ · 0 lỗi".into());
        live.set_kedi_reply("Xoài sấy demo hiện có giá 89.000đ theo dữ liệu mock.".into());
    }
    app.set_hotclip(args[2].starts_with("hotclip-"));
    app.set_launcher(args[2] == "launcher-production" || args[2] == "launcher-resize" || args[2] == "kedi-sidebar" || args[2] == "kedi-shell");
    if args[2] == "launcher-production" {
        app.global::<LauncherUi>().set_sidebar_mode(1);
    }
    app.global::<Theme>().set_dark(args.iter().any(|arg| arg == "dark"));
    app.global::<Theme>().set_reduced_motion(true);
    let weak = app.as_weak();
    app.global::<LauncherUi>().on_sidebar_mode_changed(move |mode| {
        weak.unwrap().global::<LauncherUi>().set_sidebar_mode(mode);
    });
    let weak = app.as_weak();
    app.global::<LauncherUi>().on_theme_changed(move |dark| {
        weak.unwrap().global::<Theme>().set_dark(dark);
    });
    let weak = app.as_weak();
    app.global::<LauncherUi>().on_reduced_motion_changed(move |reduced| {
        weak.unwrap().global::<Theme>().set_reduced_motion(reduced);
    });
    if args.get(2).is_some_and(|mode| mode.starts_with("production")) {
        let state = app.global::<ProductionStudio>();
        let phase = match args[2].as_str() {
            "production-script" | "production-scenes" => 2, "production-assets" => 3,
            "production-director" => 4, "production-render" | "production-create" => 7,
            "production-anchor" => 4, "production-visuals" => 5,
            "production-voice" => 6, "production-export" | "production-busy" | "production-error" | "production-completed" => 7,
            _ => 1,
        };
        state.set_current_phase(phase);
        if args[2] == "production-create" {
            state.set_workflow_ids(slint::ModelRc::new(slint::VecModel::from(vec!["concat.native_production.v1".into()])));
            state.set_workflow_names(slint::ModelRc::new(slint::VecModel::from(vec!["Native fixture — no providers".into()])));
            state.set_page_names(slint::ModelRc::new(slint::VecModel::from(vec!["Select Page".into(), "Fixture Page".into()])));
            state.set_page_index(1);
            // Reproduce stale selections from the report: the explicit native
            // graph must show its fixed Grok/9:16 values, not these old values.
            state.set_aspect_index(1);
            state.set_visual_engine_index(0);
            state.set_status("Fixture: no network or generation requests.".into());
        }
        if ["production-scenes","production-director","production-render"].contains(&args[2].as_str()) {
            let scenes=app.global::<ProductionScenes>();
            scenes.set_count(6);scenes.set_selected(0);scenes.set_title("Mở đầu câu chuyện".into());scenes.set_duration(8.0);
            scenes.set_summary("Dữ liệu kiểm tra bố cục · 6 cảnh · 48 giây".into());
            scenes.set_narration("Xin chào, đây là câu chuyện của nhân vật chính.".into());
            scenes.set_background_label("Nền: canh-mo-dau.png".into());scenes.set_character_label("Nhân vật: nhan-vat.png".into());
            scenes.set_voice_label("Giọng đọc: loi-dan.wav".into());scenes.set_music_label("Nhạc: nhac-nen.mp3".into());
            scenes.set_setting("Thành phố lúc bình minh".into());scenes.set_lighting("Nắng ấm, tương phản mềm".into());scenes.set_expression("Tự nhiên".into());
            scenes.set_qa("Kiểm tra bố cục · Cảnh 3 thiếu giọng đọc; cảnh 5 chưa có bản xem thử.".into());
            scenes.set_rows(slint::ModelRc::new(slint::VecModel::from((0..6).map(|i|SceneCardRow{index:i,id:format!("fixture-{i}").into(),title:format!("Cảnh {} · câu chuyện",i+1).into(),narration:"Dữ liệu kiểm tra, không phải kết quả tạo video.".into(),duration:"8.0s".into(),status:if i==2{"Thiếu giọng".into()}else{"Có preview".into()},warning:i==2,thumbnail:Default::default()}).collect::<Vec<_>>())));
        }
        state.set_brief("Câu chuyện 60 giây về một nhân vật, giữ nhận diện xuyên suốt các cảnh.".into());
        if args[2] == "production-local" {
            state.set_page_names(slint::ModelRc::new(slint::VecModel::from(vec!["Chọn Page…".into(), "Kênh mẫu · chỉ kiểm tra bố cục".into()])));
            state.set_page_index(1);
            state.set_page_summary("Kênh mẫu · Hồ sơ Nexora: fixture-profile\nLưu tại: thư mục kiểm thử, không phải dữ liệu người dùng".into());
            state.set_visual_engine_index(1);
            state.set_local_progress("Dữ liệu bố cục kiểm thử, không tạo nội dung hay gửi mạng.".into());
            state.set_local_image_status("Đã nhận và kiểm tra file · fixture".into());
            state.set_local_expand_status("Đang chạy · fixture".into());
        }
        if args[2] == "production-anchor" {
            // A neutral fixture image, not a user upload or generated character.
            state.set_anchor_label("anh-tham-chieu-layout.png · ảnh mẫu kiểm tra giao diện".into());
            let pixels = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&[75, 90, 110, 255].repeat(180*180), 180, 180);
            state.set_anchor_preview(slint::Image::from_rgba8(pixels));
        }
        if args[2] == "production-busy" { state.set_busy(true); state.set_status("Đang kiểm tra cấu hình… (dữ liệu kiểm tra bố cục)".into()); }
        if args[2] == "production-error" { state.set_uncertain(true); state.set_status("Chưa xác định được kết quả gửi. Kiểm tra tác vụ trước khi tạo lại. (Dữ liệu kiểm tra bố cục)".into()); }
        if args[2] == "production-completed" {
            state.set_runs_loaded(true);
            state.set_runs(slint::ModelRc::new(slint::VecModel::from(vec![StudioRunRow {
                id: "layout-fixture-not-a-real-job".into(), workflow: "Quy trình mẫu kiểm tra bố cục".into(), status: "completed".into(), node: "capcut".into(), created: "Dữ liệu kiểm tra".into(), completed: 5, total: 5,
            }])));
        }
    }
    if args.get(2).is_some_and(|mode| mode.starts_with("shorts-") || mode == "hotclip-transcript") {
        let state=app.global::<AutoShorts>();
        state.set_has_source(true);
        state.set_source_label("video-mau-layout.mp4".into());
        state.set_source_duration(if args[2]=="shorts-short" {5.0} else {1200.0});
        state.set_source_info(if args[2]=="shorts-short" {"1024 × 576 · 5 giây · dữ liệu bố cục"} else {"1920 × 1080 · 20 phút · dữ liệu bố cục"}.into());
        if args[2]=="shorts-empty" {
            state.set_analyzed(true);
            state.set_status("Chưa tìm được đoạn phù hợp. Thử giảm độ dài tối thiểu hoặc chọn video dài hơn; chưa tạo video mới.".into());
            state.set_analysis_note("Chưa có bộ nhận giọng nói: chưa chọn đoạn theo lời thoại. Có thể nhập phụ đề để phân tích lời nói.".into());
            state.set_diagnostics("Explicit layout fixture: original technical warnings remain here, not on the primary surface.".into());
        }
        if args[2]=="shorts-busy" {state.set_busy(true); state.set_status("Đang đọc video…".into());state.set_progress(0.4);}
        if args[2]=="shorts-advanced" {state.set_advanced(true);state.set_ai_mode_index(1);}
        if args[2]=="shorts-transcript" || args[2]=="hotclip-transcript" {
            state.set_analyzed(true); state.set_cue_count(2);
            state.set_transcript_page("2 câu nguồn · 2 câu khớp · trang 1/1 · dữ liệu kiểm tra bố cục".into());
            state.set_transcript_has_range(true);
            state.set_transcript_range("Vùng chọn: 1.00–20.00s · 19 giây".into());
            state.set_cue_editor_label("Sửa câu 1.00–4.00s · tối đa 180 ký tự".into());
            state.set_cue_editor_text("Xin chào Việt Nam!".into());
            state.set_transcript_rows(slint::ModelRc::new(slint::VecModel::from(vec![
                TranscriptRow {id:"9".into(), time:"1.00–4.00s".into(), text:"Xin chào Việt Nam! Đây là câu dài để kiểm tra xuống dòng trên màn hình nhỏ, không phải lời thoại lấy từ video thật.".into(), marker:"Bắt đầu".into()},
                TranscriptRow {id:"2".into(), time:"5.00–20.00s".into(), text:"Câu kết thúc mẫu kiểm tra bố cục.".into(), marker:"Kết thúc".into()},
            ])));
        }
        if args[2]=="shorts-result" {
            state.set_analyzed(true);state.set_result_count(1);
            state.set_status("Tìm được 1 đoạn. Xem lại, chọn đoạn muốn giữ rồi xuất thành video riêng.".into());
            state.set_rows(slint::ModelRc::new(slint::VecModel::from(vec![ShortsRow {
                id:"fixture-clip".into(),title:"Đoạn mẫu kiểm tra bố cục".into(),hook:"".into(),reason:"Đây là dữ liệu bố cục, không phải kết quả phân tích thật.".into(),range:"00:20 → 00:45 · 25 giây".into(),score:"".into(),review:"Xem lại khung hình trước khi xuất.".into(),status:"Chưa xuất".into(),selected:false,thumbnail:Default::default(),
            }])));
        }
    }
    if args.get(2).is_some_and(|value| value == "mc") {
        let talking = app.global::<StudioOpenTalking>();
        let pixels = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&[45, 55, 70, 255].repeat(1024*768), 1024, 768);
        talking.set_gesture_image(slint::Image::from_rgba8(pixels));
        talking.set_gesture_image_name("Ảnh thử bố cục 1024×768 · không phải ảnh người dùng".into());
        talking.set_gesture_status("Dữ liệu thử giao diện; chưa gửi ảnh, âm thanh hay chạy model.".into());
    }
    if args.get(2).is_some_and(|value| value.starts_with("live")) {
        let live = app.global::<StudioLive>();
        let talking = app.global::<StudioOpenTalking>();
        let ai = app.global::<StudioAi>();
        talking.set_catalogs_loaded(true);
        app.global::<StudioPromaxAvatars>().set_loaded(true);
        ai.set_models_loaded(true);
        live.set_username("@layout_fixture".into());
        if args[2] != "live" && args[2] != "live-loading" {
            talking.set_avatar_options(slint::ModelRc::new(slint::VecModel::from(vec!["Nhân vật mẫu có tên dài để kiểm tra độ vừa khung".into()])));
            talking.set_voice_options(slint::ModelRc::new(slint::VecModel::from(vec!["Tiếng Việt · Giọng mẫu".into()])));
            let pixels = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&[45, 55, 70, 255].repeat(180*320), 180, 320);
            // Explicit neutral layout swatch; not a real character or runtime readiness proof.
            talking.set_avatar_preview(slint::Image::from_rgba8(pixels));
        }
        if args[2] == "live-connected" || args[2] == "live-advanced" {
            live.set_state("connected".into());
            live.set_motion_playing(true);
            ai.set_models(slint::ModelRc::new(slint::VecModel::from(vec!["fixture/model".into()])));
            live.set_comments(slint::ModelRc::new(slint::VecModel::from(vec![
                StudioLiveCommentRow {event_id:"fixture-chat".into(),kind:"chat".into(),username:"Người xem mẫu với tên dài".into(),content:"Đây là bình luận mẫu rất dài để kiểm tra xuống dòng, không phải dữ liệu người dùng hay phản hồi từ TikTok.".into(),time:"12:34".into()},
                StudioLiveCommentRow {event_id:"fixture-ai".into(),kind:"ai".into(),username:"AI".into(),content:"Phản hồi bố cục mẫu, không gọi nhà cung cấp.".into(),time:"12:35".into()},
            ])));
        }
        if args[2] == "live-loading" { live.set_state("connecting".into()); live.set_busy(true); talking.set_busy(true); live.set_motion_loading(true); }
        if args[2] == "live-error" { live.set_state("failed".into()); live.set_status("Không kết nối được TikTok. Kiểm tra mạng rồi thử lại. [FIXTURE_ERROR]".into()); }
        if args[2] == "live-advanced" { live.set_show_settings(true); }
        if args[2] == "live-voice" {
            // Layout fixture only. No actual server connection or sample upload.
            live.set_voice_server_enabled(true);
            live.set_voice_sample_mode(true);
            live.set_voice_server_names(slint::ModelRc::new(slint::VecModel::from(vec!["Giọng mẫu của tôi · dữ liệu bố cục".into()])));
            live.set_reference_voice_name("Giọng thử bố cục".into());
            live.set_motion_title("video-mau-ui.mp4".into());
        }
    }
    if args.get(2).map(String::as_str) == Some("avatar-picker-input") {
        app.set_avatar_picker(true);
        let state = app.global::<StudioPromaxAvatars>();
        state.set_loaded(true);
        state.set_ready(true);
        state.set_status("Bộ quét sẵn sàng · chưa có nhân vật từ video của bạn.".into());
    }
    if matches!(args.get(2).map(String::as_str), Some("promax-avatars" | "promax-offline")) {
        app.set_avatar_panel(true);
        let state = app.global::<StudioPromaxAvatars>();
        state.set_loaded(true);
        state.set_endpoint("http://127.0.0.1:43100".into());
        if args.get(2).map(String::as_str) == Some("promax-offline") {
            state.set_error(true);
            state.set_status("Dữ liệu bố cục mẫu: Chưa kết nối Promax. Khởi động Promax BE đã cập nhật rồi bấm Tải lại.".into());
        } else {
            // Explicit layout fixture, not a backend/model verification.
            state.set_ready(true);
            state.set_name("Nhân vật mẫu để kiểm tra bố cục".into());
            state.set_file_label("video-mau-UI-khong-phai-du-lieu-ca-nhan.mp4".into());
            state.set_status("Dữ liệu bố cục mẫu: đã quét xong 50 khung hình. Không gọi backend trong chế độ xem trước này.".into());
            state.set_ready_options(slint::ModelRc::new(slint::VecModel::from(vec![slint::SharedString::from("Nhân vật mẫu UI")])));
            state.set_ready_ids(slint::ModelRc::new(slint::VecModel::from(vec![slint::SharedString::from("00000000-0000-4000-8000-000000000001")])));
            state.set_jobs(slint::ModelRc::new(slint::VecModel::from(vec![PromaxAvatarRow {id:"00000000-0000-4000-8000-000000000001".into(),name:"Nhân vật mẫu UI".into(),state:"Sẵn sàng".into(),details:"50 khung hình · 2 giây · Dữ liệu bố cục giả lập".into(),deletable:true}])));
        }
    }
    if args.get(2).is_some_and(|mode| mode.starts_with("ai-edit")) {
        app.set_editing(true);
        let ai = app.global::<AiEditing>();
        ai.set_project_open(true);
        ai.set_project_info("Timeline mẫu: 2 clip · 2 tư liệu · không gọi AI".into());
        if args[2] == "ai-edit-api" || args.get(5).map(String::as_str) == Some("input-smoke") { ai.set_external_mode(false); }
        if args[2] == "ai-edit-manual" { ai.set_web_import_open(true); }
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
    if args[2] == "production-navigation" {
        app.global::<ProductionStudio>().set_pages_loaded(true);
        app.global::<ProductionStudio>().set_page_index(1);
        app.global::<ProductionStudio>().set_page_names(slint::ModelRc::new(slint::VecModel::from(vec!["Select Page".into(), "Fixture Page".into()])));
    }
    if !window.draw_if_needed(|renderer| { renderer.render(&mut buffer,width as usize); }) { return Err("no rendered frame".into()); }
    if args[2] == "lux-cancel" {
        use std::cell::Cell;
        use slint::{LogicalPosition, platform::{WindowEvent, PointerEventButton}};
        let calls = Rc::new(Cell::new(0)); let count = calls.clone();
        app.global::<StudioLux>().on_cancel_download(move || count.set(count.get() + 1));
        'search: for y in (height / 2..height).step_by(8) {
            for x in (width / 2..width).step_by(8) {
                let position = LogicalPosition::new(x as f32, y as f32);
                app.window().dispatch_event(WindowEvent::PointerPressed { position, button: PointerEventButton::Left });
                app.window().dispatch_event(WindowEvent::PointerReleased { position, button: PointerEventButton::Left });
                if calls.get() > 0 { break 'search; }
            }
        }
        assert_eq!(calls.get(), 1, "Cancel button reaches cancellation callback while downloading");
        println!("LUX_CANCEL_CLICK=PASS; no network or user project mutations");
    }
    if args[2] == "production-navigation" {
        use std::cell::Cell;
        use slint::{LogicalPosition, platform::{WindowEvent, PointerEventButton}};
        let reloads = Rc::new(Cell::new(0));
        let count = reloads.clone();
        app.global::<ProductionStudio>().on_load_pages(move || count.set(count.get()+1));
        let mut follow = |from, to| {
            for x in (20..width.min(900)).step_by(8) {
                app.set_route(from);
                app.window().request_redraw();
                window.draw_if_needed(|renderer| { renderer.render(&mut buffer, width as usize); });
                let position = LogicalPosition::new(x as f32, 32.);
                app.window().dispatch_event(WindowEvent::PointerPressed { position, button: PointerEventButton::Left });
                app.window().dispatch_event(WindowEvent::PointerReleased { position, button: PointerEventButton::Left });
                if app.get_route() == to { return true; }
            }
            false
        };
        assert!(follow(StudioRoute::Production, StudioRoute::Jobs), "Tasks header reaches jobs");
        assert!(follow(StudioRoute::Jobs, StudioRoute::Production), "Back reaches production hub");
        assert_eq!(reloads.get(), 0, "Back must not reload Page catalogue");
        assert_eq!(app.global::<ProductionStudio>().get_page_index(), 1, "Back preserves selected Page");
        println!("PRODUCTION_NAVIGATION=PASS Tasks -> Back; Page retained; no auto reload or providers");
    }
    if args[2] == "production-create" {
        use std::cell::Cell;
        use slint::{LogicalPosition, platform::{WindowEvent, PointerEventButton}};
        let calls = Rc::new(Cell::new(0));
        let counter = calls.clone();
        let weak = app.as_weak();
        app.global::<ProductionStudio>().on_start_run(move || {
            counter.set(counter.get() + 1);
            let app = weak.unwrap();
            app.global::<ProductionStudio>().set_confirm_start(false);
            app.global::<ProductionStudio>().set_busy(true);
            app.global::<ProductionStudio>().set_status("Fixture: Create reached start handler once; no providers.".into());
        });
        assert!(app.global::<ProductionStudio>().get_native_workflow());
        let click = |x, y| {
            let position = LogicalPosition::new(x, y);
            app.window().dispatch_event(WindowEvent::PointerPressed { position, button: PointerEventButton::Left });
            app.window().dispatch_event(WindowEvent::PointerReleased { position, button: PointerEventButton::Left });
        };
        let mut hit = None;
        'search: for y in (440..height.min(1500)).step_by(12) {
            for x in (40..400).step_by(12) {
                click(x as f32, y as f32);
                if calls.get() == 1 { hit = Some((x, y)); break 'search; }
            }
        }
        let (x, y) = hit.ok_or("Create button did not reach native handler")?;
        app.window().request_redraw();
        window.draw_if_needed(|renderer| { renderer.render(&mut buffer, width as usize); });
        click(x as f32, y as f32);
        assert_eq!(calls.get(), 1, "busy guard prevents duplicate submission");
        println!("PRODUCTION_CREATE=PASS native pointer -> start handler once; duplicate disabled; stale aspect/provider retained but not used; no network");
    }
    if args[2] == "launcher-resize" {
        use slint::{LogicalPosition, platform::{WindowEvent, PointerEventButton}};
        let position = LogicalPosition::new(80., 234.);
        app.window().dispatch_event(WindowEvent::PointerPressed {position, button:PointerEventButton::Left});
        app.window().dispatch_event(WindowEvent::PointerReleased {position, button:PointerEventButton::Left});
        for _ in 0..3 {
            for (w,h) in [(1600,900),(1080,720),(940,720),(680,720),(650,720),(640,720),(360,900),(64,64),(1,1),(0,0),(1080,720)] {
                eprintln!("RESIZE_CHECK {w}x{h}");
                app.set_viewport_width(w as i32); app.set_viewport_height(h as i32);
                window.set_size(PhysicalSize::new(w,h));
                app.window().request_redraw();
                let mut pixels = vec![Rgb8Pixel::default(); (w.max(1)*h.max(1)) as usize];
                window.draw_if_needed(|renderer| {renderer.render(&mut pixels,w.max(1) as usize);});
            }
        }
        app.set_viewport_width(width as i32); app.set_viewport_height(height as i32);
        window.set_size(PhysicalSize::new(width,height));
        println!("LAUNCHER_RESIZE=PASS sidebar LIVE -> 33 resize/minimize/restore transitions; no network");
    }
    if args[2] == "kedi-sidebar" {
        use slint::{LogicalPosition, platform::{WindowEvent, PointerEventButton}};
        let draw = |pixels: &mut Vec<Rgb8Pixel>| {
            slint::platform::update_timers_and_animations();
            app.window().request_redraw();
            window.draw_if_needed(|renderer| { renderer.render(pixels, width as usize); });
        };
        let click = |x, y| {
            let position = LogicalPosition::new(x, y);
            app.window().dispatch_event(WindowEvent::PointerPressed { position, button: PointerEventButton::Left });
            app.window().dispatch_event(WindowEvent::PointerReleased { position, button: PointerEventButton::Left });
        };
        if width >= 1100 {
            assert!(app.get_sidebar_expanded(), "wide Auto reserves expanded sidebar");
            'collapse: for y in (16..60).step_by(8) {
                for x in (180..244).step_by(8) {
                    click(x as f32, y as f32);
                    draw(&mut buffer);
                    if app.global::<LauncherUi>().get_sidebar_mode() == 2 { break 'collapse; }
                }
            }
            assert_eq!(app.global::<LauncherUi>().get_sidebar_mode(), 2, "collapse button reaches preference callback");
            assert!(!app.get_sidebar_expanded(), "explicit collapse closes wide sidebar");
        } else if width >= 680 {
            assert!(!app.get_sidebar_expanded(), "medium Auto starts compact");
        }
        if width >= 680 {
            let reserved = app.get_sidebar_reserved_width();
            app.window().dispatch_event(WindowEvent::PointerMoved { position: LogicalPosition::new(34., 220.) });
            draw(&mut buffer);
            assert!(app.get_sidebar_expanded(), "hover opens actual rail");
            assert_eq!(app.get_sidebar_reserved_width(), reserved, "hover overlay never pushes form");
            app.window().dispatch_event(WindowEvent::PointerMoved { position: LogicalPosition::new(500., 220.) });
            std::thread::sleep(std::time::Duration::from_millis(460));
            draw(&mut buffer);
            assert!(!app.get_sidebar_expanded(), "leave collapses after delay");
            click(34., 90.);
            draw(&mut buffer);
            assert_eq!(app.global::<LauncherUi>().get_sidebar_mode(), 1, "pin click reaches persisted mode callback");
            assert!(app.get_sidebar_expanded(), "pin keeps rail open at supported width");
            click(100., 90.);
            draw(&mut buffer);
            assert_eq!(app.global::<LauncherUi>().get_sidebar_mode(), 0, "unpin returns to Auto without navigation");
        } else {
            'menu: for y in (8..52).step_by(8) {
                for x in (20..64).step_by(8) {
                    click(x as f32, y as f32);
                    draw(&mut buffer);
                    if app.get_sidebar_drawer_open() { break 'menu; }
                }
            }
            assert!(app.get_sidebar_drawer_open(), "menu button opens native drawer");
            app.window().dispatch_event(WindowEvent::KeyPressed { text: slint::platform::Key::Escape.into() });
            app.window().dispatch_event(WindowEvent::KeyReleased { text: slint::platform::Key::Escape.into() });
            draw(&mut buffer);
            assert!(!app.get_sidebar_drawer_open(), "Escape closes drawer");
            click(34., 28.);
            draw(&mut buffer);
            click(width as f32 - 8., 300.);
            draw(&mut buffer);
            assert!(!app.get_sidebar_drawer_open(), "backdrop closes drawer");
            for key in [" ", "\n"] {
                click(34., 28.);
                draw(&mut buffer);
                assert!(app.get_sidebar_drawer_open());
                app.window().dispatch_event(WindowEvent::KeyPressed { text: key.into() });
                app.window().dispatch_event(WindowEvent::KeyReleased { text: key.into() });
                draw(&mut buffer);
                assert!(!app.get_sidebar_drawer_open(), "Enter/Space activates actual focused drawer action");
            }
        }
        let before = buffer.clone();
        app.global::<Theme>().set_dark(!app.global::<Theme>().get_dark());
        draw(&mut buffer);
        assert_ne!(before, buffer, "theme switch changes real rendered pixels");
        app.global::<ProductionStudio>().set_brief("Bản nháp không mất khi đổi giao diện".into());
        app.global::<Theme>().set_dark(!app.global::<Theme>().get_dark());
        draw(&mut buffer);
        assert_eq!(app.global::<ProductionStudio>().get_brief().as_str(), "Bản nháp không mất khi đổi giao diện");
        assert!(app.global::<Theme>().get_reduced_motion());
        println!("KEDI_SIDEBAR=PASS native pointer/key routing, adaptive layout, preference callback, real theme pixels, draft retention; width={width}");
    }
    if args.get(5).map(String::as_str) == Some("kedi-motion") {
        assert_eq!(app.global::<Theme>().get_motion_hover(), 0, "safe default has zero-duration transitions");
        app.global::<Theme>().set_reduced_motion(false);
        assert_eq!(app.global::<Theme>().get_motion_hover(), 160, "opt-in enables bounded native hover transition");
        app.global::<Theme>().set_system_reduced_motion(true);
        assert_eq!(app.global::<Theme>().get_motion_hover(), 0, "system accessibility overrides opt-in");
        app.global::<Theme>().set_system_reduced_motion(false);
        app.global::<Theme>().set_reduced_motion(true);
        assert_eq!(app.global::<Theme>().get_motion_panel(), 0);
        println!("KEDI_MOTION=PASS actual native duration bindings: default reduced, opt-in, system override");
    }
    if args[2] == "launcher-production" {
        if width != 1080 { return Err("launcher regression requires 1080px wide".into()); }
        use slint::{LogicalPosition, platform::{WindowEvent, PointerEventButton}};
        let click = |x, y| {
            let position = LogicalPosition::new(x, y);
            app.window().dispatch_event(WindowEvent::PointerPressed {position, button:PointerEventButton::Left});
            app.window().dispatch_event(WindowEvent::PointerReleased {position, button:PointerEventButton::Left});
        };
        // Real StartScreen sidebar -> its internal route -> production form.
        click(80., 218.);
        app.window().request_redraw();
        window.draw_if_needed(|renderer| { renderer.render(&mut buffer,width as usize); });
        // Header tools are permanently visible now; locate the real step-2
        // button within the form instead of relying on an old fixed y value.
        'step: for y in (160..height.min(950)).step_by(12) {
            for x in (300..width.min(900)).step_by(12) {
                click(x as f32, y as f32);
                if app.global::<ProductionStudio>().get_current_phase() == 2 { break 'step; }
            }
        }
        assert_eq!(app.global::<ProductionStudio>().get_current_phase(), 2, "sidebar must open production and its second step must receive the click");
        for phase in 1..=7 {
            app.global::<ProductionStudio>().set_current_phase(phase);
            app.window().request_redraw();
            window.draw_if_needed(|renderer| { renderer.render(&mut buffer,width as usize); });
        }
        app.global::<ProductionStudio>().set_current_phase(1);
        println!("LAUNCHER_PRODUCTION=PASS actual sidebar click -> production -> step 2 click -> all seven native layouts; no providers");
    }
    if args.get(5).map(String::as_str)==Some("shorts-input") {
        if args[2] != "shorts-short" || width != 1080 { return Err("shorts input requires shorts-short 1080".into()); }
        use std::cell::Cell;
        use slint::{LogicalPosition, platform::{WindowEvent, PointerEventButton}};
        let calls=Rc::new(Cell::new(0));
        let counter=calls.clone();
        app.global::<AutoShorts>().on_analyze(move || counter.set(counter.get()+1));
        let click=|x,y| {
            let position=LogicalPosition::new(x,y);
            app.window().dispatch_event(WindowEvent::PointerPressed {position,button:PointerEventButton::Left});
            app.window().dispatch_event(WindowEvent::PointerReleased {position,button:PointerEventButton::Left});
        };
        click(90.,467.);
        assert_eq!(calls.get(),0,"5s source / 15s minimum must not submit analysis");
        assert_eq!(app.global::<AutoShorts>().get_min_duration(),15,"must not silently rewrite chosen limits");
        app.global::<AutoShorts>().set_min_duration(4);
        app.window().request_redraw();
        window.draw_if_needed(|renderer| { renderer.render(&mut buffer,width as usize); });
        click(90.,434.);
        assert_eq!(calls.get(),1,"explicitly lowering the minimum enables analysis");
        app.global::<AutoShorts>().set_busy(true);
        click(90.,434.);
        assert_eq!(calls.get(),1,"busy action must not submit twice");
        app.global::<AutoShorts>().set_busy(false);
        click(300.,33.);
        assert!(app.global::<AutoShorts>().get_advanced(),"optional configuration remains reachable");
        assert_eq!(app.global::<AutoShorts>().get_max_duration(),60);
        println!("SHORTS_INPUT=PASS too_short_blocked=1 explicit_adjustment=1 busy_blocked=1 advanced=reachable (no media/AI request)");
    }
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
        println!("INPUT_SMOKE=PASS (real native pointer/key events, API prompt state; no website/provider)");
    }
    if args.get(5).map(String::as_str)==Some("clipboard-smoke") {
        if args[2] != "ai-edit-manual" || width != 1080 { return Err("clipboard fixture requires ai-edit-manual 1080".into()); }
        let weak=app.as_weak();
        app.global::<AiEditing>().on_prepare_web_context(move || weak.unwrap().global::<AiEditing>().set_web_context("fixture-context-not-a-provider-call".into()));
        use slint::{LogicalPosition, platform::{WindowEvent, PointerEventButton}};
        let position=LogicalPosition::new(500.,408.);
        app.window().dispatch_event(WindowEvent::PointerPressed {position,button:PointerEventButton::Left});
        app.window().dispatch_event(WindowEvent::PointerReleased {position,button:PointerEventButton::Left});
        assert_eq!(&*clipboard.borrow(),"fixture-context-not-a-provider-call","manual copy must prepare fresh context before copying");
        println!("CLIPBOARD_SMOKE=PASS (native click; fresh context; isolated clipboard)");
    }
    if args.get(5).map(String::as_str)==Some("live-input") {
        if args[2] != "live" || width != 1600 { return Err("live input fixture requires live 1600".into()); }
        use std::cell::Cell;
        use slint::{LogicalPosition, platform::{WindowEvent, PointerEventButton}};
        let starts=Rc::new(Cell::new(0));
        let calls=starts.clone();
        app.global::<StudioLive>().on_connect(move |name| { assert_eq!(name.as_str(),"@layout_fixture"); calls.set(calls.get()+1); });
        let click=|x,y| {
            let position=LogicalPosition::new(x,y);
            app.window().dispatch_event(WindowEvent::PointerPressed {position,button:PointerEventButton::Left});
            app.window().dispatch_event(WindowEvent::PointerReleased {position,button:PointerEventButton::Left});
        };
        for y in (210..300).step_by(4) { click(100.,y as f32); if starts.get()>0 { break; } }
        assert_eq!(starts.get(),1,"connect needs an explicit native click");
        for text in ["\n", " "] {
            app.window().dispatch_event(WindowEvent::KeyPressed {text:text.into()});
            app.window().dispatch_event(WindowEvent::KeyReleased {text:text.into()});
        }
        assert_eq!(starts.get(),3,"focused connection action supports Enter and Space");
        app.global::<StudioLive>().set_busy(true);
        app.window().dispatch_event(WindowEvent::KeyPressed {text:" ".into()});
        assert_eq!(starts.get(),3,"busy action cannot be repeated from keyboard");
        app.global::<StudioLive>().set_busy(false);
        assert!(!app.global::<StudioLive>().get_auto_reply(),"opening the page/settings must not enable automatic AI");
        println!("LIVE_INPUT=PASS explicit_connect=1 keyboard=2 busy_blocked=1 auto_reply=off (setup is always visible; no network)");
    }
    if args.get(2).map(String::as_str)==Some("avatar-picker-input") {
        use std::cell::Cell;
        use slint::{LogicalPosition, platform::{WindowEvent, PointerEventButton}};
        let chosen=Rc::new(Cell::new(0));
        let refreshed=Rc::new(Cell::new(0));
        let imported=Rc::new(Cell::new(0));
        let chosen_count=chosen.clone();
        let weak=app.as_weak();
        app.global::<StudioPromaxAvatars>().on_choose_video(move || {
            chosen_count.set(chosen_count.get()+1);
            let app=weak.unwrap();
            app.global::<StudioPromaxAvatars>().set_file_label("explicit-layout-fixture.mp4".into());
            app.global::<StudioPromaxAvatars>().set_name("Fixture".into());
        });
        let refreshed_count=refreshed.clone();
        app.global::<StudioPromaxAvatars>().on_refresh(move || refreshed_count.set(refreshed_count.get()+1));
        let imported_count=imported.clone();
        app.global::<StudioPromaxAvatars>().on_import_video(move || imported_count.set(imported_count.get()+1));
        let click=|x,y| {
            let position=LogicalPosition::new(x,y);
            app.window().dispatch_event(WindowEvent::PointerPressed {position,button:PointerEventButton::Left});
            app.window().dispatch_event(WindowEvent::PointerReleased {position,button:PointerEventButton::Left});
        };
        for y in (24..450).step_by(4) {
            click(width as f32/2.,y as f32);
            if chosen.get()>0 || refreshed.get()>0 { break; }
        }
        assert_eq!(chosen.get(),1,"empty ready library must open video selection, not silently refresh an empty list");
        assert_eq!(refreshed.get(),0);
        assert_eq!(imported.get(),0,"selecting a file must not automatically submit it");
        assert_eq!(app.global::<StudioPromaxAvatars>().get_file_label().as_str(),"explicit-layout-fixture.mp4");
        println!("AVATAR_PICKER_INPUT=PASS choose=1 automatic_upload=0 (picker only selects; scanning requires the separate creation form)");
    }
    if args[2] == "hotclip-transcript" {
        use slint::{LogicalPosition, platform::{WindowEvent, PointerEventButton}};
        let position = LogicalPosition::new(width as f32 * 0.375, 56.);
        app.window().dispatch_event(WindowEvent::PointerPressed {position, button:PointerEventButton::Left});
        app.window().dispatch_event(WindowEvent::PointerReleased {position, button:PointerEventButton::Left});
        app.window().request_redraw();
        window.draw_if_needed(|renderer| { renderer.render(&mut buffer,width as usize); });
    }
    if args.get(5).map(String::as_str)==Some("scroll-bottom") {
        let before=buffer.clone();
        app.window().dispatch_event(slint::platform::WindowEvent::PointerScrolled {
            position:slint::LogicalPosition::new(3.,height as f32/2.), delta_x:0., delta_y:-10000.
        });
        app.window().request_redraw();
        window.draw_if_needed(|renderer| { renderer.render(&mut buffer,width as usize); });
        assert_ne!(before,buffer,"overflow page must actually scroll to lower controls");
        println!("SCROLL_INPUT=PASS (native wheel moves the constrained page)");
    }
    app.window().request_redraw();
    window.draw_if_needed(|renderer| { renderer.render(&mut buffer,width as usize); });
    let bytes: Vec<u8> = buffer.iter().flat_map(|p|[p.r,p.g,p.b]).collect();
    let file = std::fs::File::create(output)?;
    let mut encoder = png::Encoder::new(file,width,height);
    encoder.set_color(png::ColorType::Rgb); encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&bytes)?;
    println!("Rendered {output} ({width}x{height}); disconnected state, no backend calls.");
    Ok(())
}

