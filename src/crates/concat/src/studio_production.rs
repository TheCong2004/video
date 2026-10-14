// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native workflow execution. No automatic submission or retry of mutations.
use crate::i18n::t;
use crate::ui::{App, ProductionStudio, StudioRunRow};
use concat_host::studio_workflows::{
    ProductionPlan as Plan, RunError, RunSummary, WorkflowClient, WorkflowError,
};
use slint::{ComponentHandle, Model, ModelRc, VecModel};

enum Action {
    List,
    Check(Plan),
    Start(Plan),
    Cancel(String),
    Resume(String),
}
enum Reply {
    Runs(Vec<RunSummary>),
    Checked(Plan),
    Started(RunSummary),
    Cancelled(RunSummary),
    Resumed(RunSummary),
}

fn client() -> Result<WorkflowClient, RunError> {
    crate::studio_session::client().map_err(RunError::Remote)
}

fn plan(app: &App) -> Result<Plan, RunError> {
    let state = app.global::<ProductionStudio>();
    let index = usize::try_from(state.get_workflow_index()).map_err(|_| RunError::Input)?;
    let workflow = state
        .get_workflow_ids()
        .row_data(index)
        .ok_or(RunError::Input)?
        .to_string();
    Plan::new(
        workflow,
        &state.get_brief(),
        &state.get_sources(),
        state.get_scenes(),
        state.get_duration(),
        &state.get_extra_inputs(),
    )
}

fn row(run: RunSummary) -> StudioRunRow {
    StudioRunRow {
        id: run.id.into(),
        workflow: run.workflow_id.into(),
        status: run.status.into(),
        node: run.node.into(),
        created: run.created_at.into(),
        completed: run.completed_steps as i32,
        total: run.total_steps as i32,
    }
}

fn message(error: RunError) -> String {
    match error {
        RunError::Input => t("nativeStudio.invalidPlan"),
        RunError::NotReady => t("nativeStudio.workerMissing"),
        RunError::UnknownOutcome => t("nativeStudio.unknownOutcome"),
        RunError::Remote(WorkflowError::Authentication) => t("productionStudio.authRequired"),
        RunError::Remote(WorkflowError::Forbidden) => t("productionStudio.accessDenied"),
        RunError::Remote(WorkflowError::RateLimited) => t("productionStudio.rateLimited"),
        RunError::Remote(WorkflowError::Configuration) => {
            t("productionStudio.workflowConfiguration")
        }
        RunError::Remote(WorkflowError::InvalidResponse) => t("productionStudio.workflowInvalid"),
        RunError::Remote(WorkflowError::Unavailable) => t("productionStudio.workflowUnavailable"),
    }
}

pub(crate) fn bind(app: &App) {
    let prepared = std::sync::Arc::new(std::sync::Mutex::new(None::<Plan>));
    let weak = app.as_weak();
    app.global::<ProductionStudio>().on_load_runs(move || {
        if let Some(app) = weak.upgrade() {
            run(&app, Action::List, None);
        }
    });
    let weak = app.as_weak();
    let gate = prepared.clone();
    app.global::<ProductionStudio>().on_check_plan(move || {
        let Some(app) = weak.upgrade() else {
            return;
        };
        if app.global::<ProductionStudio>().get_busy() {
            return;
        }
        *gate.lock().unwrap() = None;
        match plan(&app) {
            Ok(plan) => run(&app, Action::Check(plan), Some(gate.clone())),
            Err(error) => app
                .global::<ProductionStudio>()
                .set_status(message(error).into()),
        }
    });
    let weak = app.as_weak();
    let gate = prepared;
    app.global::<ProductionStudio>().on_start_run(move || {
        let Some(app) = weak.upgrade() else {
            return;
        };
        let state = app.global::<ProductionStudio>();
        if state.get_busy()
            || state.get_uncertain()
            || !state.get_confirm_start()
            || !state.get_ready()
        {
            return;
        }
        let current = plan(&app);
        let prepared = gate.lock().unwrap().take();
        state.set_ready(false);
        state.set_confirm_start(false);
        match (current, prepared) {
            (Ok(current), Some(prepared)) if current == prepared => {
                run(&app, Action::Start(current), None)
            }
            _ => state.set_status(t("nativeStudio.preflightAgain").into()),
        }
    });
    let weak = app.as_weak();
    app.global::<ProductionStudio>().on_cancel_run(move |id| {
        if let Some(app) = weak.upgrade() {
            let state = app.global::<ProductionStudio>();
            if id.is_empty()
                || state.get_cancel_id() != id
                || state.get_busy()
                || state.get_uncertain()
            {
                return;
            }
            run(&app, Action::Cancel(id.to_string()), None);
        }
    });
    let weak = app.as_weak();
    app.global::<ProductionStudio>().on_resume_run(move |id| {
        let Some(app) = weak.upgrade() else {
            return;
        };
        let state = app.global::<ProductionStudio>();
        if id.is_empty()
            || id != state.get_resume_id().as_str()
            || state.get_busy()
            || state.get_uncertain()
        {
            return;
        }
        run(&app, Action::Resume(id.to_string()), None);
    });
}

