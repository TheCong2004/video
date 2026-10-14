// SPDX-License-Identifier: AGPL-3.0-or-later
//! Non-blocking native provider catalogue and explicit AI tests.
use crate::{
    i18n::t,
    ui::{App, StudioAi, StudioProviderRow},
};
use concat_host::{
    studio_ai::{AiError, Answer, Model as AiModel, Provider},
    studio_workflows::WorkflowError,
};
use slint::{ComponentHandle, Model, ModelRc, VecModel};

enum Action {
    Providers,
    Models,
    Test(String, String),
    Ask(String, String),
    Add(String, String, String, String),
    Enable(String, bool),
    Delete(String),
}
enum Reply {
    Providers(Vec<Provider>),
    Models(Vec<AiModel>),
    Tested(bool),
    Answer(Answer),
    Changed,
}

fn message(error: AiError) -> String {
    match error {
        AiError::Input => t("studioAi.invalidInput"),
        AiError::UnknownOutcome => t("studioAi.unknownOutcome"),
        AiError::Remote(error) => match error {
            WorkflowError::Authentication => t("productionStudio.authRequired"),
            WorkflowError::Forbidden => t("productionStudio.accessDenied"),
            WorkflowError::RateLimited => t("productionStudio.rateLimited"),
            WorkflowError::Configuration => t("productionStudio.workflowConfiguration"),
            WorkflowError::InvalidResponse => t("studioAi.invalidResponse"),
            WorkflowError::Unavailable => t("studioAi.unavailable"),
        },
    }
}

fn execute(app: &App, action: Action) {
    if crate::studio_session::busy(app) {
        return;
    }
    let state = app.global::<StudioAi>();
    if state.get_busy() {
        return;
    }
    state.set_busy(true);
    state.set_status("".into());
    if matches!(action, Action::Ask(..)) {
        state.set_answer("".into());
        state.set_answered_model("".into());
    }
    let reply = app.as_weak();
    let worker = std::thread::Builder::new()
        .name("studio-ai".into())
        .spawn(move || {
            let result = (|| {
                let client = crate::studio_session::client().map_err(AiError::Remote)?;
                match action {
                    Action::Providers => client
                        .ai_providers()
                        .map(Reply::Providers)
                        .map_err(AiError::Remote),
                    Action::Models => client
                        .ai_models()
                        .map(Reply::Models)
                        .map_err(AiError::Remote),
                    Action::Test(id, model) => {
                        client.ai_test_connection(&id, &model).map(Reply::Tested)
                    }
                    Action::Ask(model, prompt) => client.ai_ask(&model, &prompt).map(Reply::Answer),
                    Action::Add(provider, name, key, model) => client
                        .ai_add_provider(&provider, &name, &key, &model)
                        .map(|_| Reply::Changed),
                    Action::Enable(id, active) => client
                        .ai_enable_provider(&id, active)
                        .map(|_| Reply::Changed),
                    Action::Delete(id) => client.ai_delete_provider(&id).map(|_| Reply::Changed),
                }
            })();
            let _ = reply.upgrade_in_event_loop(move |app| {
                let state = app.global::<StudioAi>();
                state.set_busy(false);
                match result {
                    Ok(Reply::Providers(rows)) => {
                        state.set_providers(ModelRc::new(VecModel::from(
                            rows.into_iter()
                                .map(|p| StudioProviderRow {
                                    id: p.id.into(),
                                    name: p.name.into(),
                                    provider: p.provider.into(),
                                    active: p.active,
                                    model: p.default_model.into(),
                                })
                                .collect::<Vec<_>>(),
                        )));
                        state.set_providers_loaded(true);
                        state.set_status(t("studioAi.providersLoaded").into());
                    }
                    Ok(Reply::Models(rows)) => {
                        state.set_models(ModelRc::new(VecModel::from(
                            rows.into_iter()
                                .map(|m| m.id.into())
                                .collect::<Vec<slint::SharedString>>(),
                        )));
                        state.set_model_index(0);
                        state.set_models_loaded(true);
                        state.set_confirm_ask(false);
                        state.set_status(t("studioAi.modelsLoaded").into());
                    }
                    Ok(Reply::Tested(valid)) => {
                        state.set_status(
                            if valid {
                                t("studioAi.testPassed")
                            } else {
                                t("studioAi.testFailed")
                            }
                            .into(),
                        );
                        state.set_test_id("".into());
                    }
                    Ok(Reply::Answer(answer)) => {
                        state.set_answer(answer.text.into());
                        state.set_answered_model(answer.model.into());
                        state.set_status(t("studioAi.answered").into());
                    }
                    Ok(Reply::Changed) => {
                        // Do not replay a successful mutation if refreshing later fails.
                        // Clear stale connection/model choices until explicitly reloaded.
                        state.set_providers(ModelRc::default());
                        state.set_providers_loaded(false);
                        state.set_models(ModelRc::default());
                        state.set_models_loaded(false);
                        state.set_confirm_ask(false);
                        state.set_new_open(false);
                        state.set_delete_id("".into());
                        state.set_change_id("".into());
                        state.set_status(t("studioAi.changed").into());
                    }
                    Err(error) => state.set_status(message(error).into()),
                }
            });
        });
    if worker.is_err() {
        state.set_busy(false);
        state.set_status(t("studioAi.unavailable").into());
    }
}

