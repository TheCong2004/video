// SPDX-License-Identifier: AGPL-3.0-or-later
//! Explicit lifecycle requests to Nexora's local manager. No process scanning.
use crate::{
    i18n::t,
    ui::{App, StudioBrowser},
};
use concat_host::studio_browser::{BrowserClient, BrowserError, BrowserProfile};
use concat_studio::nexora::NexoraSettings;
use slint::{ComponentHandle, ModelRc, VecModel};
use std::sync::{Arc, Mutex};

fn settings() -> Result<NexoraSettings, BrowserError> {
    let dirs = concat_host::AppDirs::locate().map_err(|_| BrowserError::Configuration)?;
    NexoraSettings::load(&dirs.config)
}
/// One configured local boundary shared with OAuth and service diagnostics.
pub(crate) fn client() -> Result<BrowserClient, BrowserError> {
    settings()?.client()
}

fn message(error: BrowserError) -> String {
    match error {
        BrowserError::Configuration => t("studioBrowser.configuration"),
        BrowserError::Unavailable => t("studioBrowser.unavailable"),
        BrowserError::InvalidResponse => t("studioBrowser.invalidResponse"),
        BrowserError::Stale => t("studioBrowser.stale"),
        BrowserError::UnknownOutcome => t("studioBrowser.unknownOutcome"),
        BrowserError::Authentication => "Nexora yêu cầu Local API token. Sao chép token từ Cài đặt/Tích hợp của Nexora vào ô bên dưới; không dùng token Promax và không gửi token vào chat.".into(),
        BrowserError::UnsupportedApi => "Dịch vụ không hỗ trợ Local Browser API của Nexora. Kiểm tra địa chỉ/cổng hoặc cập nhật Nexora.".into(),
    }
}

fn execute(
    app: &App,
    selected: Option<(BrowserProfile, bool)>,
    rows: Arc<Mutex<Vec<BrowserProfile>>>,
    start: bool,
) {
    if crate::studio_session::busy(app) {
        return;
    }
    let state = app.global::<StudioBrowser>();
    if state.get_busy() {
        return;
    }
    state.set_busy(true);
    state.set_status("".into());
    state.set_confirm_open(false);
    state.set_confirm_stop(false);
    let reply = app.as_weak();
    let worker = std::thread::Builder::new()
        .name("studio-browser".into())
        .spawn(move || {
            let result = (|| {
                if start {
                    return settings()?.show().map(Some);
                }
                let client = client()?;
                client.health()?;
                if let Some((selected, stop)) = selected {
                    if stop {
                        client.stop(&selected)?;
                    } else {
                        client.open_grok(&selected)?;
                    }
                    // No read-after-write network failure can undo a confirmed action.
                    Ok(None)
                } else {
                    client.profiles().map(Some)
                }
            })();
            let _ = reply.upgrade_in_event_loop(move |app| {
                let state = app.global::<StudioBrowser>();
                state.set_busy(false);
                match result {
                    Ok(Some(profiles)) => {
                        state.set_ids(ModelRc::new(VecModel::from(
                            profiles
                                .iter()
                                .map(|p| p.id.clone().into())
                                .collect::<Vec<slint::SharedString>>(),
                        )));
                        state.set_names(ModelRc::new(VecModel::from(
                            profiles
                                .iter()
                                .map(|p| {
                                    format!(
                                        "{} · {}",
                                        p.name,
                                        if p.running {
                                            t("studioBrowser.running")
                                        } else {
                                            t("studioBrowser.offline")
                                        }
                                    )
                                    .into()
                                })
                                .collect::<Vec<slint::SharedString>>(),
                        )));
                        if let Ok(mut rows) = rows.lock() {
                            *rows = profiles;
                        }
                        state.set_index(0);
                        state.set_loaded(true);
                        state.set_status(if start {
                            "Đã yêu cầu Nexora hiện cửa sổ đang chạy.".into()
                        } else {
                            t("studioBrowser.loaded").into()
                        });
                    }
                    Ok(None) => {
                        if let Ok(mut rows) = rows.lock() {
                            rows.clear();
                        }
                        state.set_ids(ModelRc::default());
                        state.set_names(ModelRc::default());
                        state.set_loaded(false);
                        state.set_status(t("studioBrowser.changed").into());
                    }
                    Err(error) => {
                        state.set_loaded(false);
                        state.set_ids(ModelRc::default());
                        state.set_names(ModelRc::default());
                        if let Ok(mut rows) = rows.lock() {
                            rows.clear();
                        }
                        state.set_status(message(error).into());
                    }
                }
            });
        });
    if worker.is_err() {
        state.set_busy(false);
        state.set_status(t("studioBrowser.unavailable").into());
    }
}

