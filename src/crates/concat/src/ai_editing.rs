// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat contributors
//! Native editing proposals. Workers never replace or mutate the live Session.
//! Cancellation discards results; it does not promise upstream cancellation.
use std::cell::RefCell;
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use concat_agent::{CompiledPlan, EditPlan};
use concat_host::contact_sheet::{ContactSheet, inspection_times, render_sheet};
use concat_host::preview::Monitor;
use concat_project::{DocumentSettings, Project};
use concat_studio::ai::EditPlanOutcome;
use serde_json::{Value, json};
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::{
    host::{self, Shell},
    ui::{AiEditing, App, StudioAi},
};

#[derive(Default)]
struct ProposalState {
    generation: u64,
    epoch: u64,
    revision: u64,
    running: Option<u64>,
    cancel: Option<Arc<AtomicBool>>,
    proposal: Option<CompiledPlan>,
    previewed: bool,
}
thread_local! {
    static STATE: RefCell<ProposalState> = RefCell::new(ProposalState::default());
}

fn clear_view(app: &App) {
    let ui = app.global::<AiEditing>();
    ui.set_ready(false);
    ui.set_preview_ready(false);
    ui.set_before(slint::Image::default());
    ui.set_after(slint::Image::default());
    ui.set_changes(ModelRc::default());
}

fn invalidate(app: &App, status: &str) {
    STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        state.generation = state.generation.wrapping_add(1);
        state.proposal = None;
        state.previewed = false;
        if let Some(cancel) = &state.cancel {
            cancel.store(true, Ordering::Release);
        }
    });
    clear_view(app);
    app.global::<AiEditing>().set_status(status.into());
}

fn synchronize(app: &App) {
    Shell::with(|shell, _| {
        let studio = shell.studio.borrow();
        let epoch = host::project_epoch();
        let revision = studio.session.as_ref().map(|session| session.revision());
        let stale = STATE.with(|slot| {
            let state = slot.borrow();
            (state.proposal.is_some() || state.running.is_some())
                && (state.epoch != epoch || revision != Some(state.revision))
        });
        if stale {
            invalidate(
                app,
                "Timeline changed. This proposal is invalid; generate a new draft after the worker finishes.",
            );
        }
    });
}

/// Share only editable timeline facts, not source paths, tokens or unrelated projects.
fn context(project: &Project) -> Result<Value, String> {
    let timeline = project.active();
    if timeline.clips.len() > 256 || project.media.len() > 256 || timeline.tracks.len() > 256 {
        return Err("This first editing slice supports at most 256 timeline clips".into());
    }
    Ok(json!({
        "tracks": timeline.tracks.iter().map(|track| json!({"id":track.id})).collect::<Vec<_>>(),
        "media": project.media.iter().map(|media| json!({"id":media.id,"kind":format!("{:?}",media.kind),"duration":media.duration})).collect::<Vec<_>>(),
        "clips": timeline.clips.iter().map(|clip| json!({
            "id":clip.id,"trackId":clip.track_id,"mediaId":clip.media_id,"start":clip.start,
            "duration":clip.duration,"kind":format!("{:?}",clip.kind)
        })).collect::<Vec<_>>()
    }))
}

fn diff_report(before: &Project, after: &Project) -> String {
    let was = &before.active().clips;
    let now = &after.active().clips;
    let created = now
        .iter()
        .filter(|clip| !was.iter().any(|old| old.id == clip.id))
        .count();
    let removed = was
        .iter()
        .filter(|clip| !now.iter().any(|new| new.id == clip.id))
        .count();
    let changed = now
        .iter()
        .filter(|clip| was.iter().any(|old| old.id == clip.id && old != *clip))
        .count();
    let duration = |project: &Project| {
        project
            .active()
            .clips
            .iter()
            .map(|clip| clip.start + clip.duration)
            .fold(0.0, f64::max)
    };
    format!(
        "Native diff: +{created} clips, -{removed} clips, {changed} changed · {:.2}s → {:.2}s",
        duration(before),
        duration(after)
    )
}

fn same_scope(
    generation: u64,
    request: u64,
    origin_epoch: u64,
    epoch: u64,
    revision: u64,
    live_revision: Option<u64>,
) -> bool {
    generation == request && origin_epoch == epoch && live_revision == Some(revision)
}

