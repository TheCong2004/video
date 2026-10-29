//! In-memory component integration only: this platform creates no OS windows.
mod ui {pub use concat_ui::*;}
mod studio_ai_control {
    pub(crate) fn on_ui(weak:&slint::Weak<crate::ui::App>,call:impl FnOnce(&crate::ui::App)->Result<serde_json::Value,String>+Send+'static)->Result<serde_json::Value,concat_studio::ai_control::Failure> {
        let app=weak.upgrade().expect("same test thread");
        call(&app).map_err(|message|concat_studio::ai_control::Failure{message,unknown:false,waiting_input:false})
    }
}
#[path="../../src/crates/concat/src/studio_ui_actions.rs"] mod actions;
use slint::ComponentHandle;
use slint::platform::{Platform,WindowAdapter,software_renderer::{MinimalSoftwareWindow,RepaintBufferType}};
use std::{rc::Rc,cell::RefCell};
struct MemoryPlatform {windows:Rc<RefCell<Vec<Rc<MinimalSoftwareWindow>>>>}
impl Platform for MemoryPlatform {
    fn create_window_adapter(&self)->Result<Rc<dyn WindowAdapter>,slint::PlatformError>{
        let window=MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);self.windows.borrow_mut().push(window.clone());Ok(window)
    }
}
#[test]
fn real_component_pointer_navigation_paste_and_guard() {
    let windows=Rc::new(RefCell::new(vec![]));
    slint::platform::set_platform(Box::new(MemoryPlatform{windows:windows.clone()})).unwrap();
    let app=ui::App::new().unwrap();app.set_on_start(true);
    app.window().set_size(slint::LogicalSize::new(900.,712.));app.show().unwrap();
    let draw=|| {for window in windows.borrow().iter(){let size=window.size();if size.width>0 && size.height>0 {let mut pixels=slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(size.width,size.height);let stride=size.width as usize;window.draw_if_needed(|r|{r.render(pixels.make_mut_slice(),stride);});}}};
    draw();
    assert!(actions::perform(&app,"delete_everything","",true).is_err());
    app.global::<ui::AiControl>().set_busy(true);
    assert!(actions::perform(&app,"open_factory","",true).is_err(),"busy actor must refuse input without its guard");
    app.global::<ui::AiControl>().set_busy(false);
    for action in ["open_factory","open_film"] {
        assert_eq!(actions::perform(&app,action,"",false).unwrap()["visible"],true);
        app.global::<ui::AiControl>().set_busy(true);actions::sync(&app);assert!(actions::guard_visible());
        assert_eq!(actions::perform(&app,action,"",true).unwrap()["applied"],true);draw();
        actions::release();assert!(!actions::guard_visible());
        app.global::<ui::AiControl>().set_busy(false);actions::sync(&app);
    }
    let text="Visible input through the real Film control";
    let mut ready=false;
    for _ in 0..16 {let state=actions::perform(&app,"film_idea",text,false).unwrap();draw();if state["visible"]==true{ready=true;break;}}
    assert!(ready,"real Film field must be reachable");
    assert_eq!(actions::perform(&app,"film_idea",text,true).unwrap()["applied"],true);
    assert_eq!(app.global::<ui::FilmStudio>().get_idea().as_str(),text,"actual edited callback must update FilmStudio");
    actions::release();app.hide().unwrap();
}