pub(crate) fn bind(app: &App) {
    let profiles = Arc::new(Mutex::new(Vec::<BrowserProfile>::new()));
    let weak = app.as_weak();
    let rows = profiles.clone();
    app.global::<StudioBrowser>().on_refresh(move || {
        if let Some(app) = weak.upgrade() {
            execute(&app, None, rows.clone(), false);
        }
    });
    let weak = app.as_weak();
    let rows = profiles.clone();
    app.global::<StudioBrowser>().on_open_grok(move || {
        if let Some(app) = weak.upgrade() {
            let state = app.global::<StudioBrowser>();
            if state.get_busy() || !state.get_confirm_open() {
                return;
            }
            let selected = usize::try_from(state.get_index())
                .ok()
                .and_then(|index| rows.lock().ok()?.get(index).cloned());
            if let Some(selected) = selected {
                execute(&app, Some((selected, false)), rows.clone(), false);
            }
        }
    });
    let weak = app.as_weak();
    let rows = profiles.clone();
    app.global::<StudioBrowser>().on_stop_profile(move || {
        if let Some(app) = weak.upgrade() {
            let state = app.global::<StudioBrowser>();
            if state.get_busy() || !state.get_confirm_stop() {
                return;
            }
            let selected = usize::try_from(state.get_index())
                .ok()
                .and_then(|index| rows.lock().ok()?.get(index).cloned());
            if let Some(selected) = selected {
                execute(&app, Some((selected, true)), rows.clone(), false);
            }
        }
    });
    let weak = app.as_weak();
    let rows = profiles;
    app.global::<StudioBrowser>().on_start(move || {
        if let Some(app) = weak.upgrade() {
            execute(&app, None, rows.clone(), true);
        }
    });
    let weak = app.as_weak();
    app.global::<StudioBrowser>().on_save_connection(move || {
        if let Some(app) = weak.upgrade() {
            configure(&app, false, false);
        }
    });
    let weak = app.as_weak();
    app.global::<StudioBrowser>().on_choose_executable(move || {
        if let Some(app) = weak.upgrade() {
            configure(&app, true, false);
        }
    });
    let weak = app.as_weak();
    app.global::<StudioBrowser>().on_forget_token(move || {
        if let Some(app) = weak.upgrade() {
            configure(&app, false, true);
        }
    });
    let weak = app.as_weak();
    let _ = std::thread::Builder::new()
        .name("nexora-settings-load".into())
        .spawn(move || {
            let result = settings();
            let _ = weak.upgrade_in_event_loop(move |app| {
                if app.global::<StudioBrowser>().get_busy() {
                    return;
                }
                if let Ok(settings) = result {
                    publish_settings(&app, &settings);
                }
            });
        });
}

fn publish_settings(app: &App, settings: &NexoraSettings) {
    let state = app.global::<StudioBrowser>();
    state.set_api_origin(settings.origin.clone().into());
    state.set_executable(
        settings
            .executable
            .clone()
            .or_else(|| settings.discover_executable())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
            .into(),
    );
    state.set_token_present(settings.has_token());
}

fn configure(app: &App, choose: bool, forget: bool) {
    let state = app.global::<StudioBrowser>();
    if state.get_busy() || crate::studio_session::busy(app) {
        return;
    }
    let origin = state.get_api_origin().to_string();
    let executable = state.get_executable().to_string();
    let token = state.get_new_token().to_string();
    state.set_new_token("".into());
    state.set_busy(true);
    let weak = app.as_weak();
    let worker=std::thread::Builder::new().name("nexora-settings-save".into()).spawn(move||{
        let result=(||{
            let mut settings=settings()?;
            if choose {
                if let Some(path)=rfd::FileDialog::new().set_title("Chọn nexorabrowser.exe đã cài đặt").add_filter("Nexora executable",&["exe"]).pick_file(){ settings.executable=Some(path); }
            } else if forget {settings.set_token("")?;}
            else {
                settings.origin=origin.trim().to_string();
                settings.executable=if executable.trim().is_empty(){None}else{Some(executable.trim().into())};
                if !token.is_empty(){settings.set_token(&token)?;}
            }
            let dirs=concat_host::AppDirs::locate().map_err(|_|BrowserError::Configuration)?;
            settings.save(&dirs.config)?;
            Ok::<_,BrowserError>(settings)
        })();
        let _=weak.upgrade_in_event_loop(move|app|{
            let state=app.global::<StudioBrowser>();state.set_busy(false);
            state.set_loaded(false);state.set_ids(ModelRc::default());state.set_names(ModelRc::default());
            match result {
                Ok(settings)=>{
                    crate::studio_session::connection_changed();
                    crate::studio_production_pages::disconnect(&app);
                    publish_settings(&app,&settings);
                    state.set_status("Đã lưu cấu hình Nexora trên máy. Tải lại Page/hồ sơ để dùng kết nối mới; token được lưu mã hóa.".into());
                }
                Err(error)=>state.set_status(message(error).into()),
            }
        });
    });
    if worker.is_err() {
        state.set_busy(false);
        state.set_status(t("studioBrowser.unavailable").into());
    }
}