fn run(app: &App, action: Action, gate: Option<std::sync::Arc<std::sync::Mutex<Option<Plan>>>>) {
    if crate::studio_session::busy(app) {
        return;
    }
    let state = app.global::<ProductionStudio>();
    if state.get_busy() {
        return;
    }
    state.set_busy(true);
    state.set_ready(false);
    state.set_status(t("nativeStudio.working").into());
    let weak = app.as_weak();
    let spawned = std::thread::Builder::new()
        .name("studio-production".into())
        .spawn(move || {
            let result = (|| {
                let client = client()?;
                match action {
                    Action::List => client.runs().map(Reply::Runs).map_err(RunError::Remote),
                    Action::Check(plan) => {
                        let result = client.preflight(&plan.workflow, &plan.inputs)?;
                        if !result.can_start || (result.requires_worker && !result.worker_available)
                        {
                            return Err(RunError::NotReady);
                        }
                        Ok(Reply::Checked(plan))
                    }
                    Action::Start(plan) => client
                        .start(&plan.workflow, &plan.inputs)
                        .map(Reply::Started),
                    Action::Cancel(id) => client.cancel_run(&id).map(Reply::Cancelled),
                    Action::Resume(id) => client.resume_run(&id).map(Reply::Resumed),
                }
            })();
            let _ = weak.upgrade_in_event_loop(move |app| {
                let state = app.global::<ProductionStudio>();
                state.set_busy(false);
                match result {
                    Ok(Reply::Runs(runs)) => {
                        state.set_history_runs(ModelRc::new(VecModel::from(
                            runs.iter()
                                .filter(|run| {
                                    matches!(
                                        run.status.as_str(),
                                        "completed" | "failed" | "cancelled"
                                    )
                                })
                                .cloned()
                                .map(row)
                                .collect::<Vec<_>>(),
                        )));
                        state.set_runs(ModelRc::new(VecModel::from(
                            runs.into_iter().map(row).collect::<Vec<_>>(),
                        )));
                        state.set_runs_loaded(true);
                        state.set_status(if state.get_uncertain() {
                            t("nativeStudio.unknownOutcome").into()
                        } else {
                            t("nativeStudio.runsLoaded").into()
                        });
                        // Unknown submission outcome is deliberately NOT cleared by
                        // loading a page: absence on page 1 proves nothing.
                    }
                    Ok(Reply::Checked(checked)) => {
                        if plan(&app).ok().as_ref() == Some(&checked) {
                            if let Some(gate) = gate {
                                *gate.lock().unwrap() = Some(checked);
                            }
                            state.set_ready(true);
                            state.set_status(t("nativeStudio.preflightReady").into());
                        } else {
                            state.set_status(t("nativeStudio.preflightAgain").into());
                        }
                    }
                    Ok(Reply::Started(run)) => {
                        state.set_active_run(run.id.clone().into());
                        state.set_runs(ModelRc::new(VecModel::from(vec![row(run)])));
                        state.set_status(t("nativeStudio.submitted").into());
                    }
                    Ok(Reply::Cancelled(run)) => {
                        state.set_cancel_id("".into());
                        state.set_runs(ModelRc::new(VecModel::from(vec![row(run)])));
                        state.set_status(t("nativeStudio.cancelRequested").into());
                    }
                    Ok(Reply::Resumed(run)) => {
                        state.set_resume_id("".into());
                        state.set_runs(ModelRc::new(VecModel::from(vec![row(run)])));
                        state.set_status(t("nativeStudio.resumeRequested").into());
                    }
                    Err(error) => {
                        if error == RunError::UnknownOutcome {
                            state.set_uncertain(true);
                        }
                        state.set_status(message(error).into());
                    }
                }
            });
        });
    if spawned.is_err() {
        state.set_busy(false);
        state.set_status(t("nativeStudio.threadError").into());
    }
}
