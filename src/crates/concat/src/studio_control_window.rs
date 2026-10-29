//! A second view of the existing control owner, never a second project/session.
use crate::ui::{AiControl, AiEditing, App};
use concat_control_ui::{AiControl as PanelControl, ControlWindow};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{cell::RefCell, time::Duration};

thread_local! {
    static PANEL: RefCell<Option<ControlWindow>> = const { RefCell::new(None) };
    static TIMER: RefCell<Option<slint::Timer>> = const { RefCell::new(None) };
}

macro_rules! fields {
    ($from:expr, $to:expr; $( $get:ident => $set:ident ),* $(,)?) => {
        $( $to.$set($from.$get()); )*
    };
}

fn inputs(panel: &ControlWindow, app: &App) {
    let from = panel.global::<PanelControl>();
    let to = app.global::<AiControl>();
    fields!(from,to;
        get_command=>set_command,get_script=>set_script,get_model=>set_model,
        get_source=>set_source,get_web_provider=>set_web_provider,
        get_workflow_kind=>set_workflow_kind,get_target_project=>set_target_project,
        get_project_name=>set_project_name,get_mode=>set_mode,
        get_selected_step=>set_selected_step,get_step_input=>set_step_input,
        get_allow_network=>set_allow_network,get_allow_mcp=>set_allow_mcp,
        get_codex_path=>set_codex_path,get_show_details=>set_show_details,
        get_show_inputs=>set_show_inputs);
}

fn outputs(app: &App, panel: &ControlWindow, restore: bool) {
    panel.set_nexora_busy(app.global::<crate::ui::StudioBrowser>().get_busy() || app.global::<crate::ui::ProductionStudio>().get_busy());
    let browser_status=app.global::<crate::ui::StudioBrowser>().get_status();
    let page_status=app.global::<crate::ui::ProductionStudio>().get_page_summary();
    panel.set_nexora_status(if !browser_status.is_empty(){browser_status}else if !page_status.is_empty(){page_status}else{"Dùng kết nối Nexora của Xưởng phim AI".into()});
    panel.global::<concat_control_ui::Theme>().set_dark(app.global::<crate::ui::Theme>().get_dark());
    panel.global::<concat_control_ui::Theme>().set_reduced_motion(app.global::<crate::ui::Theme>().get_reduced_motion());
    panel.global::<concat_control_ui::Theme>().set_system_reduced_motion(app.global::<crate::ui::Theme>().get_system_reduced_motion());
    let from = app.global::<AiControl>();
    let to = panel.global::<PanelControl>();
    let changed = from.get_workflow_id() != to.get_workflow_id()
        || from.get_workflow_revision() != to.get_workflow_revision();
    if restore || from.get_workflow_id() != to.get_workflow_id() {
        fields!(from,to;
            get_command=>set_command,get_script=>set_script,get_model=>set_model,
            get_source=>set_source,get_web_provider=>set_web_provider,
            get_workflow_kind=>set_workflow_kind,get_target_project=>set_target_project,
            get_project_name=>set_project_name,get_mode=>set_mode,
            get_allow_network=>set_allow_network,get_allow_mcp=>set_allow_mcp,
            get_codex_path=>set_codex_path,get_selected_step=>set_selected_step,
            get_step_input=>set_step_input);
    }
    fields!(from,to;
        get_ready=>set_ready,get_busy=>set_busy,get_status=>set_status,
        get_web_status=>set_web_status,get_web_login_busy=>set_web_login_busy,get_result_path=>set_result_path,
        get_project_label=>set_project_label,get_attachments=>set_attachments,
        get_audio_path=>set_audio_path,get_history=>set_history,get_favorites=>set_favorites,
        get_workflow_id=>set_workflow_id,get_workflow_title=>set_workflow_title,
        get_workflow_revision=>set_workflow_revision,get_activity=>set_activity,
        get_completed=>set_completed,get_resumable=>set_resumable,
        get_codex_status=>set_codex_status,get_codex_busy=>set_codex_busy);
    if restore || changed || from.get_steps().row_count() != to.get_steps().row_count() {
        let rows = from.get_steps().iter().map(|r| concat_control_ui::ControlStepRow {
            id:r.id,tool:r.tool,state:r.state,title:r.title,detail:r.detail,
            state_label:r.state_label,dependencies:r.dependencies,error:r.error,
            input:r.input,output:r.output,approvable:r.approvable,retryable:r.retryable,
        }).collect::<Vec<_>>();
        to.set_steps(ModelRc::new(VecModel::from(rows)));
    }
}

