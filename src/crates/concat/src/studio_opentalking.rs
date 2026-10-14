// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native controls for the existing local OpenTalking engine.
use crate::{
    i18n::t,
    ui::{
        App, StudioAvatarRow, StudioOpenTalking, StudioSceneRow, StudioVideoJobRow,
        StudioVideoModelRow, StudioVoiceRow,
    },
};
use concat_host::studio_opentalking::{
    Avatar, OpenTalkingClient, OpenTalkingError, Scene, VideoJob, VideoModel, Voice,
};
use slint::{ComponentHandle, Model, ModelRc, VecModel};

enum Action {
    Refresh,
    RefreshJobs,
    Create(String, String, String, String, String, Option<String>),
    Stop(String),
    Delete(String),
    Import(String, String),
}
enum Reply {
    Jobs(Result<Vec<VideoJob>, OpenTalkingError>),
    Refreshed(
        Result<Vec<Avatar>, OpenTalkingError>,
        Result<Vec<Voice>, OpenTalkingError>,
        Result<Vec<Scene>, OpenTalkingError>,
        Result<(String, Vec<VideoModel>), OpenTalkingError>,
        Result<Vec<VideoJob>, OpenTalkingError>,
    ),
    Created(VideoJob, Result<Vec<VideoJob>, OpenTalkingError>),
    Stopped(Result<Vec<VideoJob>, OpenTalkingError>),
    Deleted(Result<Vec<VideoJob>, OpenTalkingError>),
    Imported(std::path::PathBuf, String),
}

fn client() -> Result<OpenTalkingClient, OpenTalkingError> {
    OpenTalkingClient::new(
        &std::env::var("CONCAT_ARTCRAFT_ORIGIN")
            .unwrap_or_else(|_| "http://127.0.0.1:30000".into()),
    )
}

fn message(error: OpenTalkingError) -> slint::SharedString {
    let message = match error {
        OpenTalkingError::Configuration => t("studioOpenTalking.configuration"),
        OpenTalkingError::Unavailable => t("studioOpenTalking.unavailable"),
        OpenTalkingError::NotReady => t("studioOpenTalking.notReady"),
        OpenTalkingError::Input => t("studioOpenTalking.invalidInput"),
        OpenTalkingError::InvalidResponse => t("studioOpenTalking.invalidResponse"),
        OpenTalkingError::UnknownOutcome => t("studioOpenTalking.unknownOutcome"),
        OpenTalkingError::SpeechUnavailable => t("studioOpenTalking.previewServiceFailed"),
    };
    message.into()
}

fn avatars(rows: Vec<Avatar>) -> (ModelRc<StudioAvatarRow>, ModelRc<slint::SharedString>) {
    let labels = rows
        .iter()
        .map(|row| format!("{} · {}", row.name, row.model_type).into())
        .collect::<Vec<slint::SharedString>>();
    let models = rows
        .into_iter()
        .map(|row| StudioAvatarRow {
            id: row.id.into(),
            label: row.name.into(),
            model_type: row.model_type.into(),
        })
        .collect::<Vec<_>>();
    (
        ModelRc::new(VecModel::from(models)),
        ModelRc::new(VecModel::from(labels)),
    )
}

fn voices(rows: Vec<Voice>) -> (ModelRc<StudioVoiceRow>, ModelRc<slint::SharedString>) {
    let labels = rows
        .iter()
        .map(|row| row.label.clone().into())
        .collect::<Vec<slint::SharedString>>();
    let models = rows
        .into_iter()
        .map(|row| StudioVoiceRow {
            id: row.id.into(),
            label: row.label.into(),
        })
        .collect::<Vec<_>>();
    (
        ModelRc::new(VecModel::from(models)),
        ModelRc::new(VecModel::from(labels)),
    )
}

fn scenes(rows: Vec<Scene>) -> (ModelRc<StudioSceneRow>, ModelRc<slint::SharedString>) {
    let mut labels = vec![t("studioOpenTalking.noScene").into()];
    labels.extend(rows.iter().map(|row| row.name.clone().into()));
    let models = rows
        .into_iter()
        .map(|row| StudioSceneRow {
            id: row.id.into(),
            label: row.name.into(),
        })
        .collect::<Vec<_>>();
    (
        ModelRc::new(VecModel::from(models)),
        ModelRc::new(VecModel::from(labels)),
    )
}

