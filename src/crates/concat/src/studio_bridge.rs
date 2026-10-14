// SPDX-License-Identifier: AGPL-3.0-or-later

//! Bind the on-demand Studio health check without blocking the window.

use concat_host::studio_bridge::{Reachability, check_local};
use concat_host::studio_workflows::WorkflowError;
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::ui::{App, ProductionStudio, StudioBridge, StudioWorkflowRow};

fn state(result: Reachability) -> i32 {
    match result {
        Reachability::Reachable => 1,
        Reachability::Unavailable => 2,
        Reachability::InvalidResponse => 3,
    }
}

pub(crate) fn bind(app: &App) {
    bind_workflows(app);
    let weak = app.as_weak();
    app.global::<StudioBridge>().on_check_services(move || {
        let Some(app) = weak.upgrade() else { return };
        let bridge = app.global::<StudioBridge>();
        if bridge.get_checking() {
            return;
        }
        bridge.set_checking(true);
        let reply = app.as_weak();
        let worker = std::thread::Builder::new()
            .name("studio-health".into())
            .spawn(move || {
                let promax = state(check_local(43100, "/health/ready"));
                let artcraft = state(check_local(30000, "/health"));
                // Closing the window discards this reply. No global window or
                // project state is accessed by the worker.
                let _ = reply.upgrade_in_event_loop(move |app| {
                    let bridge = app.global::<StudioBridge>();
                    bridge.set_promax_state(promax);
                    bridge.set_artcraft_state(artcraft);
                    bridge.set_checking(false);
                });
            });
        if worker.is_err() {
            bridge.set_promax_state(2);
            bridge.set_artcraft_state(2);
            bridge.set_checking(false);
        }
    });
}

fn workflow_state(error: WorkflowError) -> i32 {
    match error {
        WorkflowError::Authentication => 2,
        WorkflowError::Forbidden => 3,
        WorkflowError::RateLimited => 4,
        WorkflowError::Unavailable => 5,
        WorkflowError::InvalidResponse => 6,
        WorkflowError::Configuration => 7,
    }
}

fn bind_workflows(app: &App) {
    let weak = app.as_weak();
    app.global::<StudioBridge>().on_cancel_workflows(move || {
        if let Some(app) = weak.upgrade() {
            let bridge = app.global::<StudioBridge>();
            // A bounded blocking HTTP call may still be finishing, but its
            // response no longer belongs to this view and cannot be published.
            bridge.set_workflow_generation(bridge.get_workflow_generation().wrapping_add(1));
            bridge.set_workflows_loading(false);
            bridge.set_workflows_state(8);
            bridge.set_workflows(ModelRc::default());
        }
    });
    // The actual worker remains single-flight even after logical cancellation.
    let weak = app.as_weak();
    app.global::<StudioBridge>().on_refresh_workflows(move || {
        let Some(app) = weak.upgrade() else { return };
        if crate::studio_session::busy(&app) {
            return;
        }
        let bridge = app.global::<StudioBridge>();
        if bridge.get_workflow_worker_busy() {
            return;
        }
        bridge.set_workflow_worker_busy(true);
        let generation = bridge.get_workflow_generation().wrapping_add(1);
        bridge.set_workflow_generation(generation);
        bridge.set_workflows_loading(true);
        bridge.set_workflows(ModelRc::default());
        let production = app.global::<ProductionStudio>();
        production.set_workflow_ids(ModelRc::default());
        production.set_workflow_names(ModelRc::default());
        production.set_ready(false);
        let reply = app.as_weak();
        let worker = std::thread::Builder::new()
            .name("studio-workflows".into())
            .spawn(move || {
                let result =
                    crate::studio_session::client().and_then(|client| client.definitions());
                let _ = reply.upgrade_in_event_loop(move |app| {
                    let bridge = app.global::<StudioBridge>();
                    bridge.set_workflow_worker_busy(false);
                    if bridge.get_workflow_generation() != generation {
                        return;
                    }
                    bridge.set_workflows_loading(false);
                    match result {
                        Ok(definitions) => {
                            let production = app.global::<ProductionStudio>();
                            production.set_workflow_ids(ModelRc::new(VecModel::from(
                                definitions
                                    .iter()
                                    .map(|item| item.workflow_id.clone().into())
                                    .collect::<Vec<slint::SharedString>>(),
                            )));
                            production.set_workflow_names(ModelRc::new(VecModel::from(
                                definitions
                                    .iter()
                                    .map(|item| item.name.clone().into())
                                    .collect::<Vec<slint::SharedString>>(),
                            )));
                            production.set_workflow_index(0);
                            production.set_ready(false);
                            let rows: Vec<StudioWorkflowRow> = definitions
                                .into_iter()
                                .map(|item| StudioWorkflowRow {
                                    id: item.workflow_id.into(),
                                    name: item.name.into(),
                                    version: item.version.into(),
                                    description: item.description.into(),
                                    nodes: item.node_count as i32,
                                    edges: item.edge_count as i32,
                                })
                                .collect();
                            bridge.set_workflows(ModelRc::new(VecModel::from(rows)));
                            bridge.set_workflows_state(1);
                        }
                        Err(error) => bridge.set_workflows_state(workflow_state(error)),
                    }
                });
            });
        if worker.is_err() {
            bridge.set_workflow_worker_busy(false);
            bridge.set_workflows_loading(false);
            bridge.set_workflows_state(5);
        }
    });
}