pub(crate) fn reveal_main(app: &App) {
    app.global::<AiControl>().set_workspace_open(false);
    if app.global::<AiEditing>().get_project_open() { app.set_on_start(false); }
    app.window().set_minimized(false);
    let _ = app.show();
    #[cfg(not(any(target_os="android", target_os="ios")))]
    {
        use slint::winit_030::WinitWindowAccessor;
        app.window().with_winit_window(|window| window.focus_window());
    }
}

fn tile(app: &App, panel: &ControlWindow) {
    #[cfg(not(any(target_os="android", target_os="ios")))]
    {
        use slint::winit_030::WinitWindowAccessor;
        if let Some(Some(monitor)) = app.window().with_winit_window(|w| w.current_monitor()) {
            let scale = monitor.scale_factor();
            let size = monitor.size().to_logical::<f32>(scale);
            let origin = monitor.position();
            let (main_width, panel_width) = widths(size.width);
            let height = (size.height-88.).max(420.);
            app.window().set_maximized(false);
            panel.window().set_maximized(false);
            app.window().set_size(slint::LogicalSize::new(main_width,height));
            panel.window().set_size(slint::LogicalSize::new(panel_width,height));
            // Monitor origins are physical desktop coordinates, including negative
            // origins on secondary displays. Do not scale the origin a second time.
            app.window().set_position(slint::PhysicalPosition::new(origin.x+8,origin.y+40));
            panel.window().set_position(slint::PhysicalPosition::new(origin.x+((main_width+20.)*scale as f32) as i32,origin.y+40));
            log::info!("AI windows tiled: monitor={:?} origin={:?} scale={} main={} panel={}",monitor.size(),origin,scale,main_width,panel_width);
        }
    }
}

fn widths(available: f32) -> (f32,f32) {
    let panel = (available-940.).clamp(340.,520.);
    ((available-panel-40.).max(900.),panel)
}

fn bind_proxy_callbacks(app: &App, panel: &ControlWindow) {
    macro_rules! proxy {
        ($($on:ident => $invoke:ident),* $(,)?) => { $(
            let main = app.as_weak(); let secondary = panel.as_weak();
            panel.global::<PanelControl>().$on(move || {
                if let (Some(app),Some(panel)) = (main.upgrade(),secondary.upgrade()) {
                    if matches!(stringify!($on),"on_pause"|"on_cancel") {crate::studio_ui_actions::release();}
                    inputs(&panel,&app);
                    app.global::<AiControl>().$invoke();
                    outputs(&app,&panel,false);
                }
            });
        )* };
    }
    proxy!(on_web_login=>invoke_web_login,on_pick_assets=>invoke_pick_assets,
        on_pick_audio=>invoke_pick_audio,on_pause=>invoke_pause,on_resume=>invoke_resume,
        on_cancel=>invoke_cancel,on_retry=>invoke_retry,on_approve=>invoke_approve,
        on_refresh=>invoke_refresh,on_save_favorite=>invoke_save_favorite,
        on_mode_changed=>invoke_mode_changed,on_update_step=>invoke_update_step,
        on_probe_codex=>invoke_probe_codex,on_open_project=>invoke_open_project,
        on_preview_result=>invoke_preview_result,on_reveal_result=>invoke_reveal_result,
        on_takeover_project=>invoke_takeover_project);
}

// Exercise the real secondary-window bridge without opening a browser or using
// an account. Separate UI instances keep the running owner's callbacks intact.
#[cfg(debug_assertions)]
pub(crate) fn smoke_web_login_bridge() -> Result<(), slint::PlatformError> {
    let main=App::new()?;
    let panel=ControlWindow::new()?;
    let observed=std::rc::Rc::new(std::cell::Cell::new(None));
    let result=observed.clone();
    let weak=main.as_weak();
    main.global::<AiControl>().on_web_login(move || {
        if let Some(main)=weak.upgrade(){
            let ui=main.global::<AiControl>();
            result.set(Some(ui.get_web_provider()));
            ui.set_web_login_busy(true);
            ui.set_web_status("bridge dispatch acknowledged".into());
        }
    });
    bind_proxy_callbacks(&main,&panel);
    for provider in 0..3 {
        observed.set(None);
        let ui=panel.global::<PanelControl>();
        ui.set_web_provider(provider);
        ui.set_web_login_busy(false);
        ui.set_web_status("".into());
        ui.invoke_web_login();
        if observed.get()!=Some(provider) || !ui.get_web_login_busy() || ui.get_web_status()!="bridge dispatch acknowledged" {
            return Err(slint::PlatformError::Other(format!("AI web login callback did not reach the owner or mirror status for provider {provider}")));
        }
    }
    log::info!("smoke: AI web login bridge: 3 providers dispatched and status mirrored");
    Ok(())
}