fn generate(app: &App) {
    if crate::studio_session::busy(app) {
        return;
    }
    synchronize(app);
    let ai = app.global::<StudioAi>();
    let model = ai
        .get_models()
        .row_data(ai.get_model_index().max(0) as usize)
        .map(|model| model.to_string())
        .unwrap_or_default();
    let prompt = app.global::<AiEditing>().get_prompt().to_string();
    let agent_mode = app.global::<AiEditing>().get_agent_mode();
    let max_turns = app.global::<AiEditing>().get_max_turns().clamp(1.0, 12.0) as u32;
    if model.is_empty() || prompt.trim().is_empty() || prompt.len() > 8000 {
        app.global::<AiEditing>().set_status(
            "Choose a configured model and enter an instruction (maximum 8000 bytes)".into(),
        );
        return;
    }
    Shell::with(move |shell, app| {
        if STATE.with(|slot| slot.borrow().running.is_some()) {
            return;
        }
        let studio = shell.studio.borrow();
        let Some(session) = studio.session.as_ref() else {
            app.global::<AiEditing>()
                .set_status("Open a project before requesting an edit".into());
            return;
        };
        let snapshot = session.planning_snapshot();
        let selection = studio.selection.clone();
        let context = match context(snapshot.project()) {
            Ok(context) => context,
            Err(error) => {
                app.global::<AiEditing>().set_status(error.into());
                return;
            }
        };
        let epoch = host::project_epoch();
        let revision = snapshot.revision();
        let timeline = snapshot.project().active_timeline_id.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let id = STATE.with(|slot| {
            let mut state = slot.borrow_mut();
            state.generation = state.generation.wrapping_add(1);
            state.epoch = epoch;
            state.revision = revision;
            state.proposal = None;
            state.previewed = false;
            state.running = Some(state.generation);
            state.cancel = Some(cancel.clone());
            state.generation
        });
        drop(studio);
        clear_view(&app);
        let ui = app.global::<AiEditing>();
        ui.set_busy(true);
        ui.set_status(if agent_mode { format!("Agent may use up to {max_turns} provider turns. Tools are read-only until human approval.") } else { "Requesting one structured edit plan. No changes are applied.".into() }.into());
        host::spawn(
            move || -> Result<(CompiledPlan, String), String> {
                if cancel.load(Ordering::Acquire) {
                    return Err("Canceled before request".into());
                }
                let client = crate::studio_session::client().map_err(|_| {
                    "Studio session is unavailable; sign in through Studio settings".to_string()
                })?;
                if agent_mode {
                    use concat_agent::runtime::{GatewayProvider, RunLimits, run};
                    let mut provider = GatewayProvider::new(client, model.clone())
                        .map_err(|error| error.to_string())?;
                    let result = run(
                        &snapshot,
                        &selection,
                        &prompt,
                        RunLimits {
                            max_turns,
                            ..Default::default()
                        },
                        &cancel,
                        &mut provider,
                    )
                    .map_err(|error| error.to_string())?;
                    let report = format!(
                        "{model} · {} actual reported tokens · {}",
                        result.tokens,
                        result.activity.join("; ")
                    );
                    return Ok((result.proposal, report));
                }
                let answer = client.ai_edit_plan(&model, &prompt, &timeline, revision, &context)
                .map_err(|_| "AI request failed or has an unknown outcome. No edit was applied; it will not be retried automatically.".to_string())?;
                if cancel.load(Ordering::Acquire) {
                    return Err(
                        "Response discarded. Provider usage may still have occurred.".into(),
                    );
                }
                if answer.outcome != EditPlanOutcome::Plan {
                    return Err(format!(
                        "Provider returned {:?}; no edit was applied",
                        answer.outcome
                    ));
                }
                let plan = answer.plan.ok_or("Structured response has no plan")?;
                let compiled = EditPlan::parse(&plan.to_string())
                    .and_then(|plan| plan.compile(&snapshot))
                    .map_err(|error| error.to_string())?;
                Ok((compiled, answer.model))
            },
            move |studio, app, _, result| {
                let accepted = STATE.with(|slot| {
                    let mut state = slot.borrow_mut();
                    if state.running != Some(id) {
                        return false;
                    }
                    state.running = None;
                    state.cancel = None;
                    same_scope(
                        state.generation,
                        id,
                        epoch,
                        host::project_epoch(),
                        revision,
                        studio.session.as_ref().map(|session| session.revision()),
                    )
                });
                app.global::<AiEditing>().set_busy(false);
                if !accepted {
                    return;
                }
                match result {
                    Ok((proposal, model)) => {
                        let ui = app.global::<AiEditing>();
                        let mut changes = Vec::new();
                        if let Some(session) = studio.session.as_ref() {
                            changes.push(slint::SharedString::from(diff_report(
                                session.planning_snapshot().project(),
                                proposal.draft().project(),
                            )));
                        }
                        changes.extend(
                            proposal
                                .summaries()
                                .iter()
                                .map(|summary| slint::SharedString::from(summary.as_str())),
                        );
                        ui.set_changes(ModelRc::new(VecModel::from(changes)));
                        ui.set_ready(true);
                        ui.set_status(format!("Draft from {model}. Inspect changes and render a draft preview before approval.").into());
                        STATE.with(|slot| slot.borrow_mut().proposal = Some(proposal));
                    }
                    Err(error) => app.global::<AiEditing>().set_status(error.into()),
                }
            },
        );
    });
}

