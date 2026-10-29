// SPDX-License-Identifier: AGPL-3.0-or-later
//! Concat delegates production connectivity to Nexora; no separate user login.
use crate::ui::{App, StudioSession};
use concat_host::studio_workflows::{WorkflowClient, WorkflowError};
use slint::ComponentHandle;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

static CONNECTION_GENERATION: AtomicU64 = AtomicU64::new(1);
static LEGACY_MANAGER: OnceLock<Result<Arc<concat_host::studio_auth::AuthManager>, WorkflowError>> =
    OnceLock::new();

pub(crate) fn session_generation() -> u64 {
    CONNECTION_GENERATION.load(Ordering::Acquire)
}

pub(crate) fn connection_changed() {
    CONNECTION_GENERATION.fetch_add(1, Ordering::AcqRel);
}

pub(crate) fn client() -> Result<WorkflowClient, WorkflowError> {
    // Preserve the existing credential path for unrelated legacy consumers
    // (e.g. the product catalogue). Production never uses this path or asks
    // the user to establish a separate Concat account.
    let manager = LEGACY_MANAGER
        .get_or_init(|| {
            let origin = std::env::var("CONCAT_PROMAX_ORIGIN")
                .unwrap_or_else(|_| "https://promax-backend.thecong2610.workers.dev".into());
            let dirs = concat_host::AppDirs::locate().map_err(|_| WorkflowError::Configuration)?;
            concat_host::studio_auth::AuthManager::new(&origin, &dirs.config)
                .map(Arc::new)
                .map_err(|_| WorkflowError::Configuration)
        })
        .as_ref()
        .map_err(|e| *e)?;
    manager.client()
}

pub(crate) fn production_client() -> Result<WorkflowClient, WorkflowError> {
    let page = crate::studio_production_pages::selected().ok_or(WorkflowError::Configuration)?;
    let dirs = concat_host::AppDirs::locate().map_err(|_| WorkflowError::Configuration)?;
    let settings = concat_studio::nexora::NexoraSettings::load(&dirs.config)
        .map_err(|_| WorkflowError::Configuration)?;
    let browser = settings
        .client()
        .map_err(|_| WorkflowError::Configuration)?;
    browser.health().map_err(|_| WorkflowError::Unavailable)?;
    WorkflowClient::via_nexora(browser, &page.profile.id)
}

pub(crate) fn busy(_app: &App) -> bool {
    false
}

pub(crate) fn bind(app: &App) {
    crate::studio_local_ai::set_owner("concat-local-workspace-v1");
    let state = app.global::<StudioSession>();
    state.set_busy(false);
    // Local connection is not a signed-in user; never fabricate an account.
    state.set_signed_in(false);
    state.set_account("Nexora local".into());
    state.set_status(
        "Chọn Page/hồ sơ đã tạo ở Nexora. Concat dùng kết nối Nexora, không cần đăng nhập riêng."
            .into(),
    );
}

pub(crate) fn cancel_pending_login() {
    connection_changed();
    crate::ai_editing::shutdown();
}
