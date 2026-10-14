// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native content-page catalog callbacks. Persistence and filesystem access run off the UI thread.

use crate::i18n::t;
use crate::ui::{App, StudioChannels, StudioPageRow};
use concat_host::dirs::AppDirs;
use concat_host::studio_pages::{ContentPage, ContentPageDraft, ContentPageStore};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::sync::{Arc, Mutex};

enum Action {
    Load(bool),
    Save(ContentPageDraft),
    Archive(String, u64, bool),
}

enum Reply {
    Loaded(Vec<ContentPage>),
    Saved(ContentPage, Vec<ContentPage>),
    Archived(Vec<ContentPage>),
}

fn row(page: ContentPage) -> StudioPageRow {
    StudioPageRow {
        id: page.id.into(),
        name: page.name.into(),
        slug: page.slug.into(),
        output_root: page.output_root.into(),
        target_platform: page.target_platform.unwrap_or_default().into(),
        default_model_id: page.default_model_id.unwrap_or_default().into(),
        default_workflow_id: page.default_workflow_id.unwrap_or_default().into(),
        default_language: page.default_language.unwrap_or_default().into(),
        default_tone: page.default_tone.unwrap_or_default().into(),
        default_aspect_ratio: page.default_aspect_ratio.unwrap_or_default().into(),
        browser_profile_id: page.browser_profile_id.unwrap_or_default().into(),
        worker_pool_id: page.worker_pool_id.unwrap_or_default().into(),
        default_image_prompt: page.default_image_prompt.unwrap_or_default().into(),
        default_expand_prompt: page.default_expand_9_16_prompt.unwrap_or_default().into(),
        default_video_prompt: page.default_video_prompt.unwrap_or_default().into(),
        is_archived: page.is_archived,
        revision: i32::try_from(page.revision).unwrap_or(i32::MAX),
    }
}

fn optional(value: slint::SharedString) -> Option<String> {
    let value = value.trim().to_owned();
    (!value.is_empty()).then_some(value)
}

fn clear_editor(state: &StudioChannels) {
    state.set_id("".into());
    state.set_revision(0);
    state.set_name("".into());
    state.set_slug("".into());
    state.set_output_root("".into());
    state.set_target_platform("".into());
    state.set_default_model_id("".into());
    state.set_default_workflow_id("".into());
    state.set_default_language("".into());
    state.set_default_tone("".into());
    state.set_default_aspect_ratio("".into());
    state.set_browser_profile_id("".into());
    state.set_worker_pool_id("".into());
    state.set_default_image_prompt("".into());
    state.set_default_expand_prompt("".into());
    state.set_default_video_prompt("".into());
}

