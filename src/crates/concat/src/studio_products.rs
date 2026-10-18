// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native product script editor; all persistent writes use the shared account.
use crate::{
    i18n::t,
    ui::{App, StudioLive, StudioOpenTalking, StudioProductRow, StudioProducts},
};
use concat_studio::{
    products::{LivePlaylist, LiveProduct, ProductsError},
    workflows::WorkflowError,
};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::sync::{Arc, Mutex};

type Cache = Arc<Mutex<Option<LivePlaylist>>>;
enum Action {
    Load,
    Save(Vec<LiveProduct>),
    Select(String),
    Next,
}

fn error_message(error: ProductsError) -> String {
    t(match error {
        ProductsError::Input => "studioProducts.invalid",
        ProductsError::UnknownOutcome => "studioProducts.uncertain",
        ProductsError::Remote(WorkflowError::Authentication) => "studioAi.authRequired",
        ProductsError::Remote(WorkflowError::Forbidden) => "productionStudio.accessDenied",
        ProductsError::Remote(WorkflowError::RateLimited) => "productionStudio.rateLimited",
        _ => "studioProducts.unavailable",
    })
}

fn publish(app: &App, cache: &Cache, playlist: LivePlaylist) {
    let state = app.global::<StudioProducts>();
    let active = playlist
        .products
        .iter()
        .find(|p| p.id == playlist.active_product_id && p.id.is_some());
    state.set_active_title(active.map(|p| p.title.clone()).unwrap_or_default().into());
    state.set_active_price(active.map(|p| p.price.clone()).unwrap_or_default().into());
    state.set_rows(ModelRc::new(VecModel::from(
        playlist
            .products
            .iter()
            .enumerate()
            .map(|(index, p)| StudioProductRow {
                index: index as i32,
                title: p.title.clone().into(),
                price: p.price.clone().into(),
                enabled: p.enabled,
                active: p.id.is_some() && p.id == playlist.active_product_id,
            })
            .collect::<Vec<_>>(),
    )));
    let live = app.global::<StudioLive>();
    live.set_pinned_title(active.map(|p| p.title.clone()).unwrap_or_default().into());
    live.set_pinned_price(active.map(|p| p.price.clone()).unwrap_or_default().into());
    live.set_pinned_script(
        active
            .map(|p| {
                if p.script.is_empty() {
                    p.title.clone()
                } else {
                    p.script.clone()
                }
            })
            .unwrap_or_default()
            .into(),
    );
    if let Ok(mut value) = cache.lock() {
        *value = Some(playlist);
    }
}

fn execute(app: &App, cache: Cache, action: Action) {
    let state = app.global::<StudioProducts>();
    if state.get_busy() || crate::studio_session::busy(app) {
        return;
    }
    let prepare_script = matches!(action, Action::Select(_) | Action::Next);
    let is_load = matches!(action, Action::Load);
    state.set_busy(true);
    let weak = app.as_weak();
    let worker = std::thread::Builder::new()
        .name("studio-live-products".into())
        .spawn(move || {
            let result = (|| {
                let client = crate::studio_session::client().map_err(ProductsError::Remote)?;
                match action {
                    Action::Load => client.live_products(),
                    Action::Save(products) => client.live_products_save(&products),
                    Action::Select(id) => client.live_product_select(&id),
                    Action::Next => client.live_product_next(),
                }
            })();
            let _ = weak.upgrade_in_event_loop(move |app| {
                let state = app.global::<StudioProducts>();
                state.set_busy(false);
                match result {
                    Ok(playlist) => {
                        if prepare_script {
                            if let Some(product) = playlist.products.iter().find(|p| {
                                p.id.is_some() && p.id == playlist.active_product_id && p.enabled
                            }) {
                                app.global::<StudioOpenTalking>().set_script(
                                    if product.script.is_empty() {
                                        product.title.clone()
                                    } else {
                                        product.script.clone()
                                    }
                                    .into(),
                                );
                            }
                        }
                        publish(&app, &cache, playlist);
                        state.set_loaded(true);
                        state.set_dirty(false);
                        state.set_uncertain(false);
                        state.set_editing(false);
                        state.set_delete_index(-1);
                        state.set_status(
                            t(if prepare_script {
                                "studioProducts.selected"
                            } else if is_load {
                                "studioProducts.loaded"
                            } else {
                                "studioProducts.saved"
                            })
                            .into(),
                        );
                    }
                    Err(error) => {
                        if error == ProductsError::UnknownOutcome {
                            state.set_uncertain(true);
                        }
                        state.set_status(error_message(error).into());
                    }
                }
            });
        });
    if worker.is_err() {
        state.set_busy(false);
        state.set_status(t("studioProducts.unavailable").into());
    }
}

fn editable(app: &App) -> bool {
    let state = app.global::<StudioProducts>();
    state.get_loaded()
        && !state.get_busy()
        && !state.get_uncertain()
        && !crate::studio_session::busy(app)
}

