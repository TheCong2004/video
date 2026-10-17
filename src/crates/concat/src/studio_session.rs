// SPDX-License-Identifier: AGPL-3.0-or-later
//! Local-first Studio. Opening the editor never creates a remote identity.
use crate::ui::{App, StudioSession};
use concat_host::studio_workflows::{WorkflowClient, WorkflowError};
use slint::ComponentHandle;

/// Cloud workflows are not anonymously authorized by local app access.
pub(crate) fn client() -> Result<WorkflowClient, WorkflowError> {
    Err(WorkflowError::Configuration)
}

pub(crate) fn busy(_app: &App) -> bool {
    false
}

pub(crate) fn bind(app: &App) {
    // A local storage namespace, not a fake cloud user/session. DPAPI and
    // authenticated loopback management remain enforced by the supervisor.
    crate::studio_local_ai::set_owner("concat-local-workspace-v1");
    let state = app.global::<StudioSession>();
    state.set_signed_in(false);
    state.set_busy(false);
    state.set_account("Local Studio".into());
    state.set_status("Dùng trực tiếp trên máy, không cần tài khoản. AI dùng key riêng; dữ liệu cloud không được mở ẩn danh.".into());
}

pub(crate) fn cancel_pending_login() {
    crate::ai_editing::shutdown();
    // No OAuth callback listener is opened in the local distribution.
}
