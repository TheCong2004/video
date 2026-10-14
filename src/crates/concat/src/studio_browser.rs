// SPDX-License-Identifier: AGPL-3.0-or-later
//! Explicit lifecycle requests to Nexora's local manager. No process scanning.
use crate::{
    i18n::t,
    ui::{App, StudioBrowser},
};
use concat_host::studio_browser::{BrowserClient, BrowserError, BrowserProfile};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::sync::{Arc, Mutex};

fn message(error: BrowserError) -> String {
    match error {
        BrowserError::Configuration => t("studioBrowser.configuration"),
        BrowserError::Unavailable => t("studioBrowser.unavailable"),
        BrowserError::InvalidResponse => t("studioBrowser.invalidResponse"),
        BrowserError::Stale => t("studioBrowser.stale"),
        BrowserError::UnknownOutcome => t("studioBrowser.unknownOutcome"),
    }
}

fn execute(
    app: &App,
    selected: Option<(BrowserProfile, bool)>,
    rows: Arc<Mutex<Vec<BrowserProfile>>>,
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
                let client = BrowserClient::new(
                    &std::env::var("DONUT_BROWSER_API_URL")
                        .unwrap_or_else(|_| "http://127.0.0.1:10108".into()),
                )?;
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
                        state.set_status(t("studioBrowser.loaded").into());
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
                    Err(error) => state.set_status(message(error).into()),
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
            execute(&app, None, rows.clone());
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
                execute(&app, Some((selected, false)), rows.clone());
            }
        }
    });
    let weak = app.as_weak();
    let rows = profiles;
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
                execute(&app, Some((selected, true)), rows.clone());
            }
        }
    });
}