fn models(rows: Vec<VideoModel>) -> (ModelRc<StudioVideoModelRow>, ModelRc<slint::SharedString>) {
    let models = rows
        .into_iter()
        .map(|row| StudioVideoModelRow {
            id: row.id.clone().into(),
            label: row.id.into(),
        })
        .collect::<Vec<_>>();
    let labels = models
        .iter()
        .map(|row| row.label.clone())
        .collect::<Vec<_>>();
    (
        ModelRc::new(VecModel::from(models)),
        ModelRc::new(VecModel::from(labels)),
    )
}

fn jobs(rows: Vec<VideoJob>) -> ModelRc<StudioVideoJobRow> {
    ModelRc::new(VecModel::from(
        rows.into_iter()
            .map(|row| StudioVideoJobRow {
                id: row.id.into(),
                title: row.title.into(),
                status: row.status.into(),
                avatar: row.avatar_id.into(),
                driver: row.driver.into(),
                created: row.created_at.into(),
                duration: row.duration.into(),
            })
            .collect::<Vec<_>>(),
    ))
}

fn execute(app: &App, action: Action) {
    if crate::studio_session::busy(app) {
        return;
    }
    let state = app.global::<StudioOpenTalking>();
    if state.get_busy() {
        return;
    }
    state.set_busy(true);
    let previous_avatar =
        selected(&state.get_avatars(), state.get_avatar_index()).map(|row| row.id.to_string());
    let previous_voice =
        selected(&state.get_voices(), state.get_voice_index()).map(|row| row.id.to_string());
    let previous_model =
        selected(&state.get_models(), state.get_model_index()).map(|row| row.id.to_string());
    state.set_confirm_create(false);
    state.set_status(t("studioOpenTalking.working").into());
    let weak = app.as_weak();
    let worker = std::thread::Builder::new()
        .name("studio-opentalking".into())
        .spawn(move || {
            let result = (|| {
                let client = client()?;
                match action {
                    Action::RefreshJobs => Ok(Reply::Jobs(client.jobs())),
                    Action::Refresh => Ok(Reply::Refreshed(
                        client.avatars(),
                        client.voices(),
                        client.scenes(),
                        client.models_with_status(),
                        client.jobs(),
                    )),
                    Action::Create(avatar, voice, driver, title, script, scene) => {
                        let job = client.create_job(
                            &avatar,
                            &voice,
                            &driver,
                            &title,
                            &script,
                            scene.as_deref(),
                        )?;
                        Ok(Reply::Created(job, client.jobs()))
                    }
                    Action::Stop(id) => {
                        client.stop_job(&id)?;
                        Ok(Reply::Stopped(client.jobs()))
                    }
                    Action::Delete(id) => {
                        client.delete_job(&id)?;
                        Ok(Reply::Deleted(client.jobs()))
                    }
                    Action::Import(id, project_path) => {
                        let dirs = concat_host::AppDirs::locate()
                            .map_err(|_| OpenTalkingError::Unavailable)?;
                        client
                            .download_video(&id, &dirs.data.join("opentalking-exports"))
                            .map(|path| Reply::Imported(path, project_path))
                    }
                }
            })();
            let _ = weak.upgrade_in_event_loop(move |app| {
                let state = app.global::<StudioOpenTalking>();
                state.set_busy(false);
                match result {
                    Ok(Reply::Jobs(result)) => {
                        apply_jobs(&state, result);
                    }
                    Ok(Reply::Refreshed(
                        avatar_result,
                        voice_result,
                        scene_result,
                        model_result,
                        job_result,
                    )) => {
                        let mut messages = Vec::new();
                        state.set_catalogs_loaded(
                            avatar_result.is_ok()
                                && voice_result.is_ok()
                                && scene_result.is_ok()
                                && job_result.is_ok(),
                        );
                        let video_ready = model_result
                            .as_ref()
                            .is_ok_and(|(_, rows)| !rows.is_empty());
                        let voice_ready = voice_result.as_ref().is_ok_and(|rows| !rows.is_empty());
                        state.set_engine_status(
                            if video_ready && voice_ready {
                                model_result
                                    .as_ref()
                                    .map(|(status, _)| status.as_str())
                                    .unwrap_or("OFFLINE")
                            } else if video_ready || voice_ready {
                                "PARTIALLY READY"
                            } else {
                                "OFFLINE"
                            }
                            .into(),
                        );
                        match avatar_result {
                            Ok(rows) => {
                                let index =
                                    rows.iter()
                                        .position(|row| Some(&row.id) == previous_avatar.as_ref())
                                        .unwrap_or(0) as i32;
                                let (rows, options) = avatars(rows);
                                state.set_avatars(rows);
                                state.set_avatar_options(options);
                                state.set_avatar_index(index);
                            }
                            Err(error) => {
                                state.set_avatars(ModelRc::default());
                                state.set_avatar_options(ModelRc::default());
                                messages.push(message(error));
                            }
                        }
                        match voice_result {
                            Ok(rows) => {
                                let index =
                                    rows.iter()
                                        .position(|row| Some(&row.id) == previous_voice.as_ref())
                                        .unwrap_or(0) as i32;
                                let (rows, options) = voices(rows);
                                state.set_voices(rows);
                                state.set_voice_options(options);
                                state.set_voice_index(index);
                            }
                            Err(error) => {
                                state.set_voices(ModelRc::default());
                                state.set_voice_options(ModelRc::default());
                                messages.push(message(error));
                            }
                        }
                        match scene_result {
                            Ok(rows) => {
                                let (rows, options) = scenes(rows);
                                state.set_scenes(rows);
                                state.set_scene_options(options);
                                state.set_scene_index(0);
                            }
                            Err(error) => {
                                state.set_scenes(ModelRc::default());
                                state.set_scene_options(ModelRc::default());
                                messages.push(message(error));
                            }
                        }
                        match model_result {
                            Ok((_, rows)) => {
                                let index =
                                    rows.iter()
                                        .position(|row| Some(&row.id) == previous_model.as_ref())
                                        .unwrap_or(0) as i32;
                                let (rows, options) = models(rows);
                                state.set_models(rows);
                                state.set_model_options(options);
                                state.set_model_index(index);
                            }
                            Err(error) => {
                                state.set_models(ModelRc::default());
                                state.set_model_options(ModelRc::default());
                                messages.push(message(error));
                            }
                        }
                        match job_result {
                            Ok(rows) => {
                                state.set_jobs(jobs(rows));
                            }
                            Err(error) => messages.push(message(error)),
                        }
                        state.set_status(if messages.is_empty() {
                            t("studioOpenTalking.loaded").into()
                        } else {
                            messages
                                .into_iter()
                                .next()
                                .unwrap_or_else(|| t("studioOpenTalking.notReady").into())
                        });
                        state.set_jobs_running(
                            state
                                .get_jobs()
                                .iter()
                                .any(|row| matches!(row.status.as_str(), "queued" | "processing")),
                        );
                        refresh_preview(&app);
                    }
                    Ok(Reply::Created(job, job_result)) => {
                        state.set_status(
                            format!("{} {}", t("studioOpenTalking.jobCreated"), job.id).into(),
                        );
                        apply_jobs(&state, job_result);
                    }
                    Ok(Reply::Stopped(job_result)) => {
                        state.set_status(t("studioOpenTalking.jobStopped").into());
                        state.set_stop_id("".into());
                        apply_jobs(&state, job_result);
                    }
                    Ok(Reply::Deleted(job_result)) => {
                        state.set_status(t("studioOpenTalking.jobDeleted").into());
                        state.set_delete_id("".into());
                        apply_jobs(&state, job_result);
                    }
                    Ok(Reply::Imported(path, expected_project)) => {
                        let mut imported = false;
                        let mut project_changed = false;
                        crate::host::Shell::with(|shell, app| {
                            let studio = shell.studio.borrow();
                            let Some(session) = studio.session.as_ref() else {
                                return;
                            };
                            if session.path() != expected_project.as_str() {
                                project_changed = true;
                                return;
                            }
                            drop(studio);
                            shell.studio.borrow_mut().handle(crate::panes::Msg::Media(
                                crate::panes::media_bin::MediaMsg::Import(vec![path]),
                            ));
                            shell.studio.borrow_mut().refresh_art();
                            shell.studio.borrow().publish(&app, &shell.models);
                            imported = true;
                        });
                        state.set_status(if imported {
                            t("studioOpenTalking.imported").into()
                        } else if project_changed {
                            t("studioOpenTalking.projectChanged").into()
                        } else {
                            t("studioOpenTalking.projectRequired").into()
                        });
                    }
                    Err(error) => state.set_status(message(error)),
                }
            });
        });
    if worker.is_err() {
        state.set_busy(false);
        state.set_status(t("studioOpenTalking.unavailable").into());
    }
}