pub(crate) fn bind(app: &App) {
    let pages = Arc::new(Mutex::new(Vec::<ContentPage>::new()));

    let weak = app.as_weak();
    let cached = pages.clone();
    app.global::<StudioChannels>().on_load_pages(move || {
        if let Some(app) = weak.upgrade() {
            let include_archived = app.global::<StudioChannels>().get_include_archived();
            run(&app, Action::Load(include_archived), cached.clone());
        }
    });

    let weak = app.as_weak();
    app.global::<StudioChannels>().on_new_page(move || {
        if let Some(app) = weak.upgrade() {
            let state = app.global::<StudioChannels>();
            if state.get_busy() {
                return;
            }
            clear_editor(&state);
            state.set_editing(true);
            state.set_confirm_archive_id("".into());
            state.set_status(t("studioPages.newDraft").into());
        }
    });

    let weak = app.as_weak();
    let cached = pages.clone();
    app.global::<StudioChannels>().on_select_page(move |id| {
        let Some(app) = weak.upgrade() else {
            return;
        };
        let state = app.global::<StudioChannels>();
        if state.get_busy() {
            return;
        }
        let page = cached
            .lock()
            .ok()
            .and_then(|pages| pages.iter().find(|page| page.id == id.as_str()).cloned());
        if let Some(page) = page {
            state.set_id(page.id.into());
            state.set_revision(i32::try_from(page.revision).unwrap_or(i32::MAX));
            state.set_name(page.name.into());
            state.set_slug(page.slug.into());
            state.set_output_root(page.output_root.into());
            state.set_target_platform(page.target_platform.unwrap_or_default().into());
            state.set_default_model_id(page.default_model_id.unwrap_or_default().into());
            state.set_default_workflow_id(page.default_workflow_id.unwrap_or_default().into());
            state.set_default_language(page.default_language.unwrap_or_default().into());
            state.set_default_tone(page.default_tone.unwrap_or_default().into());
            state.set_default_aspect_ratio(page.default_aspect_ratio.unwrap_or_default().into());
            state.set_browser_profile_id(page.browser_profile_id.unwrap_or_default().into());
            state.set_worker_pool_id(page.worker_pool_id.unwrap_or_default().into());
            state.set_default_image_prompt(page.default_image_prompt.unwrap_or_default().into());
            state.set_default_expand_prompt(
                page.default_expand_9_16_prompt.unwrap_or_default().into(),
            );
            state.set_default_video_prompt(page.default_video_prompt.unwrap_or_default().into());
            state.set_editing(true);
            state.set_confirm_archive_id("".into());
            state.set_status(t("studioPages.editing").into());
        }
    });

    let weak = app.as_weak();
    let cached = pages.clone();
    app.global::<StudioChannels>().on_save_page(move || {
        let Some(app) = weak.upgrade() else {
            return;
        };
        let state = app.global::<StudioChannels>();
        if state.get_busy() || !state.get_editing() {
            return;
        }
        let revision = match u64::try_from(state.get_revision()) {
            Ok(revision) => revision,
            Err(_) => {
                state.set_status(t("studioPages.invalidRevision").into());
                return;
            }
        };
        let id = state.get_id().to_string();
        let draft = ContentPageDraft {
            id: (!id.is_empty()).then_some(id),
            revision,
            name: state.get_name().to_string(),
            slug: optional(state.get_slug()),
            output_root: state.get_output_root().to_string(),
            target_platform: optional(state.get_target_platform()),
            default_model_id: optional(state.get_default_model_id()),
            default_workflow_id: optional(state.get_default_workflow_id()),
            default_language: optional(state.get_default_language()),
            default_tone: optional(state.get_default_tone()),
            default_aspect_ratio: optional(state.get_default_aspect_ratio()),
            browser_profile_id: optional(state.get_browser_profile_id()),
            worker_pool_id: optional(state.get_worker_pool_id()),
            default_image_prompt: optional(state.get_default_image_prompt()),
            default_expand_9_16_prompt: optional(state.get_default_expand_prompt()),
            default_video_prompt: optional(state.get_default_video_prompt()),
        };
        run(&app, Action::Save(draft), cached.clone());
    });

    let weak = app.as_weak();
    let cached = pages;
    app.global::<StudioChannels>()
        .on_archive_page(move |id, revision, archived| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            let state = app.global::<StudioChannels>();
            if state.get_busy() || state.get_confirm_archive_id() != id {
                return;
            }
            let Ok(revision) = u64::try_from(revision) else {
                state.set_status(t("studioPages.invalidRevision").into());
                return;
            };
            run(
                &app,
                Action::Archive(id.to_string(), revision, archived),
                cached.clone(),
            );
        });
}

fn run(app: &App, action: Action, cached: Arc<Mutex<Vec<ContentPage>>>) {
    let state = app.global::<StudioChannels>();
    if state.get_busy() {
        return;
    }
    state.set_busy(true);
    state.set_status(t("studioPages.working").into());
    let include_archived = state.get_include_archived();
    let weak = app.as_weak();
    let worker = std::thread::Builder::new()
        .name("studio-pages".into())
        .spawn(move || {
            let result = (|| -> Result<Reply, String> {
                let dirs = AppDirs::locate()?;
                let store = ContentPageStore::new(&dirs.config);
                match action {
                    Action::Load(include_archived) => {
                        store.list(include_archived).map(Reply::Loaded)
                    }
                    Action::Save(draft) => {
                        let page = store.save(draft)?;
                        let rows = store.list(include_archived)?;
                        Ok(Reply::Saved(page, rows))
                    }
                    Action::Archive(id, revision, archived) => {
                        store.archive(&id, revision, archived)?;
                        store.list(include_archived).map(Reply::Archived)
                    }
                }
            })();
            let _ = weak.upgrade_in_event_loop(move |app| {
                let state = app.global::<StudioChannels>();
                state.set_busy(false);
                match result {
                    Ok(Reply::Loaded(rows)) => {
                        set_rows(&state, &cached, rows);
                        state.set_loaded(true);
                        state.set_status(t("studioPages.loaded").into());
                    }
                    Ok(Reply::Saved(page, rows)) => {
                        state.set_id(page.id.into());
                        state.set_revision(i32::try_from(page.revision).unwrap_or(i32::MAX));
                        state.set_editing(true);
                        set_rows(&state, &cached, rows);
                        state.set_loaded(true);
                        state.set_status(t("studioPages.saved").into());
                    }
                    Ok(Reply::Archived(rows)) => {
                        state.set_confirm_archive_id("".into());
                        state.set_editing(false);
                        set_rows(&state, &cached, rows);
                        state.set_loaded(true);
                        state.set_status(t("studioPages.archivedDone").into());
                    }
                    Err(error) => state.set_status(error.into()),
                }
            });
        });
    if worker.is_err() {
        state.set_busy(false);
        state.set_status(t("studioPages.workerError").into());
    }
}

fn set_rows(state: &StudioChannels, cached: &Mutex<Vec<ContentPage>>, pages: Vec<ContentPage>) {
    if let Ok(mut rows) = cached.lock() {
        *rows = pages.clone();
    }
    state.set_rows(ModelRc::new(VecModel::from(
        pages.into_iter().map(row).collect::<Vec<_>>(),
    )));
}