fn create(app: &App) -> Result<ControlWindow, slint::PlatformError> {
    let panel = ControlWindow::new()?;
    outputs(app,&panel,true);
    crate::studio_campaign::bind(app,&panel);
    bind_proxy_callbacks(app,&panel);
    let main=app.as_weak();
    panel.on_open_nexora(move ||{if let Some(app)=main.upgrade(){if !app.global::<AiControl>().get_busy(){app.global::<crate::ui::StudioBridge>().invoke_start_nexora();}}});
    let main=app.as_weak();
    panel.on_refresh_nexora(move ||{if let Some(app)=main.upgrade(){if !app.global::<AiControl>().get_busy(){app.global::<crate::ui::StudioBrowser>().invoke_refresh();app.global::<crate::ui::ProductionStudio>().invoke_load_pages();}}});
    let main=app.as_weak(); let secondary=panel.as_weak();
    panel.global::<PanelControl>().on_submit(move || {
        if let (Some(app),Some(panel))=(main.upgrade(),secondary.upgrade()) {
            inputs(&panel,&app);
            panel.set_show_plan(true);
            app.global::<AiControl>().invoke_submit();
            outputs(&app,&panel,false);
        }
    });
    let main=app.as_weak(); let secondary=panel.as_weak();
    panel.global::<PanelControl>().on_load_workflow(move |index| {
        if let (Some(app),Some(panel))=(main.upgrade(),secondary.upgrade()) {
            inputs(&panel,&app);
            panel.set_show_plan(true);
            panel.global::<PanelControl>().set_allow_network(false);
            app.global::<AiControl>().invoke_load_workflow(index);
        }
    });
    let main=app.as_weak();
    panel.on_focus_main(move || {if let Some(app)=main.upgrade(){reveal_main(&app);}});
    let main=app.as_weak();
    panel.on_connections(move || {if let Some(app)=main.upgrade(){reveal_main(&app);app.invoke_open_settings();}});
    let main=app.as_weak(); let secondary=panel.as_weak();
    panel.on_tile_windows(move || {if let (Some(app),Some(panel))=(main.upgrade(),secondary.upgrade()){reveal_main(&app);tile(&app,&panel);}});
    let main=app.as_weak(); let secondary=panel.as_weak();
    panel.on_dismiss(move || {if let (Some(app),Some(panel))=(main.upgrade(),secondary.upgrade()){inputs(&panel,&app);let _=panel.hide();}});
    let main=app.as_weak(); let secondary=panel.as_weak();
    panel.window().on_close_requested(move || {
        if let (Some(app),Some(panel))=(main.upgrade(),secondary.upgrade()){inputs(&panel,&app);}
        slint::CloseRequestResponse::HideWindow
    });
    Ok(panel)
}

pub(crate) fn bind(app: &App) {
    if std::env::var("CONCAT_AI_CONTROL_EMBEDDED").as_deref()==Ok("1") {return;}
    let main=app.as_weak();
    let timer=slint::Timer::default();
    let mut initial=true;
    let mut visible_ticks=0;
    timer.start(slint::TimerMode::Repeated,Duration::from_millis(33),move || {
        let Some(app)=main.upgrade() else {return;};
        PANEL.with(|slot| {
            let mut slot=slot.borrow_mut();
            if !app.window().is_visible() {
                crate::studio_ui_actions::release();
                if let Some(panel)=slot.as_ref(){let _=panel.hide();}
                return;
            }
            let requested=app.global::<AiControl>().get_workspace_open();
            app.global::<AiControl>().set_workspace_open(false);
            visible_ticks+=1;
            if (initial && visible_ticks>=18) || requested {
                initial=false;
                if slot.is_none() {
                    match create(&app) {Ok(panel)=>*slot=Some(panel),Err(error)=>{log::error!("AI window: {error}");return;}}
                }
                if let Some(panel)=slot.as_ref() {
                    outputs(&app,panel,true);
                    reveal_main(&app);
                    let _=panel.show();
                    tile(&app,panel);
                }
            }
            if let Some(panel)=slot.as_ref(){outputs(&app,panel,false);}
            crate::studio_ui_actions::sync(&app);
        });
    });
    TIMER.with(|slot| *slot.borrow_mut()=Some(timer));
}

#[cfg(test)]
mod tests {
    #[test]
    fn desktop_columns_preserve_editor_and_fit_standard_monitors() {
        for available in [1280.,1366.,1920.,2560.] {
            let (main,panel)=super::widths(available);
            assert!(main>=900. && panel>=340.);
            assert!(main+panel+40.<=available);
        }
    }
}
