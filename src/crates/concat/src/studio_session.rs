// SPDX-License-Identifier: AGPL-3.0-or-later
//! One native session shared by every Studio cloud adapter. No cookie scraping.
use crate::{
    i18n::t,
    ui::{App, ProductionStudio, StudioAi, StudioBridge, StudioBrowser, StudioSession},
};
use concat_host::{
    studio_auth::{Account, AuthError, AuthManager, LoginAttempt},
    studio_browser::BrowserClient,
    studio_workflows::{WorkflowClient, WorkflowError},
};
use slint::{ComponentHandle, Model, ModelRc};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
};

static READY: AtomicBool = AtomicBool::new(false);
static ENV_ALLOWED: AtomicBool = AtomicBool::new(true);
static MANAGER: OnceLock<Result<AuthManager, AuthError>> = OnceLock::new();
static PENDING_LOGIN_CANCEL: OnceLock<Arc<AtomicBool>> = OnceLock::new();

fn manager() -> Result<&'static AuthManager, AuthError> {
    MANAGER
        .get_or_init(|| {
            let origin = std::env::var("CONCAT_PROMAX_ORIGIN")
                .unwrap_or_else(|_| "http://127.0.0.1:43100".into());
            let dirs = concat_host::AppDirs::locate().map_err(|_| AuthError::Storage)?;
            AuthManager::new(&origin, &dirs.config)
        })
        .as_ref()
        .map_err(|e| *e)
}
pub(crate) fn client() -> Result<WorkflowClient, WorkflowError> {
    if READY.load(Ordering::Acquire) {
        return manager()
            .map_err(|_| WorkflowError::Configuration)?
            .client();
    }
    // Explicit developer pilot only. Logout disables fallback for this app run.
    if ENV_ALLOWED.load(Ordering::Acquire) {
        return WorkflowClient::new(
            &std::env::var("CONCAT_PROMAX_ORIGIN")
                .unwrap_or_else(|_| "http://127.0.0.1:43100".into()),
            std::env::var("CONCAT_PROMAX_ACCESS_TOKEN").ok(),
        );
    }
    Err(WorkflowError::Authentication)
}
pub(crate) fn busy(app: &App) -> bool {
    app.global::<StudioSession>().get_busy()
}
fn cloud_busy(app: &App) -> bool {
    app.global::<StudioAi>().get_busy()
        || app.global::<ProductionStudio>().get_busy()
        || app.global::<StudioBridge>().get_workflow_worker_busy()
        || app.global::<StudioBrowser>().get_busy()
}
fn clear_cloud(app: &App) {
    let ai = app.global::<StudioAi>();
    ai.set_providers(ModelRc::default());
    ai.set_providers_loaded(false);
    ai.set_models(ModelRc::default());
    ai.set_models_loaded(false);
    ai.set_answer("".into());
    ai.set_answered_model("".into());
    ai.set_new_key("".into());
    ai.set_new_open(false);
    ai.set_test_id("".into());
    ai.set_delete_id("".into());
    ai.set_change_id("".into());
    ai.set_confirm_ask(false);
    let bridge = app.global::<StudioBridge>();
    bridge.set_workflows(ModelRc::default());
    bridge.set_workflows_state(0);
    let prod = app.global::<ProductionStudio>();
    prod.set_workflow_ids(ModelRc::default());
    prod.set_workflow_names(ModelRc::default());
    prod.set_runs(ModelRc::default());
    prod.set_history_runs(ModelRc::default());
    prod.set_runs_loaded(false);
    prod.set_ready(false);
    prod.set_confirm_start(false);
    prod.set_active_run("".into());
    prod.set_status("".into());
    // Unknown-outcome latch must not be reset just by switching accounts.
}
fn message(error: AuthError) -> String {
    match error {
        AuthError::Configuration => t("studioSession.configuration"),
        AuthError::Callback => t("studioSession.callbackError"),
        AuthError::Cancelled => t("studioSession.cancelled"),
        AuthError::Rejected => t("studioSession.rejected"),
        AuthError::Unavailable => t("studioSession.unavailable"),
        AuthError::SignedOut => t("studioSession.signedOut"),
        AuthError::Storage => t("studioSession.storageError"),
    }
}
enum Action {
    Login(String),
    Restore,
    Logout,
}
enum Reply {
    Account(Account),
    LoggedOut(bool),
}
fn execute(app: &App, action: Action, cancel: Arc<AtomicBool>) {
    let state = app.global::<StudioSession>();
    if state.get_busy() {
        return;
    }
    if cloud_busy(app) {
        state.set_status(t("studioSession.finishRequests").into());
        return;
    }
    state.set_busy(true);
    state.set_confirm_logout(false);
    state.set_status(t("studioSession.waiting").into());
    cancel.store(false, Ordering::Release);
    ENV_ALLOWED.store(false, Ordering::Release);
    clear_cloud(app);
    let reply = app.as_weak();
    let worker = std::thread::Builder::new()
        .name("studio-session".into())
        .spawn(move || {
            let result = (|| {
                let manager = manager()?;
                match action {
                    Action::Restore => manager.account().map(Reply::Account),
                    Action::Logout => {
                        READY.store(false, Ordering::Release);
                        manager.logout().map(Reply::LoggedOut)
                    }
                    Action::Login(profile_id) => {
                        // Public identifiers copied from the existing ArtCraft identity
                        // config. This is not a server/admin credential or provider key.
                        let origin = std::env::var("CONCAT_SUPABASE_URL")
                            .unwrap_or_else(|_| "https://wcecwdoclzfiwyaavocy.supabase.co".into());
                        let key =
                            std::env::var("CONCAT_SUPABASE_PUBLISHABLE_KEY").unwrap_or_else(|_| {
                                "sb_publishable_-WkSHYM-rjBTAqRXBkX-wA_vi-UyK3l".into()
                            });
                        let attempt = LoginAttempt::new(&origin, &key)?;
                        let browser = BrowserClient::new(
                            &std::env::var("DONUT_BROWSER_API_URL")
                                .unwrap_or_else(|_| "http://127.0.0.1:10108".into()),
                        )
                        .map_err(|_| AuthError::Configuration)?;
                        let profile = browser
                            .profiles()
                            .map_err(|_| AuthError::Unavailable)?
                            .into_iter()
                            .find(|p| p.id == profile_id)
                            .ok_or(AuthError::Rejected)?;
                        if cancel.load(Ordering::Acquire) {
                            return Err(AuthError::Cancelled);
                        }
                        // On uncertain navigation don't open another browser or retry.
                        browser
                            .open_https(&profile, attempt.authorize_url())
                            .map_err(|_| AuthError::Unavailable)?;
                        let code = attempt.wait(&cancel)?;
                        manager
                            .complete_login(&origin, &key, code, &cancel)
                            .map(Reply::Account)
                    }
                }
            })();
            let _ = reply.upgrade_in_event_loop(move |app| {
                let state = app.global::<StudioSession>();
                state.set_busy(false);
                match result {
                    Ok(Reply::Account(account)) => {
                        READY.store(true, Ordering::Release);
                        state.set_signed_in(true);
                        state.set_account(
                            format!(
                                "{} · {} · {}",
                                account.user_id,
                                account.tenant_id,
                                account.roles.join(", ")
                            )
                            .into(),
                        );
                        state.set_status(t("studioSession.connected").into());
                    }
                    Ok(Reply::LoggedOut(remote)) => {
                        state.set_signed_in(false);
                        state.set_account("".into());
                        state.set_status(
                            if remote {
                                t("studioSession.loggedOut")
                            } else {
                                t("studioSession.logoutUnconfirmed")
                            }
                            .into(),
                        );
                    }
                    Err(error) => {
                        if !READY.load(Ordering::Acquire) {
                            state.set_signed_in(false);
                            state.set_account("".into());
                        }
                        state.set_status(message(error).into());
                    }
                }
            });
        });
    if worker.is_err() {
        state.set_busy(false);
        state.set_status(t("studioSession.unavailable").into());
    }
}
pub(crate) fn bind(app: &App) {
    let cancel = Arc::new(AtomicBool::new(false));
    let _ = PENDING_LOGIN_CANCEL.set(cancel.clone());
    let weak = app.as_weak();
    let signal = cancel.clone();
    app.global::<StudioSession>().on_sign_in(move || {
        if let Some(app) = weak.upgrade() {
            if app.global::<StudioSession>().get_signed_in() {
                return;
            }
            let browser = app.global::<StudioBrowser>();
            let profile = usize::try_from(browser.get_index())
                .ok()
                .and_then(|index| browser.get_ids().row_data(index));
            if let Some(profile) = profile {
                execute(&app, Action::Login(profile.to_string()), signal.clone());
            }
        }
    });
    let weak = app.as_weak();
    let signal = cancel.clone();
    app.global::<StudioSession>().on_restore(move || {
        if let Some(app) = weak.upgrade() {
            execute(&app, Action::Restore, signal.clone());
        }
    });
    let weak = app.as_weak();
    let signal = cancel.clone();
    app.global::<StudioSession>().on_logout(move || {
        if let Some(app) = weak.upgrade() {
            if app.global::<StudioSession>().get_confirm_logout() {
                execute(&app, Action::Logout, signal.clone());
            }
        }
    });
    app.global::<StudioSession>().on_cancel(move || {
        cancel.store(true, Ordering::Release);
    });
}

/// Close the one-use OAuth callback listener when the native app exits.
pub(crate) fn cancel_pending_login() {
    if let Some(cancel) = PENDING_LOGIN_CANCEL.get() {
        cancel.store(true, Ordering::Release);
    }
}
