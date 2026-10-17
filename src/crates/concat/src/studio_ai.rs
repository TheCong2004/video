// SPDX-License-Identifier: AGPL-3.0-or-later
//! Non-blocking native provider catalogue and explicit AI tests.
use crate::{
    i18n::t,
    ui::{App, StudioAi, StudioProviderRow},
};
use concat_host::studio_ai::{AiError, Answer, Model as AiModel, Provider};
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
    use concat_host::studio_workflows::WorkflowError;
    match error {
        AiError::Input => "Dữ liệu nhập không hợp lệ. Kiểm tra provider, key và model.".into(),
        AiError::UnknownOutcome => {
            "Chưa xác nhận kết quả; yêu cầu có thể đã dùng hạn mức. Không tự gửi lại.".into()
        }
        AiError::Remote(WorkflowError::Authentication) => {
            "Phiên quản lý OmniRoute local không hợp lệ. Khởi động lại app/runtime; không cần đăng nhập Concat.".into()
        }
        AiError::Remote(WorkflowError::RateLimited) => {
            "Nhà cung cấp đang giới hạn yêu cầu. Vui lòng chờ rồi thử lại.".into()
        }
        AiError::Remote(WorkflowError::Forbidden) => {
            "Không có quyền sử dụng tài nguyên này.".into()
        }
        AiError::Remote(WorkflowError::Configuration) => {
            "Thiếu hoặc lỗi runtime OmniRoute. Cần giải nén/cài đầy đủ ứng dụng.".into()
        }
        AiError::Remote(_) => {
            "Không nhận được phản hồi hợp lệ từ OmniRoute. Kiểm tra dịch vụ và thử tải lại.".into()
        }
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
    let generation = crate::studio_local_ai::generation();
    let is_providers = matches!(action, Action::Providers);
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
            let result: Result<Reply, AiError> = (|| {
                let dirs = concat_host::AppDirs::locate()
                    .map_err(|_| AiError::Remote(concat_host::studio_workflows::WorkflowError::Configuration))?;
                concat_studio::legacy_keys::seal(&dirs.data).map_err(AiError::Remote)?;
                let client = crate::studio_local_ai::client().map_err(AiError::Remote)?;
                // A worker queued before account switching must not dispatch
                // the old form/key into the newly signed-in owner's runtime.
                if generation.is_none() || generation != crate::studio_local_ai::generation() {
                    return Err(AiError::Remote(concat_host::studio_workflows::WorkflowError::Authentication));
                }
                match action {
                    Action::Providers => client.ai_providers().map(Reply::Providers).map_err(AiError::Remote),
                    Action::Models => client.ai_models().map(Reply::Models).map_err(AiError::Remote),
                    Action::Test(id, model) => client.ai_test_connection(&id, &model).map(Reply::Tested),
                    Action::Ask(model, prompt) => client.ai_ask(&model, &prompt).map(Reply::Answer),
                    Action::Add(provider, name, key, model) => client.ai_add_provider(&provider, &name, &key, &model).map(|_| Reply::Changed),
                    Action::Enable(id, active) => client.ai_enable_provider(&id, active).map(|_| Reply::Changed),
                    Action::Delete(id) => client.ai_delete_provider(&id).map(|_| Reply::Changed),
                }
            })();
            let _ = reply.upgrade_in_event_loop(move |app| {
                let state = app.global::<StudioAi>();
                state.set_busy(false);
                if generation != crate::studio_local_ai::generation() {
                    if is_providers {
                        execute(&app, Action::Providers);
                    }
                    return;
                }
                match result {
                    Ok(Reply::Providers(rows)) => {
                        let origin = crate::studio_local_ai::origin()
                            .unwrap_or_else(|| "OmniRoute Local Gateway".into());
                        state.set_backend_origin(origin.into());
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
                        if !state.get_models_loaded() {
                            execute(&app, Action::Models);
                        }
                    }
                    Ok(Reply::Models(rows)) => {
                        let count = rows.len();
                        state.set_models(ModelRc::new(VecModel::from(
                            rows.into_iter()
                                .map(|m| m.id.into())
                                .collect::<Vec<slint::SharedString>>(),
                        )));
                        state.set_model_index(0);
                        state.set_models_loaded(true);
                        state.set_confirm_ask(false);
                        state.set_status(if count == 0 { "Chưa có model khả dụng. Thêm kết nối trong AI Keys hoặc dùng AI Edit → AI bên ngoài.".into() } else { format!("Đã tải {count} model. Danh sách không xác nhận key/hạn mức; chỉ gọi khi bạn bấm gửi.").into() });
                    }
                    Ok(Reply::Tested(valid)) => {
                        state.set_status(
                            if valid {
                                "Đã kiểm tra kết nối key. Bấm Gửi để kiểm tra câu trả lời thật; không xác nhận hạn mức còn lại."
                            } else {
                                "❌ Không thể kết nối tới API Key này."
                            }
                            .into(),
                        );
                        state.set_test_id("".into());
                    }
                    Ok(Reply::Answer(answer)) => {
                        state.set_answer(answer.text.into());
                        state.set_answered_model(answer.model.into());
                        state.set_status("✅ OmniRoute đã tạo câu trả lời thành công.".into());
                    }
                    Ok(Reply::Changed) => {
                        state.set_models_loaded(false);
                        state.set_confirm_ask(false);
                        state.set_new_open(false);
                        state.set_delete_id("".into());
                        state.set_change_id("".into());
                        state.set_status("✅ Đã cập nhật cấu hình OmniRoute Gateway.".into());
                        execute(&app, Action::Providers);
                        // Load models explicitly after provider refresh (one worker at a time).
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
    app.global::<StudioAi>()
        .set_backend_origin("OmniRoute local · workspace trên máy".into());
    app.global::<StudioAi>().set_providers_loaded(false);
    let default_models = vec![
        slint::SharedString::from("gemini/gemini-2.5-flash"),
        slint::SharedString::from("openai/gpt-4o-mini"),
        slint::SharedString::from("auto/coding:pro"),
        slint::SharedString::from("auto/reasoning:pro"),
        slint::SharedString::from("auto/best"),
    ];
    app.global::<StudioAi>().set_models(ModelRc::new(VecModel::from(default_models)));
    app.global::<StudioAi>().set_model_index(0);
    app.global::<StudioAi>().set_models_loaded(true);

    let weak = app.as_weak();
    if let Some(app) = weak.upgrade() {
        if crate::studio_local_ai::generation().is_some() {
            execute(&app, Action::Providers);
            execute(&app, Action::Models);
        }
    }

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
            app.global::<StudioAi>().set_busy(false);
            execute(&app, Action::Providers);
        }
    });

    let weak = app.as_weak();
    app.global::<StudioAi>().on_load_models(move || {
        if let Some(app) = weak.upgrade() {
            app.global::<StudioAi>().set_models_loaded(false);
            execute(&app, Action::Providers);
        }
    });

    let weak = app.as_weak();
    app.global::<StudioAi>().on_test_connection(move || {
        if let Some(app) = weak.upgrade() {
            let state = app.global::<StudioAi>();
            let id = state.get_test_id().to_string();
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

    let weak = app.as_weak();
    app.global::<StudioAi>().on_open_dashboard(move || {
        if let Some(app) = weak.upgrade() {
            execute(&app, Action::Models);
        }
    });

    let weak = app.as_weak();
    app.global::<StudioAi>().on_copy_master_key(move || {
        if let Some(app) = weak.upgrade() {
            app.global::<StudioAi>().set_status(
                "Thông tin xác thực local do ứng dụng quản lý, không dùng master key mẫu.".into(),
            );
        }
    });
    let weak = app.as_weak();
    app.global::<StudioAi>().on_copy_master_endpoint(move || {
        if let Some(app) = weak.upgrade() {
            app.global::<StudioAi>().set_status(
                "OmniRoute local dùng cổng riêng theo phiên; không phải endpoint Cloudflare."
                    .into(),
            );
        }
    });

    let weak = app.as_weak();
    app.global::<StudioAi>().on_quick_add(move |provider| {
        if let Some(app) = weak.upgrade() {
            let state = app.global::<StudioAi>();
            let prov = provider.to_string();
            state.set_new_provider(prov.clone().into());
            let name = match prov.as_str() {
                "gemini" => "Google Gemini",
                "openai" => "ChatGPT OpenAI",
                "grok" => "xAI Grok",
                "anthropic" => "Claude Anthropic",
                "deepseek" => "DeepSeek AI",
                "ollama" => "Ollama Local",
                other => other,
            };
            state.set_new_name(name.into());
            state.set_new_open(true);
        }
    });
}
