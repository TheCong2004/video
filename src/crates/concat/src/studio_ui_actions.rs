//! Visible, bounded input on our own Slint window. No desktop-wide input hooks.
use crate::ui::{AiControl, App};
use concat_control_ui::AutomationOverlay;
use slint::{ComponentHandle, platform::{WindowEvent, PointerEventButton}};
use std::{cell::{Cell, RefCell},sync::atomic::{AtomicBool,Ordering},time::Duration};
use slint::private_unstable_api::re_exports::{ItemRc,WindowInner,LogicalPoint,AccessibleStringProperty as P,AccessibilityAction,SupportedAccessibilityAction,AccessibleRole};
use serde_json::{Value,json};

thread_local! {
    static OVERLAY: RefCell<Option<AutomationOverlay>> = const { RefCell::new(None) };
    static MANUAL: Cell<bool> = const {Cell::new(false)};
    static TARGET: RefCell<Option<(String,[f32;4])>> = const {RefCell::new(None)};
    #[cfg(not(target_os="android"))]
    static LOCKED: Cell<Option<slint::winit_030::winit::window::WindowId>> = const {Cell::new(None)};
}

#[cfg(not(target_os="android"))]
pub(crate) fn blocks_input(id: slint::winit_030::winit::window::WindowId)->bool {LOCKED.with(|v|v.get()==Some(id))}

pub(crate) fn release() {
    MANUAL.with(|v|v.set(true));
    #[cfg(not(target_os="android"))]
    LOCKED.with(|v|v.set(None));
    TARGET.with(|v|*v.borrow_mut()=None);
    OVERLAY.with(|v|{if let Some(w)=v.borrow().as_ref(){let _=w.hide();}});
}

pub(crate) fn guard_visible()->bool {OVERLAY.with(|v|v.borrow().as_ref().is_some_and(|w|w.window().is_visible()))}

pub(crate) fn sync(app:&App) {
    // Lock only while actually operating main-app controls. Planning/login is
    // in the browser; its window must remain usable by the operator.
    let busy=app.global::<AiControl>().get_busy() && app.window().is_visible() && TARGET.with(|v|v.borrow().is_some());
    if !busy {release();MANUAL.with(|v|v.set(false));return;}
    if MANUAL.with(Cell::get){return;}
    OVERLAY.with(|slot| {
        let mut slot=slot.borrow_mut();
        if slot.is_none() {
            let Ok(window)=AutomationOverlay::new() else {return;};
            let weak=app.as_weak();
            window.on_takeover(move || {release();if let Some(app)=weak.upgrade(){app.global::<AiControl>().invoke_pause();}});
            let weak=app.as_weak();
            window.window().on_close_requested(move || {release();if let Some(app)=weak.upgrade(){app.global::<AiControl>().invoke_pause();}slint::CloseRequestResponse::HideWindow});
            *slot=Some(window);
        }
        let overlay=slot.as_ref().unwrap();
        let target=TARGET.with(|v|v.borrow().clone());
        let status=app.global::<AiControl>().get_web_status();
        overlay.set_action_text(target.as_ref().map(|v|v.0.clone()).unwrap_or_else(||
            if status.is_empty(){"AI đang xử lý. App chính tạm khóa nhập; bạn có thể dừng và tiếp quản.".into()}else{status.to_string()}).into());
        overlay.set_target_visible(target.is_some());
        if let Some((_,[x,y,w,h]))=target {overlay.set_target_x(x);overlay.set_target_y(y);overlay.set_target_width(w);overlay.set_target_height(h);}
        let size=app.window().size();
        if overlay.window().size()!=size {overlay.window().set_size(size);}
        overlay.window().set_position(app.window().position());
        #[cfg(not(target_os="android"))]
        {
            use slint::winit_030::WinitWindowAccessor;
            app.window().with_winit_window(|w|LOCKED.with(|v|v.set(Some(w.id()))));
        }
        if !overlay.window().is_visible(){let _=overlay.show();}
    });
}

fn locate(app:&App,label:&str,editable:bool)->Result<(ItemRc,[f32;4]),String> {
    let mut stack=vec![ItemRc::new_root(WindowInner::from_pub(app.window()).component())];
    let mut found=vec![];
    let mut visited=0;
    while let Some(item)=stack.pop() {
        visited+=1;if visited>20000{return Err("Cây giao diện quá lớn".into());}
        if !item.is_visible(){continue;}
        if let Some(mut child)=item.first_child() {loop {stack.push(child.clone());match child.next_sibling(){Some(next)=>child=next,None=>break}}}
        if !item.is_accessible(){continue;}
        let name=item.accessible_string_property(P::Label).unwrap_or_default();
        if name.as_str()!=label {continue;}
        if item.accessible_string_property(P::Enabled).as_deref()==Some("false"){continue;}
        let actions=item.supported_accessibility_actions();
        if editable && !actions.contains(SupportedAccessibilityAction::SetValue){continue;}
        if !editable && item.accessible_role()!=AccessibleRole::Button {continue;}
        let p=item.map_to_window(LogicalPoint::default());let size=item.geometry().size;
        if size.width>1. && size.height>1. {found.push((item,[p.x,p.y,size.width,size.height]));}
    }
    if found.len()!=1{return Err(format!("Cần đúng một điều khiển khả dụng: {label} (tìm thấy {})",found.len()));}
    Ok(found.remove(0))
}