pub(crate) fn bind(app: &App) {
    let weak = app.as_weak();
    app.global::<StudioAi>().on_add_provider(move || {
        if let Some(app) = weak.upgrade() {
            let state = app.global::<StudioAi>();
            if state.get_busy() || !state.get_new_open() {
                return;
            }
            let action = Action::Add(
                state.get_new_provider().to_string(),
                state.get_new_name().to_string(),
                state.get_new_key().to_string(),
                state.get_new_model().to_string(),
            );
            // Clear the visible secret immediately, including the error path.
            state.set_new_key("".into());
            execute(&app, action);
        }
    });
    let weak = app.as_weak();
    app.global::<StudioAi>().on_change_provider(move || {
        if let Some(app) = weak.upgrade() {
            let state = app.global::<StudioAi>();
            let id = state.get_change_id().to_string();
            if state.get_busy() || !state.get_providers().iter().any(|p| p.id.as_str() == id) {
                return;
            }
            let active = state.get_change_active();
            state.set_change_id("".into());
            execute(&app, Action::Enable(id, active));
        }
    });
    let weak = app.as_weak();
    app.global::<StudioAi>().on_delete_provider(move || {
        if let Some(app) = weak.upgrade() {
            let state = app.global::<StudioAi>();
            let id = state.get_delete_id().to_string();
            if state.get_busy() || !state.get_providers().iter().any(|p| p.id.as_str() == id) {
                return;
            }
            state.set_delete_id("".into());
            execute(&app, Action::Delete(id));
        }
    });
    let weak = app.as_weak();
    app.global::<StudioAi>().on_load_providers(move || {
        if let Some(app) = weak.upgrade() {
            execute(&app, Action::Providers);
        }
    });
    let weak = app.as_weak();
    app.global::<StudioAi>().on_load_models(move || {
        if let Some(app) = weak.upgrade() {
            execute(&app, Action::Models);
        }
    });
    let weak = app.as_weak();
    app.global::<StudioAi>().on_test_connection(move || {
        if let Some(app) = weak.upgrade() {
            let state = app.global::<StudioAi>();
            let id = state.get_test_id().to_string();
            // Only an explicitly displayed, loaded connection can be tested.
            if !state.get_providers().iter().any(|p| p.id.as_str() == id) {
                return;
            }
            execute(&app, Action::Test(id, state.get_test_model().to_string()));
        }
    });
    let weak = app.as_weak();
    app.global::<StudioAi>().on_ask(move || {
        if let Some(app) = weak.upgrade() {
            let state = app.global::<StudioAi>();
            if state.get_busy() || !state.get_confirm_ask() {
                return;
            }
            let Some(model) = usize::try_from(state.get_model_index())
                .ok()
                .and_then(|i| state.get_models().row_data(i))
            else {
                return;
            };
            state.set_confirm_ask(false);
            execute(
                &app,
                Action::Ask(model.to_string(), state.get_prompt().to_string()),
            );
        }
    });
}