fn render(
    project: &Project,
    settings: &DocumentSettings,
    path: &Path,
    dirs: &concat_host::AppDirs,
    times: &[f64],
    cancel: &AtomicBool,
) -> Result<ContactSheet, String> {
    let title_count = project
        .active()
        .clips
        .iter()
        .filter(|clip| {
            matches!(
                clip.kind,
                concat_project::model::ClipKind::Text | concat_project::model::ClipKind::Shape
            )
        })
        .count();
    let frame_bytes = u64::from(settings.width) * u64::from(settings.height) * 4;
    if frame_bytes == 0
        || frame_bytes > 32 * 1024 * 1024
        || frame_bytes.saturating_mul(title_count as u64) > 128 * 1024 * 1024
    {
        return Err("Full-geometry inspection exceeds frame/title memory budget".into());
    }
    let monitor = Monitor::with_pool(Arc::new(concat_media::ReaderPool::new(64 * 1024 * 1024, 4)));
    let mut clips = concat_export::flatten::flatten_timeline_in(project, None, Some(path));
    // The in-memory preview must not sweep the live monitor's title cache.
    let preview_dirs = concat_host::AppDirs {
        config: dirs.config.clone(),
        data: dirs.data.join("ai-draft-preview"),
    };
    let titles = concat_host::Titles::new(&preview_dirs);
    let painted = titles.clips_live(project, settings.width, settings.height);
    let expected = project
        .active()
        .clips
        .iter()
        .filter(|clip| {
            matches!(
                clip.kind,
                concat_project::model::ClipKind::Text | concat_project::model::ClipKind::Shape
            )
        })
        .count();
    if painted.len() != expected {
        return Err(
            "Some draft titles/shapes could not be rendered; preview is not trusted".into(),
        );
    }
    for title in painted {
        if let Some(frame) = title.frame {
            monitor.hold_still(Path::new(&title.clip.path), frame);
        }
        clips.push(title.clip);
    }
    render_sheet(
        &monitor,
        Arc::new(clips),
        settings,
        times,
        project.active().video.color_space,
        cancel,
    )
}

fn preview(app: &App) {
    synchronize(app);
    Shell::with(|shell, app| {
        if STATE.with(|slot| slot.borrow().running.is_some()) {
            return;
        }
        let studio = shell.studio.borrow();
        let Some(session) = studio.session.as_ref() else {
            return;
        };
        let (id, draft) = match STATE.with(|slot| {
            let mut state = slot.borrow_mut();
            let draft = state.proposal.as_ref()?.draft().project().clone();
            let id = state.generation;
            state.running = Some(id);
            state.previewed = false;
            Some((id, draft))
        }) {
            Some(value) => value,
            None => return,
        };
        let current = session.planning_snapshot().project().clone();
        let settings = session.settings();
        let path = std::path::PathBuf::from(session.path());
        let dirs = studio.host.dirs.clone();
        let epoch = host::project_epoch();
        let time = f64::from(app.global::<AiEditing>().get_time());
        let cancel = Arc::new(AtomicBool::new(false));
        STATE.with(|slot| slot.borrow_mut().cancel = Some(cancel.clone()));
        drop(studio);
        let ui = app.global::<AiEditing>();
        ui.set_busy(true);
        ui.set_preview_ready(false);
        ui.set_status(
            "Rendering full-geometry native before/draft contact sheets off the UI thread".into(),
        );
        host::spawn(
            move || -> Result<(ContactSheet, ContactSheet), String> {
                let times = inspection_times(
                    &current,
                    &draft,
                    time,
                    settings.rate_num as f64 / settings.rate_den as f64,
                )?;
                Ok((
                    render(&current, &settings, &path, &dirs, &times, &cancel)?,
                    render(&draft, &settings, &path, &dirs, &times, &cancel)?,
                ))
            },
            move |studio, app, _, result| {
                let accepted = STATE.with(|slot| {
                    let mut state = slot.borrow_mut();
                    if state.running != Some(id) {
                        return false;
                    }
                    state.running = None;
                    state.cancel = None;
                    same_scope(
                        state.generation,
                        id,
                        epoch,
                        host::project_epoch(),
                        state.revision,
                        studio.session.as_ref().map(|session| session.revision()),
                    )
                });
                app.global::<AiEditing>().set_busy(false);
                if !accepted {
                    invalidate(
                        app,
                        "Timeline changed or preview canceled. Generate a new draft.",
                    );
                    return;
                }
                if !app.global::<AiEditing>().get_ready() {
                    return;
                }
                match result {
                    Ok((before, after)) => {
                        let labels = after
                            .times
                            .iter()
                            .enumerate()
                            .map(|(index, time)| format!("{}: {time:.3}s", index + 1))
                            .collect::<Vec<_>>()
                            .join(" · ");
                        let image =
                            |sheet: ContactSheet| {
                                slint::Image::from_rgba8(slint::SharedPixelBuffer::<
                                    slint::Rgba8Pixel,
                                >::clone_from_slice(
                                    &sheet.pixels, sheet.width, sheet.height
                                ))
                            };
                        let ui = app.global::<AiEditing>();
                        ui.set_before(image(before));
                        ui.set_after(image(after));
                        ui.set_preview_ready(true);
                        ui.set_status(format!("Native contact sheets ready (row-major timestamps): {labels}. Approval applies one undo step.").into());
                        STATE.with(|slot| slot.borrow_mut().previewed = true);
                    }
                    Err(error) => app.global::<AiEditing>().set_status(error.into()),
                }
            },
        );
    });
}