fn label(action:&str)->Option<(&'static str,bool)> {
    Some(match action {
        "open_film"=>("Làm phim AI",false),
        "open_factory"=>("Kedi Factory",false),
        "film_idea"=>("Ý tưởng phim (nhân vật, bối cảnh, mâu thuẫn, cú twist, cái kết)",true),
        _=>return None,
    })
}

/// Re-query the actual tree before every scroll, highlight and dispatched click.
pub(crate) fn perform(app:&App,action:&str,value:&str,commit:bool)->Result<Value,String> {
    if commit && app.global::<AiControl>().get_busy() && !guard_visible() {
        return Err("Lớp bảo vệ chưa sẵn sàng; chưa bấm hoặc nhập nội dung.".into());
    }
    let (name,editable)=label(action).ok_or("Thao tác UI chưa được hỗ trợ")?;
    let (item,[x,y,w,h])=locate(app,name,editable)?;
    let scale=app.window().scale_factor();let size=app.window().size();
    let width=size.width as f32/scale;let height=size.height as f32/scale;
    let position=slint::LogicalPosition::new(x+w/2.,y+h/2.);
    if position.x<0. || position.x>width {return Err("Điều khiển nằm ngoài chiều ngang cửa sổ".into());}
    if position.y<130. || position.y>height-15. {
        app.window().dispatch_event(WindowEvent::PointerScrolled {position:slint::LogicalPosition::new((width-50.).max(50.),height-80.),delta_x:0.,delta_y:if position.y<130.{380.}else{-380.}});
        TARGET.with(|v|*v.borrow_mut()=Some((format!("Cuộn đến: {name}"),[width-70.,height-110.,40.,40.])));
        return Ok(json!({"visible":false}));
    }
    TARGET.with(|v|*v.borrow_mut()=Some((format!("{}: {name}",if editable {"Nhập nội dung"}else{"Bấm"}),[x,y,w,h])));
    if commit {
        app.window().dispatch_event(WindowEvent::PointerMoved {position});
        app.window().dispatch_event(WindowEvent::PointerPressed {position,button:PointerEventButton::Left});
        app.window().dispatch_event(WindowEvent::PointerReleased {position,button:PointerEventButton::Left});
        if editable {
            item.accessible_action(&AccessibilityAction::SetValue(value.into()));
            if item.accessible_string_property(P::Value).as_deref()!=Some(value){return Err("Ô nhập chưa xác nhận nội dung vừa điền".into());}
        }
    }
    Ok(json!({"visible":true,"applied":commit,"action":action}))
}

pub(crate) fn execute(weak:&slint::Weak<App>,input:&Value,cancel:&AtomicBool)->Result<Value,concat_studio::ai_control::Failure> {
    let action=input["action"].as_str().unwrap_or("").to_owned();let value=input["value"].as_str().unwrap_or("").to_owned();
    let fail=|message:&str|concat_studio::ai_control::Failure {message:message.into(),unknown:false,waiting_input:true};
    if label(&action).is_none() || value.len()>16000 || (action=="film_idea" && value.trim().is_empty()){return Err(fail("Thao tác UI không hợp lệ hoặc thiếu nội dung cần nhập"));}
    let mut visible=false;
    for _ in 0..16 {
        if cancel.load(Ordering::Acquire){return Err(fail("Đã dừng trước khi thao tác"));}
        let a=action.clone();let v=value.clone();
        let state=crate::studio_ai_control::on_ui(weak,move|app|perform(app,&a,&v,false))?;
        if state["visible"]==true {visible=true;break;}
        std::thread::sleep(Duration::from_millis(180));
    }
    if !visible{return Err(fail("Không cuộn đến được điều khiển. Hãy dừng và tiếp quản."));}
    std::thread::sleep(Duration::from_millis(650));
    if cancel.load(Ordering::Acquire){return Err(fail("Đã dừng trước khi bấm"));}
    let result=crate::studio_ai_control::on_ui(weak,move|app|perform(app,&action,&value,true))
        .map_err(|mut error|{error.unknown=true;error.waiting_input=false;error})?;
    if result["applied"]!=true{return Err(fail("Vị trí điều khiển đã thay đổi; chưa bấm. Hãy tiếp quản để kiểm tra."));}
    std::thread::sleep(Duration::from_millis(350));
    crate::studio_ai_control::on_ui(weak,move|_|{release();Ok(json!({"released":true}))})?;
    Ok(result)
}