pub(crate) fn bind(app: &App) {
    let cache: Cache = Arc::new(Mutex::new(None));
    let weak = app.as_weak();
    let cached = Arc::clone(&cache);
    app.global::<StudioProducts>().on_load(move || {
        if let Some(app) = weak.upgrade() {
            let state = app.global::<StudioProducts>();
            if !state.get_dirty() && !state.get_editing() {
                execute(&app, Arc::clone(&cached), Action::Load);
            }
        }
    });
    let weak = app.as_weak();
    let cached = Arc::clone(&cache);
    app.global::<StudioProducts>().on_discard(move || {
        if let Some(app) = weak.upgrade() {
            execute(&app, Arc::clone(&cached), Action::Load);
        }
    });
    let weak = app.as_weak();
    app.global::<StudioProducts>().on_new_product(move || {
        let Some(app) = weak.upgrade() else { return };
        let state = app.global::<StudioProducts>();
        if !editable(&app) || state.get_editing() {
            return;
        }
        state.set_edit_index(-1);
        state.set_title("".into());
        state.set_price("".into());
        state.set_script("".into());
        state.set_duration("180".into());
        state.set_enabled(true);
        state.set_editing(true);
    });
    let weak = app.as_weak();
    let cached = Arc::clone(&cache);
    app.global::<StudioProducts>().on_edit(move |index| {
        let Some(app) = weak.upgrade() else { return };
        if !editable(&app) {
            return;
        }
        let product = cached.lock().ok().and_then(|p| {
            p.as_ref().and_then(|p| {
                usize::try_from(index)
                    .ok()
                    .and_then(|i| p.products.get(i))
                    .cloned()
            })
        });
        if let Some(product) = product {
            let state = app.global::<StudioProducts>();
            state.set_edit_index(index);
            state.set_title(product.title.into());
            state.set_price(product.price.into());
            state.set_script(product.script.into());
            state.set_duration(product.duration.to_string().into());
            state.set_enabled(product.enabled);
            state.set_editing(true);
        }
    });
    let weak = app.as_weak();
    let cached = Arc::clone(&cache);
    app.global::<StudioProducts>().on_apply(move || {
        let Some(app) = weak.upgrade() else { return };
        let state = app.global::<StudioProducts>();
        if !editable(&app) || !state.get_editing() {
            return;
        }
        let Ok(mut lock) = cached.lock() else { return };
        let Some(mut playlist) = lock.clone() else {
            return;
        };
        let index = usize::try_from(state.get_edit_index()).ok();
        let Some(duration) = state.get_duration().parse::<u32>().ok() else {
            state.set_status(t("studioProducts.invalid").into());
            return;
        };
        let draft = LiveProduct {
            id: index
                .and_then(|i| playlist.products.get(i))
                .and_then(|p| p.id.clone()),
            title: state.get_title().to_string(),
            price: state.get_price().to_string(),
            script: state.get_script().to_string(),
            duration,
            enabled: state.get_enabled(),
        };
        let Ok(draft) = draft.validated() else {
            state.set_status(t("studioProducts.invalid").into());
            return;
        };
        match index {
            Some(i) if i < playlist.products.len() => playlist.products[i] = draft,
            None if playlist.products.len() < 200 => playlist.products.push(draft),
            _ => return,
        }
        *lock = Some(playlist.clone());
        drop(lock);
        publish(&app, &cached, playlist);
        state.set_editing(false);
        state.set_dirty(true);
        state.set_status(t("studioProducts.draft").into());
    });
    let weak = app.as_weak();
    let cached = Arc::clone(&cache);
    app.global::<StudioProducts>().on_remove(move |index| {
        let Some(app) = weak.upgrade() else { return };
        let state = app.global::<StudioProducts>();
        if !editable(&app) || state.get_editing() || state.get_delete_index() != index {
            return;
        }
        let Ok(mut lock) = cached.lock() else { return };
        let Some(mut playlist) = lock.clone() else {
            return;
        };
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        if index >= playlist.products.len() {
            return;
        }
        playlist.products.remove(index);
        *lock = Some(playlist.clone());
        drop(lock);
        publish(&app, &cached, playlist);
        state.set_delete_index(-1);
        state.set_dirty(true);
        state.set_status(t("studioProducts.draft").into());
    });
    let weak = app.as_weak();
    let cached = Arc::clone(&cache);
    app.global::<StudioProducts>().on_save(move || {
        let Some(app) = weak.upgrade() else { return };
        let state = app.global::<StudioProducts>();
        if !editable(&app) || !state.get_dirty() || state.get_editing() {
            return;
        }
        let products = cached
            .lock()
            .ok()
            .and_then(|p| p.as_ref().map(|p| p.products.clone()));
        if let Some(products) = products {
            execute(&app, Arc::clone(&cached), Action::Save(products));
        }
    });
    let weak = app.as_weak();
    let cached = Arc::clone(&cache);
    app.global::<StudioProducts>()
        .on_select_product(move |index| {
            let Some(app) = weak.upgrade() else { return };
            let state = app.global::<StudioProducts>();
            if !editable(&app) || state.get_dirty() || state.get_editing() {
                return;
            }
            let id = cached.lock().ok().and_then(|p| {
                p.as_ref()
                    .and_then(|p| usize::try_from(index).ok().and_then(|i| p.products.get(i)))
                    .filter(|p| p.enabled)
                    .and_then(|p| p.id.clone())
            });
            if let Some(id) = id {
                execute(&app, Arc::clone(&cached), Action::Select(id));
            }
        });
    let weak = app.as_weak();
    app.global::<StudioProducts>().on_next_product(move || {
        let Some(app) = weak.upgrade() else { return };
        let state = app.global::<StudioProducts>();
        if editable(&app) && !state.get_dirty() && !state.get_editing() {
            execute(&app, Arc::clone(&cache), Action::Next);
        }
    });
}
