// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native prompt-library callbacks. File access runs off the UI thread.

use crate::i18n::t;
use crate::ui::{App, ScriptLibrary, StudioPromptRow};
use concat_host::dirs::AppDirs;
use concat_host::studio_scripts::{PromptDraft, PromptTemplate, ScriptStore};
use slint::{ComponentHandle, ModelRc, VecModel};

enum Action {
    List,
    Save(PromptDraft),
    Delete(String, u64),
}

fn row(item: PromptTemplate) -> StudioPromptRow {
    StudioPromptRow {
        id: item.id.into(),
        name: item.name.into(),
        image_prompt: item.image_prompt.into(),
        expand_prompt: item.expand_prompt.unwrap_or_default().into(),
        video_prompt: item.video_prompt.into(),
        revision: item.revision.to_string().into(),
    }
}

pub(crate) fn bind(app: &App) {
    let weak = app.as_weak();
    app.global::<ScriptLibrary>().on_refresh(move || {
        if let Some(app) = weak.upgrade() {
            run(&app, Action::List);
        }
    });
    let weak = app.as_weak();
    app.global::<ScriptLibrary>().on_save_script(move |item| {
        if let Some(app) = weak.upgrade() {
            let revision = if item.id.is_empty() {
                0
            } else {
                match item.revision.parse::<u64>() {
                    Ok(revision) => revision,
                    Err(_) => {
                        app.global::<ScriptLibrary>()
                            .set_status(t("studioScripts.revisionError").into());
                        return;
                    }
                }
            };
            run(
                &app,
                Action::Save(PromptDraft {
                    id: (!item.id.is_empty()).then(|| item.id.to_string()),
                    revision,
                    name: item.name.to_string(),
                    image_prompt: item.image_prompt.to_string(),
                    expand_prompt: Some(item.expand_prompt.to_string()),
                    video_prompt: item.video_prompt.to_string(),
                }),
            );
        }
    });
    let weak = app.as_weak();
    app.global::<ScriptLibrary>().on_delete_script(move |item| {
        if let Some(app) = weak.upgrade() {
            if !app.global::<ScriptLibrary>().get_confirming_delete() {
                return;
            }
            if let Ok(revision) = item.revision.parse::<u64>() {
                run(&app, Action::Delete(item.id.to_string(), revision));
            }
        }
    });
}

fn run(app: &App, action: Action) {
    let library = app.global::<ScriptLibrary>();
    if library.get_busy() {
        return;
    }
    library.set_busy(true);
    library.set_status(t("studioScripts.loading").into());
    let reply = app.as_weak();
    let worker = std::thread::Builder::new()
        .name("studio-scripts".into())
        .spawn(move || {
            let result = (|| -> Result<_, String> {
                let dirs = AppDirs::locate()?;
                let store = ScriptStore::new(&dirs.config);
                let mut saved = None;
                let mut deleted = false;
                match action {
                    Action::List => {}
                    Action::Save(draft) => saved = Some(store.save(draft)?),
                    Action::Delete(id, revision) => {
                        store.delete(&id, revision)?;
                        deleted = true;
                    }
                }
                // A successful mutation remains successful even if a subsequent list
                // fails (e.g. another instance takes the lock). Never invite duplicate
                // saves by reporting that the already committed operation failed.
                let rows = store.list();
                if saved.is_none() && !deleted && rows.is_err() {
                    return Err(rows.unwrap_err());
                }
                Ok((rows, saved, deleted))
            })();
            let _ = reply.upgrade_in_event_loop(move |app| {
                let library = app.global::<ScriptLibrary>();
                library.set_busy(false);
                match result {
                    Ok((rows, saved, deleted)) => {
                        let list_failed = rows.is_err();
                        if let Ok(rows) = rows {
                            library.set_rows(ModelRc::new(VecModel::from(
                                rows.into_iter().map(row).collect::<Vec<_>>(),
                            )));
                            library.set_loaded(true);
                        }
                        let status = if let Some(saved) = saved {
                            library.set_draft(row(saved));
                            library.set_dirty(false);
                            t("studioScripts.saved")
                        } else if deleted {
                            library.invoke_new_script();
                            t("studioScripts.deleted")
                        } else {
                            t("studioScripts.loaded")
                        };
                        library.set_confirming_delete(false);
                        library.set_status(if list_failed {
                            format!("{status} {}", t("studioScripts.reload")).into()
                        } else {
                            status.into()
                        });
                    }
                    Err(error) => library.set_status(error.into()),
                }
            });
        });
    if worker.is_err() {
        library.set_busy(false);
        library.set_status(t("studioScripts.workerError").into());
    }
}