pub fn bind(app: &App) {
    app.global::<AiEditing>()
        .on_generate(|| Shell::with(|_, app| generate(&app)));
    app.global::<AiEditing>()
        .on_preview(|| Shell::with(|_, app| preview(&app)));
    app.global::<AiEditing>()
        .on_synchronize(|| Shell::with(|_, app| synchronize(&app)));
    app.global::<AiEditing>().on_cancel(|| Shell::with(|_,app|
        invalidate(&app,"Canceled locally. Waiting for worker to finish; provider request/usage may still complete.")));
    app.global::<AiEditing>().on_reject(|| {
        Shell::with(|_, app| invalidate(&app, "Draft rejected. Timeline is unchanged."))
    });
    app.global::<AiEditing>().on_approve(|| {
        Shell::with(|shell, app| {
            synchronize(&app);
            let proposal = STATE.with(|slot| {
                let mut state = slot.borrow_mut();
                if state.running.is_some() || !state.previewed {
                    return None;
                }
                state.proposal.take()
            });
            let Some(proposal) = proposal else {
                return;
            };
            let mut studio = shell.studio.borrow_mut();
            let result = studio.approve_ai_draft(proposal.into_draft());
            clear_view(&app);
            let status = match result {
                Ok(()) => "Applied approved draft. Undo reverses the entire edit.".into(),
                Err(error) => error,
            };
            app.global::<AiEditing>().set_status(status.into());
            studio.publish(&app, &shell.models);
        })
    });
}

/// Account changes invalidate proposals even when the native edit stayed put.
pub(crate) fn account_changed(app: &App) {
    invalidate(
        app,
        "Studio account changed. Reload models and generate a new draft.",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn proposal_delivery_rejects_cancel_replaced_project_edit_and_close() {
        assert!(same_scope(3, 3, 7, 7, 9, Some(9)));
        assert!(!same_scope(4, 3, 7, 7, 9, Some(9)));
        assert!(!same_scope(3, 3, 7, 8, 9, Some(9)));
        assert!(!same_scope(3, 3, 7, 7, 9, Some(10)));
        assert!(!same_scope(3, 3, 7, 7, 9, None));
    }

    #[test]
    fn context_contains_timing_not_title_contents_or_media_fields() {
        let mut editor = concat_project::Editor::new();
        editor
            .apply(concat_project::Command::AddTextClip {
                track_id: None,
                above: true,
                start: 1.0,
                style: Some(concat_project::model::TextStyle {
                    content: "private title contents".into(),
                    ..Default::default()
                }),
                duration: Some(3.0),
                offset_y: None,
            })
            .unwrap();
        let data = context(editor.project()).unwrap();
        assert_eq!(data["clips"][0]["start"], json!(1.0));
        assert_eq!(data["clips"][0]["duration"], json!(3.0));
        assert!(data["clips"][0].get("path").is_none());
        assert!(data["clips"][0].get("text").is_none());
        assert!(!data.to_string().contains("private title contents"));
    }
}