fn apply_jobs(state: &StudioOpenTalking, result: Result<Vec<VideoJob>, OpenTalkingError>) {
    match result {
        Ok(rows) => {
            state.set_jobs_running(
                rows.iter()
                    .any(|row| matches!(row.status.as_str(), "queued" | "processing")),
            );
            state.set_jobs(jobs(rows));
            state.set_catalogs_loaded(true);
        }
        Err(error) => {
            state.set_jobs_running(false);
            // The mutation already succeeded. Keep the explicit success result,
            // but require an intentional refresh to reconcile history.
            state.set_catalogs_loaded(false);
            log::warn!("OpenTalking mutation succeeded but job history refresh failed: {error:?}");
        }
    }
}

fn refresh_preview(app: &App) {
    let state = app.global::<StudioOpenTalking>();
    let Some(avatar) = selected(&state.get_avatars(), state.get_avatar_index()) else {
        state.set_avatar_preview(slint::Image::default());
        return;
    };
    let id = avatar.id.to_string();
    let weak = app.as_weak();
    let _ = std::thread::Builder::new()
        .name("studio-avatar-preview".into())
        .spawn(move || {
            let pixels = (|| {
                let bytes = client().ok()?.avatar_preview(&id).ok()?;
                let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
                    .with_guessed_format()
                    .ok()?;
                let mut limits = image::Limits::default();
                limits.max_image_width = Some(4096);
                limits.max_image_height = Some(4096);
                limits.max_alloc = Some(64 * 1024 * 1024);
                reader.limits(limits);
                Some(reader.decode().ok()?.to_rgba8())
            })();
            let _ = weak.upgrade_in_event_loop(move |app| {
                let state = app.global::<StudioOpenTalking>();
                if selected(&state.get_avatars(), state.get_avatar_index())
                    .is_none_or(|row| row.id.as_str() != id)
                {
                    return;
                }
                state.set_avatar_preview(match pixels {
                    Some(pixels) => slint::Image::from_rgba8(slint::SharedPixelBuffer::<
                        slint::Rgba8Pixel,
                    >::clone_from_slice(
                        pixels.as_raw(),
                        pixels.width(),
                        pixels.height(),
                    )),
                    None => slint::Image::default(),
                });
            });
        });
}

fn selected<T: Clone>(model: &ModelRc<T>, index: i32) -> Option<T> {
    usize::try_from(index)
        .ok()
        .and_then(|index| model.row_data(index))
}

pub(crate) fn bind(app: &App) {
    let weak = app.as_weak();
    app.global::<StudioOpenTalking>().on_refresh_jobs(move || {
        if let Some(app) = weak.upgrade() {
            execute(&app, Action::RefreshJobs);
        }
    });
    let weak = app.as_weak();
    app.global::<StudioOpenTalking>().on_refresh(move || {
        if let Some(app) = weak.upgrade() {
            execute(&app, Action::Refresh);
        }
    });
    let weak = app.as_weak();
    app.global::<StudioOpenTalking>().on_create(move || {
        let Some(app) = weak.upgrade() else { return };
        let state = app.global::<StudioOpenTalking>();
        if state.get_busy() || !state.get_confirm_create() {
            return;
        }
        let Some(avatar) = selected(&state.get_avatars(), state.get_avatar_index()) else {
            return;
        };
        let Some(voice) = selected(&state.get_voices(), state.get_voice_index()) else {
            return;
        };
        let Some(model) = selected(&state.get_models(), state.get_model_index()) else {
            return;
        };
        let scene_index = state.get_scene_index();
        let scene = if scene_index <= 0 {
            None
        } else {
            selected(&state.get_scenes(), scene_index - 1).map(|row| row.id.to_string())
        };
        execute(
            &app,
            Action::Create(
                avatar.id.to_string(),
                voice.id.to_string(),
                model.id.to_string(),
                state.get_title().to_string(),
                state.get_script().to_string(),
                scene,
            ),
        );
    });
    let weak = app.as_weak();
    app.global::<StudioOpenTalking>().on_stop(move |id| {
        if let Some(app) = weak.upgrade() {
            if !app.global::<StudioOpenTalking>().get_busy() {
                execute(&app, Action::Stop(id.to_string()));
            }
        }
    });
    let weak = app.as_weak();
    app.global::<StudioOpenTalking>().on_delete(move |id| {
        if let Some(app) = weak.upgrade() {
            if !app.global::<StudioOpenTalking>().get_busy() {
                execute(&app, Action::Delete(id.to_string()));
            }
        }
    });
    let weak = app.as_weak();
    app.global::<StudioOpenTalking>()
        .on_import_video(move |id| {
            let Some(app) = weak.upgrade() else { return };
            if app.global::<StudioOpenTalking>().get_busy() {
                return;
            }
            let mut project_path = None;
            crate::host::Shell::with(|shell, _| {
                project_path = shell
                    .studio
                    .borrow()
                    .session
                    .as_ref()
                    .map(|session| session.path().to_owned());
            });
            let Some(project_path) = project_path else {
                app.global::<StudioOpenTalking>()
                    .set_status(t("studioOpenTalking.projectRequired").into());
                return;
            };
            execute(&app, Action::Import(id.to_string(), project_path));
        });
    let weak = app.as_weak();
    app.global::<StudioOpenTalking>().on_set_tab(move |tab| {
        if let Some(app) = weak.upgrade() {
            app.global::<StudioOpenTalking>().set_active_tab(tab);
        }
    });
    let weak = app.as_weak();
    app.global::<StudioOpenTalking>()
        .on_select_avatar(move |index| {
            if let Some(app) = weak.upgrade() {
                let state = app.global::<StudioOpenTalking>();
                state.set_avatar_index(index);
                state.set_active_tab(0);
                refresh_preview(&app);
            }
        });
    let weak = app.as_weak();
    app.global::<StudioOpenTalking>()
        .on_select_voice(move |index| {
            if let Some(app) = weak.upgrade() {
                let state = app.global::<StudioOpenTalking>();
                state.set_voice_index(index);
                state.set_active_tab(0);
            }
        });
    let weak = app.as_weak();
    app.global::<StudioOpenTalking>()
        .on_test_voice(move |index| {
            let Some(app) = weak.upgrade() else { return };
            let state = app.global::<StudioOpenTalking>();
            if state.get_busy() {
                return;
            }
            let Some(voice) = selected(&state.get_voices(), index) else {
                return;
            };
            let voice_id = voice.id.to_string();
            let mut playback = None;
            crate::host::Shell::with(|shell, _| {
                playback = Some(std::sync::Arc::clone(&shell.studio.borrow().host.playback));
            });
            let Some(playback) = playback else {
                state.set_status(t("studioOpenTalking.previewFailed").into());
                return;
            };
            state.set_busy(true);
            state.set_status(t("studioOpenTalking.working").into());
            let weak_app = app.as_weak();
            let worker = std::thread::Builder::new()
                .name("studio-voice-preview".into())
                .spawn(move || {
                    let sample_text = "Xin chào, đây là giọng đọc thử nghiệm trong Concat Studio.";
                    let result = client()
                        .and_then(|client| client.synthesize_reply(sample_text, &voice_id))
                        .map_err(message)
                        .and_then(|audio| {
                            crate::studio_live::play_tts(&playback, audio)
                                .map_err(|_| t("studioOpenTalking.previewFailed").into())
                        });
                    let _ = weak_app.upgrade_in_event_loop(move |app| {
                        let state = app.global::<StudioOpenTalking>();
                        state.set_busy(false);
                        state.set_status(match result {
                            Ok(()) => t("studioOpenTalking.previewPlayed").into(),
                            Err(error) => error,
                        });
                    });
                });
            if worker.is_err() {
                state.set_busy(false);
                state.set_status(t("studioOpenTalking.previewFailed").into());
            }
        });
}
