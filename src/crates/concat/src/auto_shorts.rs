// SPDX-License-Identifier: AGPL-3.0-or-later
//! Window-owned Auto Shorts jobs. Slow media/file work never runs on Slint's thread.
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use concat_agent::highlight::{Genre, Highlight};
use concat_core::frame::Frame;
use concat_media::{DecodeOptions, Decoder, FrameSource};
use concat_shorts::{Analysis, Config, Engine, Source};
use concat_studio::shorts_jobs::{SourceFingerprint, StableFolderSampler, Stage};
use concat_project::Command;
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::{
    host::{self, Shell},
    ui::{App, AutoShorts, ShortsRow, StudioAi},
};

#[path = "auto_shorts_transcript.rs"]
mod transcript;

struct State {
    generation: u64,
    running: Option<u64>,
    cancel: Option<Arc<AtomicBool>>,
    analysis_engine: Option<Engine>,
    source: Option<Source>,
    transcript: Option<PathBuf>,
    analysis: Option<Analysis>,
    transcript_review: transcript::Workbench,
    thumbnails: BTreeMap<String, Frame>,
    analysis_scope: Option<(u64, u64)>,
    selected: BTreeSet<String>,
    output: Option<PathBuf>,
    watch: Option<PathBuf>,
    watch_sampler: Arc<Mutex<StableFolderSampler>>,
    watch_pending: VecDeque<SourceFingerprint>,
    watch_paused: bool,
    timer: Option<slint::Timer>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            generation: 0,
            running: None,
            cancel: None,
            analysis_engine: None,
            source: None,
            transcript: None,
            analysis: None,
            transcript_review: transcript::Workbench::default(),
            thumbnails: BTreeMap::new(),
            analysis_scope: None,
            selected: BTreeSet::new(),
            output: None,
            watch: None,
            watch_sampler: Arc::new(Mutex::new(StableFolderSampler::new(2, 256))),
            watch_pending: VecDeque::new(),
            watch_paused: false,
            timer: None,
        }
    }
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }

fn begin(app: &App, message: &str) -> Option<(u64, Arc<AtomicBool>)> {
    let token = STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        if state.running.is_some() {
            return None;
        }
        state.generation = state.generation.wrapping_add(1);
        let id = state.generation;
        let cancel = Arc::new(AtomicBool::new(false));
        state.running = Some(id);
        state.cancel = Some(cancel.clone());
        Some((id, cancel))
    })?;
    app.global::<AutoShorts>().set_busy(true);
    app.global::<AutoShorts>().set_progress(0.0);
    app.global::<AutoShorts>().set_status(message.into());
    Some(token)
}

fn finish(app: &App, id: u64) -> bool {
    let (owned, accepted) = STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        if state.running != Some(id) {
            return (false, false);
        }
        let accepted = state.generation == id
            && state
                .cancel
                .as_ref()
                .is_some_and(|c| !c.load(Ordering::Acquire));
        state.running = None;
        state.cancel = None;
        state.analysis_engine = None;
        (true, accepted)
    });
    if owned {
        app.global::<AutoShorts>().set_busy(false);
        if !accepted {
            app.global::<AutoShorts>()
                .set_status("Đã hủy tác vụ; không áp dụng kết quả trả về muộn.".into());
        }
    }
    accepted
}

fn stage_label(stage: Stage) -> &'static str {
    match stage {
        Stage::Queued => "Đã xếp hàng",
        Stage::Analyzing => "Đang đọc và phân tích video",
        Stage::Transcribing => "Đang nhận dạng lời nói",
        Stage::DetectingHighlights => "Đang tìm đoạn nổi bật",
        Stage::PreparingEdit => "Đang chuẩn bị bản dựng",
        Stage::Rendering => "Đang xuất video",
        Stage::QualityChecking => "Đang kiểm tra video đầu ra",
        Stage::Completed => "Đã hoàn thành",
        Stage::Failed => "Tác vụ thất bại",
        Stage::Cancelled => "Đã hủy",
        Stage::NeedsReview => "Cần bạn duyệt",
        Stage::Interrupted => "Tác vụ bị gián đoạn",
    }
}

fn progress(id: u64) -> Arc<dyn Fn(Stage, f32) + Send + Sync> {
    let last = Arc::new(Mutex::new(None::<(Stage, Instant)>));
    Arc::new(move |stage, value| {
        if let Ok(mut previous) = last.lock() {
            let now = Instant::now();
            if previous.as_ref().is_some_and(|(was, at)| {
                *was == stage && now.duration_since(*at) < Duration::from_millis(100)
            }) && value < 1.0
            {
                return;
            }
            *previous = Some((stage, now));
        }
        let _ = slint::invoke_from_event_loop(move || {
            Shell::with(|_, app| {
                let current = STATE.with(|slot| {
                    let state = slot.borrow();
                    state.running == Some(id) && state.generation == id
                });
                if current {
                    app.global::<AutoShorts>()
                        .set_progress(if value.is_finite() {
                            value.clamp(0.0, 1.0)
                        } else {
                            0.0
                        });
                    app.global::<AutoShorts>()
                        .set_status(stage_label(stage).into());
                }
            })
        });
    })
}

fn dimensions(aspect: i32, quality: i32) -> (u32, u32) {
    let short = match quality {
        1 => 1080,
        2 => 2160,
        _ => 720,
    };
    match aspect {
        1 => (short * 16 / 9, short),
        2 => (short, short),
        3 => (short, short * 5 / 4),
        _ => (short, short * 16 / 9),
    }
}

fn config(app: &App) -> Config {
    let ui = app.global::<AutoShorts>();
    let (width, height) = dimensions(ui.get_aspect_index(), ui.get_quality_index());
    let genre = match ui.get_genre_index() {
        1 => Genre::Podcast,
        2 => Genre::Livestream,
        3 => Genre::Gaming,
        4 => Genre::Education,
        5 => Genre::Review,
        6 => Genre::Sales,
        7 => Genre::Interview,
        8 => Genre::Vlog,
        9 => Genre::Entertainment,
        10 => Genre::Music,
        11 => Genre::Visual,
        _ => Genre::General,
    };
    Config {
        target_count: ui.get_target_count().clamp(1, 12) as usize,
        min_duration: f64::from(ui.get_min_duration()),
        max_duration: f64::from(ui.get_max_duration()),
        genre,
        width,
        height,
        caption_preset: ui.get_caption_index().clamp(0, 5) as u8,
        transcript_path: STATE.with(|slot| slot.borrow().transcript.clone()),
        ..Default::default()
    }
}

fn publish_source(app: &App, source: &Source) {
    let ui = app.global::<AutoShorts>();
    ui.set_has_source(true);
    ui.set_source_duration(source.duration as f32);
    ui.set_source_label(source.label.clone().into());
    ui.set_source_info(
        format!(
            "{} × {} · {:.1}s · {:.1} MB · {}",
            source.width,
            source.height,
            source.duration,
            source.bytes as f64 / 1_000_000.0,
            if source.has_audio {
                "có âm thanh"
            } else {
                "không có âm thanh"
            }
        )
        .into(),
    );
}

fn visible_highlights<'a>(
    highlights: &'a [Highlight],
    selected: &BTreeSet<String>,
    query: &str,
    sort: i32,
    review_only: bool,
    selected_only: bool,
) -> Vec<&'a Highlight> {
    let query = query
        .trim()
        .chars()
        .take(256)
        .collect::<String>()
        .to_lowercase();
    let mut visible = highlights
        .iter()
        .filter(|h| {
            (!review_only || h.needs_review)
                && (!selected_only || selected.contains(&h.id))
                && (query.is_empty()
                    || [&h.title, &h.hook, &h.reason]
                        .iter()
                        .any(|text| text.to_lowercase().contains(&query))
                    || h.gate_notes
                        .iter()
                        .any(|text| text.to_lowercase().contains(&query)))
        })
        .collect::<Vec<_>>();
    visible.sort_by(|a, b| {
        let order = match sort {
            1 => a
                .parts
                .first()
                .map_or(f64::INFINITY, |part| part.start)
                .total_cmp(&b.parts.first().map_or(f64::INFINITY, |part| part.start)),
            2 => a.duration().total_cmp(&b.duration()),
            _ => b.score.total_cmp(&a.score),
        };
        order.then_with(|| a.id.cmp(&b.id))
    });
    visible
}

fn publish_rows(app: &App) {
    let ui = app.global::<AutoShorts>();
    let query = ui.get_query();
    let sort = ui.get_sort_index();
    let review_only = ui.get_review_only();
    let selected_only = ui.get_selected_only();
    let rows = STATE.with(|slot| {
        let state = slot.borrow();
        state
            .analysis
            .as_ref()
            .map(|analysis| {
                visible_highlights(
                    &analysis.highlights,
                    &state.selected,
                    &query,
                    sort,
                    review_only,
                    selected_only,
                )
                .into_iter()
                .map(|h| ShortsRow {
                    id: h.id.clone().into(),
                    title: h.title.clone().into(),
                    hook: h.hook.clone().into(),
                    reason: h.reason.clone().into(),
                    range: format!(
                        "{} · {:.1}s",
                        h.parts
                            .iter()
                            .map(|p| format!("{:.1}–{:.1}s", p.start, p.end))
                            .collect::<Vec<_>>()
                            .join(" + "),
                        h.duration()
                    )
                    .into(),
                    score: if h.provenance == "manual-transcript" {
                        "Chọn theo lời thoại · không chấm điểm AI".into()
                    } else {
                        format!("Điểm {:.0} · độ tin cậy {:.0}%", h.score, h.confidence * 100.0).into()
                    },
                    review: h.gate_notes.join(" · ").into(),
                    status: if h.needs_review {
                        "Cần xem lại trước khi xuất"
                    } else {
                        "Đã đối chiếu dữ liệu nguồn; cần bạn duyệt"
                    }
                    .into(),
                    selected: state.selected.contains(&h.id),
                    thumbnail: state
                        .thumbnails
                        .get(&h.id)
                        .map(host::image_of)
                        .unwrap_or_default(),
                })
                .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });
    let (selected, total) = STATE.with(|slot| {
        let state = slot.borrow();
        (
            state.selected.len() as i32,
            state
                .analysis
                .as_ref()
                .map_or(0, |analysis| analysis.highlights.len()) as i32,
        )
    });
    app.global::<AutoShorts>()
        .set_rows(ModelRc::new(VecModel::from(rows)));
    app.global::<AutoShorts>().set_selected_count(selected);
    app.global::<AutoShorts>().set_result_count(total);
    transcript::publish(app);
}

fn choose_source(app: &App) {
    let Some((id, cancel)) = begin(app, "Chọn video nguồn…") else {
        return;
    };
    host::spawn(
        move || -> Result<Option<Source>, String> {
            let path = rfd::FileDialog::new()
                .add_filter(
                    "Video",
                    &["mp4", "mkv", "mov", "webm", "m4v", "avi", "flv", "ts"],
                )
                .pick_file();
            if cancel.load(Ordering::Acquire) {
                return Ok(None);
            }
            path.map(|path| Engine::inspect(&path)).transpose()
        },
        move |studio, app, _, result| {
            if !finish(app, id) {
                return;
            }
            match result {
                Ok(Some(source)) => {
                    let path_str = source.path.to_string_lossy().to_string();
                    publish_source(app, &source);
                    let ui = app.global::<AutoShorts>();
                    ui.set_analyzed(false);
                    ui.set_diagnostics("".into());
                    ui.set_analysis_note("".into());
                    STATE.with(|slot| {
                        let mut state = slot.borrow_mut();
                        state.source = Some(source);
                        state.transcript = None;
                        state.analysis = None;
                        state.transcript_review = transcript::Workbench::default();
                        state.analysis_scope = None;
                        state.selected.clear();
                    });
                    app.global::<AutoShorts>().set_transcript_label("".into());
                    app.global::<AutoShorts>()
                        .set_status("Đã nạp video vào màn hình Xem trước Concat. Bấm 'Bắt đầu tìm đoạn hay' để AI phân tích.".into());
                    publish_rows(app);

                    // Connect to Concat's Preview Monitor
                    if studio.session.is_some() {
                        if let Ok(summary) = concat_host::media::probe(&path_str) {
                            let item = summary.to_new_media();
                            let item_path = item.path.clone();
                            let minted = studio.apply(Command::AddMedia { item }).or_else(|| {
                                studio
                                    .project()
                                    .media
                                    .iter()
                                    .find(|item| item.path == item_path)
                                    .map(|item| item.id.clone())
                            });
                            if let Some(media_id) = minted {
                                studio.preview_media(&media_id, -1);
                                studio.publish_source(app);
                                studio.request_preview();
                            }
                        }
                    }
                }
                Ok(None) => {}
                Err(error) => app.global::<AutoShorts>().set_status(error.into()),
            }
        },
    );
}

fn choose_folder(app: &App, watch: bool) {
    let Some((id, cancel)) = begin(app, "Chọn thư mục…") else {
        return;
    };
    host::spawn(
        move || {
            let chosen = rfd::FileDialog::new().pick_folder();
            if cancel.load(Ordering::Acquire) {
                None
            } else {
                chosen
            }
        },
        move |_, app, _, path| {
            if !finish(app, id) {
                return;
            }
            if let Some(path) = path {
                if watch {
                    app.global::<AutoShorts>()
                        .set_watch_label(path.to_string_lossy().as_ref().into());
                    STATE.with(|slot| {
                        let mut state = slot.borrow_mut();
                        state.watch = Some(path);
                        state.watch_pending.clear();
                        state.watch_sampler =
                            Arc::new(Mutex::new(StableFolderSampler::new(2, 256)));
                    });
                } else {
                    app.global::<AutoShorts>()
                        .set_output_label(path.to_string_lossy().as_ref().into());
                    STATE.with(|slot| slot.borrow_mut().output = Some(path));
                }
                app.global::<AutoShorts>()
                    .set_status("Đã chọn thư mục. Chưa xuất hoặc đăng video.".into());
            } else {
                app.global::<AutoShorts>()
                    .set_status("Đã đóng hộp chọn thư mục.".into());
            }
        },
    );
}

fn choose_transcript(app: &App) {
    if !app.global::<AutoShorts>().get_has_source() {
        return;
    }
    let Some((id, cancel)) = begin(app, "Chọn phụ đề đúng với video nguồn…") else {
        return;
    };
    host::spawn(
        move || {
            let path = rfd::FileDialog::new()
                .add_filter("Phụ đề UTF-8", &["srt", "vtt"])
                .pick_file();
            if cancel.load(Ordering::Acquire) {
                None
            } else {
                path
            }
        },
        move |_, app, _, path| {
            if !finish(app, id) {
                return;
            }
            if let Some(path) = path {
                app.global::<AutoShorts>().set_transcript_label(
                    format!(
                        "Phụ đề: {} · sẽ kiểm tra thời gian khi phân tích",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    )
                    .into(),
                );
                STATE.with(|slot| {
                    let mut state = slot.borrow_mut();
                    state.transcript = Some(path);
                    state.analysis = None;
                    state.transcript_review = transcript::Workbench::default();
                    state.selected.clear();
                });
                publish_rows(app);
                app.global::<AutoShorts>().set_analyzed(false);
                app.global::<AutoShorts>().set_status(
                    "Đã chọn phụ đề. Chạy lại Phân tích để kiểm tra và dùng dữ liệu mới.".into(),
                );
            }
        },
    );
}

fn thumbnail_size(width: u32, height: u32) -> (u32, u32) {
    let scale = 160.0 / f64::from(width.max(height).max(1));
    let even = |value: u32| ((f64::from(value) * scale).round() as u32).clamp(2, 160) & !1;
    (even(width), even(height))
}

fn candidate_thumbnails(
    analysis: &Analysis,
    cancel: &AtomicBool,
) -> Result<BTreeMap<String, Frame>, String> {
    if !analysis
        .fingerprint
        .unchanged()
        .map_err(|e| e.to_string())?
    {
        return Err(
            "Video nguồn đã thay đổi sau phân tích. Chọn lại video và phân tích lại.".into(),
        );
    }
    let (width, height) = thumbnail_size(analysis.source.width, analysis.source.height);
    let mut thumbnails = BTreeMap::new();
    for highlight in analysis.highlights.iter().take(12) {
        if cancel.load(Ordering::Acquire) {
            return Err("Đã hủy tải ảnh xem trước.".into());
        }
        let Some(part) = highlight.parts.first() else {
            continue;
        };
        let time = part.start + ((part.end - part.start) * 0.5).min(0.5);
        let Some(time) = concat_core::time::Rational::approximate(time) else {
            continue;
        };
        let options = DecodeOptions::default()
            .scaled_to(width, height)
            .starting_at(time);
        let Some(path) = analysis.source.path.to_str() else {
            continue;
        };
        // A real, source-time frame; bounded at decoder output, not an invented cover.
        if let Ok(mut decoder) = Decoder::open(path, &options) {
            if let Ok(Some(frame)) = decoder.next_frame() {
                if frame.width() <= 160 && frame.height() <= 160 {
                    thumbnails.insert(highlight.id.clone(), frame);
                }
            }
        }
    }
    if cancel.load(Ordering::Acquire) {
        return Err("Đã hủy tải ảnh xem trước.".into());
    }
    if !analysis
        .fingerprint
        .unchanged()
        .map_err(|e| e.to_string())?
    {
        return Err("Video nguồn thay đổi khi đọc ảnh xem trước. Cần phân tích lại.".into());
    }
    Ok(thumbnails)
}

fn analyze(app: &App, watched: Option<PathBuf>) {
    let is_watched = watched.is_some();
    if is_watched {
        STATE.with(|slot| slot.borrow_mut().transcript = None);
        app.global::<AutoShorts>().set_transcript_label("".into());
    }
    let path = watched
        .or_else(|| STATE.with(|slot| slot.borrow().source.as_ref().map(|s| s.path.clone())));
    let Some(path) = path else {
        app.global::<AutoShorts>()
            .set_status("Chọn video nguồn trước.".into());
        return;
    };
    let cfg = config(app);
    if cfg.min_duration < 4.0 || cfg.max_duration < cfg.min_duration || cfg.max_duration > 180.0 {
        app.global::<AutoShorts>()
            .set_status("Kiểm tra giới hạn thời lượng 4–180 giây.".into());
        return;
    }
    if !is_watched {
        let problem = STATE.with(|slot| slot.borrow().source.as_ref().and_then(|source| {
            crate::auto_shorts_feedback::source_problem(source.duration, cfg.min_duration)
        }));
        if let Some(problem) = problem {
            app.global::<AutoShorts>().set_status(problem.into());
            return;
        }
    }
    let ai_model = if app.global::<AutoShorts>().get_ai_mode_index() == 1 {
        let ai = app.global::<StudioAi>();
        let index = ai.get_model_index();
        if index < 0 || !ai.get_providers().iter().any(|p| p.active) {
            app.global::<AutoShorts>().set_status("Chọn model và bật kết nối AI trong AI Keys trước; không tự chuyển sang model khác.".into());
            return;
        }
        let Some(model) = ai
            .get_models()
            .row_data(index as usize)
            .filter(|s| !s.trim().is_empty())
        else {
            app.global::<AutoShorts>().set_status(
                "Model đã chọn không hợp lệ. Tải model trong AI Keys rồi chọn lại.".into(),
            );
            return;
        };
        Some(model.to_string())
    } else {
        None
    };
    Shell::with(move |shell, app| {
        let Some((id, cancel)) = begin(&app, "Đang chuẩn bị phân tích…") else {
            return;
        };
        let studio = shell.studio.borrow();
        let dirs = studio.host.dirs.clone();
        let transcriber = studio.host.transcriber.clone();
        let scope = studio
            .session
            .as_ref()
            .map(|s| (host::project_epoch(), s.revision()));
        let report = progress(id);
        host::spawn(
            move || {
                // Re-probe even watched/changed files before starting an AI client.
                let inspected = Engine::inspect(&path)?;
                if let Some(problem) = crate::auto_shorts_feedback::source_problem(inspected.duration, cfg.min_duration) {
                    return Err(problem);
                }
                let engine = Engine::new(dirs, transcriber)?;
                let cancel_engine = engine.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    STATE.with(|slot| {
                        let mut state = slot.borrow_mut();
                        if state.running == Some(id) && state.generation == id {
                            state.analysis_engine = Some(cancel_engine);
                        }
                    });
                });
                let ai = ai_model
                    .map(|model| {
                        crate::studio_local_ai::client()
                            .map(|client| (client, model))
                            .map_err(|_| {
                                "Không mở được OmniRoute local; kiểm tra AI Keys.".to_string()
                            })
                    })
                    .transpose()?;
                let mut analysis = engine.analyze(&path, cfg, ai, &cancel, report)?;
                let thumbnails = candidate_thumbnails(&analysis, &cancel)?;
                if thumbnails.len() < analysis.highlights.len() {
                    analysis.warnings.push("Một số ảnh xem trước không đọc được; vẫn cần xem đoạn video thật trước khi xuất.".into());
                }
                let review = concat_agent::transcript::Review::new(
                    analysis.segments.clone(), analysis.source.duration,
                ).map_err(str::to_string)?;
                Ok((analysis, thumbnails, review))
            },
            move |_, app, _, result: Result<(Analysis, BTreeMap<String, Frame>, concat_agent::transcript::Review), String>| {
                if !finish(app, id) {
                    return;
                }
                match result {
                    Ok((analysis, thumbnails, review)) => {
                        publish_source(app, &analysis.source);
                        let message = crate::auto_shorts_feedback::result_summary(analysis.highlights.len());
                        let ui = app.global::<AutoShorts>();
                        ui.set_analyzed(true);
                        ui.set_analysis_note(crate::auto_shorts_feedback::analysis_limits(&analysis.warnings).into());
                        ui.set_diagnostics(analysis.warnings.join("\n").into());
                        STATE.with(|slot| {
                            let mut state = slot.borrow_mut();
                            state.source = Some(analysis.source.clone());
                            state.analysis = Some(analysis);
                            state.transcript_review = transcript::Workbench::new(review);
                            state.thumbnails = thumbnails;
                            state.analysis_scope = scope;
                            state.selected.clear();
                            state.watch_paused = true;
                        });
                        transcript::reset_fields(app);
                        publish_rows(app);
                        app.global::<AutoShorts>().set_status(message.into());
                        app.global::<AutoShorts>().set_progress(1.0);
                    }
                    Err(error) => app.global::<AutoShorts>().set_status(error.into()),
                }
            },
        );
    });
}

fn reviewed_analysis(app: &App) -> Option<Analysis> {
    let analysis = STATE.with(|slot| slot.borrow().analysis.clone());
    if let Some(analysis) = analysis.as_ref() {
        let expected = serde_json::to_value(&analysis.config);
        let chosen = serde_json::to_value(config(app));
        if expected.is_err() || chosen.is_err() || expected.ok() != chosen.ok() {
            app.global::<AutoShorts>().set_status("Tùy chọn đã đổi từ lần phân tích. Phân tích lại để xem và xuất đúng cấu hình đang chọn.".into());
            return None;
        }
    }
    if analysis.is_none() {
        app.global::<AutoShorts>()
            .set_status("Chưa có kết quả phân tích. Chọn nguồn và chạy Phân tích trước.".into());
    }
    analysis
}

fn preview(app: &App, highlight_id: String) {
    let Some(analysis) = reviewed_analysis(app) else {
        return;
    };
    let Some(highlight) = analysis.highlights.iter().find(|h| h.id == highlight_id).cloned() else {
        return;
    };
    Shell::with(move |shell, app| {
        let mut studio = shell.studio.borrow_mut();
        let start_sec = highlight.parts.first().map_or(0.0, |p| p.start);
        let duration_sec = highlight.duration();
        let source_path = STATE.with(|slot| slot.borrow().source.as_ref().map(|s| s.path.clone()));
        if let Some(source_path) = source_path {
            let path_str = source_path.to_string_lossy().to_string();
            let media_id = studio.project().media.iter().find(|m| m.path == path_str).map(|m| m.id.clone()).or_else(|| {
                if let Ok(summary) = concat_host::media::probe(&path_str) {
                    studio.apply(Command::AddMedia { item: summary.to_new_media() })
                } else {
                    None
                }
            });
            if let Some(media_id) = media_id {
                let need_preview = studio.source.as_ref().map_or(true, |s| s.media_id != media_id);
                if need_preview {
                    studio.preview_media(&media_id, -1);
                }
            }
        }
        studio.source_seek(start_sec);
        if let Some(source) = studio.source.as_mut() {
            if !source.playing {
                studio.source_toggle();
            }
        }
        studio.publish_source(&app);
        studio.request_preview();
        app.global::<AutoShorts>().set_status(format!("Đang phát đoạn '{}' ({:.1}s) trên màn hình Xem trước Concat.", highlight.title, duration_sec).into());
    });
}

fn scope_matches(expected: Option<(u64, u64)>, epoch: u64, revision: Option<u64>) -> bool {
    expected.is_none_or(|(was_epoch, was_revision)| {
        was_epoch == epoch && revision == Some(was_revision)
    })
}

fn stage(app: &App, highlight_id: String) {
    let Some(analysis) = reviewed_analysis(app) else {
        return;
    };
    Shell::with(move |shell, app| {
        let studio = shell.studio.borrow();
        let Some(session) = studio.session.as_ref() else {
            app.global::<AutoShorts>().set_status(
                "Mở hoặc tạo dự án Concat trước, rồi quay lại để dựng đoạn đã chọn.".into(),
            );
            return;
        };
        let expected = STATE.with(|slot| slot.borrow().analysis_scope);
        if !scope_matches(expected, host::project_epoch(), Some(session.revision())) {
            app.global::<AutoShorts>().set_status("Dự án hoặc timeline đã thay đổi từ lần phân tích. Phân tích lại để dựng đúng phiên bản.".into());
            return;
        }
        let Some((id, cancel)) = begin(
            &app,
            "Đang chuẩn bị bản dựng native; chỉ áp dụng khi dự án còn đúng phiên bản…",
        ) else {
            return;
        };
        let snapshot = session.planning_snapshot();
        let revision = session.revision();
        let epoch = host::project_epoch();
        let dirs = studio.host.dirs.clone();
        let transcriber = studio.host.transcriber.clone();
        host::spawn(
            move || {
                let engine = Engine::new(dirs, transcriber)?;
                if cancel.load(Ordering::Acquire) {
                    return Err("Đã hủy dựng đoạn.".into());
                }
                engine.draft(&analysis, &highlight_id, &snapshot)
            },
            move |studio, app, models, result| {
                if !finish(app, id) {
                    return;
                }
                if !scope_matches(
                    Some((epoch, revision)),
                    host::project_epoch(),
                    studio.session.as_ref().map(|s| s.revision()),
                ) {
                    app.global::<AutoShorts>().set_status("Bỏ kết quả dựng vì dự án/timeline đã thay đổi. Không ghi đè chỉnh sửa hiện tại.".into());
                    return;
                }
                match result.and_then(|draft| studio.approve_ai_draft(draft)) {
                    Ok(()) => {
                        STATE.with(|slot| {
                            slot.borrow_mut().analysis_scope = studio
                                .session
                                .as_ref()
                                .map(|s| (host::project_epoch(), s.revision()))
                        });
                        studio.publish(app, models);
                        app.global::<AutoShorts>().set_status("Đã dựng đoạn đã duyệt trên timeline Concat. Ctrl+Z hoàn tác toàn bộ lần dựng này.".into());
                    }
                    Err(error) => app.global::<AutoShorts>().set_status(error.into()),
                }
            },
        );
    });
}

fn export_selected(app: &App) {
    let Some(analysis) = reviewed_analysis(app) else {
        return;
    };
    let (ids, folder) = STATE.with(|slot| {
        let state = slot.borrow();
        (
            state.selected.iter().cloned().collect::<Vec<_>>(),
            state.output.clone(),
        )
    });
    let Some(folder) = folder.filter(|_| !ids.is_empty()) else {
        app.global::<AutoShorts>()
            .set_status("Chọn ít nhất một đoạn và thư mục lưu trước.".into());
        return;
    };
    Shell::with(move |shell, app| {
        let Some((id, cancel)) = begin(
            &app,
            "Đang xuất các đoạn đã chọn; không đăng lên mạng xã hội…",
        ) else {
            return;
        };
        let studio = shell.studio.borrow();
        let dirs = studio.host.dirs.clone();
        let transcriber = studio.host.transcriber.clone();
        let report = progress(id);
        host::spawn(
            move || {
                Engine::new(dirs, transcriber)?.export(&analysis, &ids, &folder, &cancel, report)
            },
            move |_, app, _, result| {
                if !finish(app, id) {
                    return;
                }
                match result {
                Ok(outputs)=>app.global::<AutoShorts>().set_status(format!("Đã xuất {} video và biên nhận kiểm tra. Mở thư mục để xem kết quả; chưa đăng bài.",outputs.len()).into()),
                Err(error)=>app.global::<AutoShorts>().set_status(error.into()),
            }
            },
        );
    });
}

fn poll_watch(app: &App) {
    if !app.global::<AutoShorts>().get_watch_enabled() {
        return;
    }
    let item = STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        if state.running.is_some() || state.watch_paused {
            return None;
        }
        Some((
            state.watch.clone()?,
            state.watch_sampler.clone(),
            state.watch_pending.pop_front(),
        ))
    });
    let Some((folder, sampler, pending)) = item else {
        return;
    };
    if let Some(source) = pending {
        analyze(app, Some(source.path));
        return;
    }
    let mut cfg = config(app);
    cfg.transcript_path = None;
    let model = if app.global::<AutoShorts>().get_ai_mode_index() == 1 {
        let ai = app.global::<StudioAi>();
        let current = ai.get_model_index();
        let model = (current >= 0)
            .then(|| ai.get_models().row_data(current as usize))
            .flatten();
        let Some(model) = model.filter(|model| !model.trim().is_empty()) else {
            app.global::<AutoShorts>().set_watch_enabled(false);
            app.global::<AutoShorts>().set_status(
                "Đã dừng theo dõi: chọn model AI hợp lệ trước. Không tự dùng model khác.".into(),
            );
            return;
        };
        Some(model.to_string())
    } else {
        None
    };
    let Some((id, cancel)) = begin(app, "Đang kiểm tra tệp video đã ghi ổn định trong thư mục…")
    else {
        return;
    };
    Shell::with(move |shell, _| {
        let studio = shell.studio.borrow();
        let dirs = studio.host.dirs.clone();
        let transcriber = studio.host.transcriber.clone();
        host::spawn(
            move || -> Result<Vec<SourceFingerprint>, String> {
                if cancel.load(Ordering::Acquire) {
                    return Ok(vec![]);
                }
                let mut value =
                    serde_json::to_value(cfg).map_err(|_| "Tùy chọn theo dõi không hợp lệ.")?;
                if let Some(model) = model {
                    value["model"] = serde_json::json!(model);
                }
                let ready = sampler
                    .lock()
                    .map_err(|_| "Không truy cập được bộ theo dõi.")?
                    .scan(&folder, &value)
                    .map_err(|e| e.to_string())?;
                let engine = Engine::new(dirs, transcriber)?;
                let jobs = engine.queue().list().map_err(|e| e.to_string())?;
                Ok(ready
                    .into_iter()
                    .filter(|source| !jobs.iter().any(|job| job.source == *source))
                    .take(128)
                    .collect())
            },
            move |_, app, _, result| {
                if !finish(app, id) {
                    return;
                }
                match result {
                    Ok(ready) => {
                        STATE.with(|slot| slot.borrow_mut().watch_pending.extend(ready));
                        app.global::<AutoShorts>().set_status("Theo dõi đang bật. Tệp ổn định được phân tích lần lượt; kết quả dừng để bạn duyệt, không tự xuất.".into());
                    }
                    Err(error) => {
                        app.global::<AutoShorts>().set_watch_enabled(false);
                        app.global::<AutoShorts>()
                            .set_status(format!("Đã dừng theo dõi: {error}").into());
                    }
                }
            },
        );
    });
}

pub(crate) fn bind(app: &App) {
    transcript::bind(app);
    app.global::<AutoShorts>()
        .on_refresh_results(|| Shell::with(|_, app| publish_rows(&app)));
    app.global::<AutoShorts>().on_activate_preview(|| {
        Shell::with(|shell, app| {
            let mut studio = shell.studio.borrow_mut();
            if studio.source.is_none() {
                let source_path = STATE.with(|slot| slot.borrow().source.as_ref().map(|s| s.path.clone()));
                if let Some(source_path) = source_path {
                    let path_str = source_path.to_string_lossy().to_string();
                    let media_id = studio.project().media.iter().find(|m| m.path == path_str).map(|m| m.id.clone()).or_else(|| {
                        if let Ok(summary) = concat_host::media::probe(&path_str) {
                            studio.apply(Command::AddMedia { item: summary.to_new_media() })
                        } else {
                            None
                        }
                    });
                    if let Some(media_id) = media_id {
                        studio.preview_media(&media_id, -1);
                        studio.publish_source(&app);
                        studio.request_preview();
                    }
                }
            }
        });
    });
    app.global::<AutoShorts>().on_toggle_preview(|| {
        Shell::with(|shell, app| {
            let mut studio = shell.studio.borrow_mut();
            studio.source_toggle();
            studio.publish_source(&app);
        });
    });
    app.global::<AutoShorts>()
        .on_choose_source(|| Shell::with(|_, app| choose_source(&app)));
    app.global::<AutoShorts>()
        .on_choose_transcript(|| Shell::with(|_, app| choose_transcript(&app)));
    app.global::<AutoShorts>()
        .on_choose_output(|| Shell::with(|_, app| choose_folder(&app, false)));
    app.global::<AutoShorts>()
        .on_choose_watch(|| Shell::with(|_, app| choose_folder(&app, true)));
    app.global::<AutoShorts>()
        .on_analyze(|| Shell::with(|_, app| analyze(&app, None)));
    app.global::<AutoShorts>()
        .on_retry(|| Shell::with(|_, app| analyze(&app, None)));
    app.global::<AutoShorts>()
        .on_preview(|id| Shell::with(move |_, app| preview(&app, id.to_string())));
    app.global::<AutoShorts>()
        .on_stage(|id| Shell::with(move |_, app| stage(&app, id.to_string())));
    app.global::<AutoShorts>()
        .on_export_selected(|| Shell::with(|_, app| export_selected(&app)));
    app.global::<AutoShorts>().on_toggle_selected(|id| {
        Shell::with(move |_, app| {
            if app.global::<AutoShorts>().get_busy() {
                return;
            }
            STATE.with(|slot| {
                let mut state = slot.borrow_mut();
                let id = id.to_string();
                if !state
                    .analysis
                    .as_ref()
                    .is_some_and(|a| a.highlights.iter().any(|h| h.id == id))
                {
                    return;
                }
                if !state.selected.remove(&id) {
                    state.selected.insert(id);
                }
            });
            publish_rows(&app);
        })
    });
    app.global::<AutoShorts>().on_open_output(|| {
        Shell::with(|_, app| {
            let folder = STATE.with(|slot| slot.borrow().output.clone());
            let Some(folder) = folder else { return };
            let Some((id, cancel)) = begin(&app, "Đang mở thư mục kết quả…") else {
                return;
            };
            host::spawn(
                move || {
                    if cancel.load(Ordering::Acquire) {
                        return Err("Đã hủy mở thư mục.");
                    }
                    opener::open(folder)
                        .map_err(|_| "Không mở được thư mục bằng ứng dụng mặc định.")
                },
                move |_, app, _, result| {
                    if finish(app, id) {
                        app.global::<AutoShorts>().set_status(match result {
                            Ok(()) => "Đã mở thư mục.".into(),
                            Err(e) => e.into(),
                        });
                    }
                },
            );
        })
    });
    app.global::<AutoShorts>().on_cancel(|| {
        Shell::with(|_, app| {
            STATE.with(|slot| {
                let mut state = slot.borrow_mut();
                state.generation = state.generation.wrapping_add(1);
                if let Some(cancel) = &state.cancel {
                    cancel.store(true, Ordering::Release)
                }
                if let Some(engine) = &state.analysis_engine {
                    engine.cancel_asr();
                }
            });
            app.global::<AutoShorts>().set_status(
                "Đang hủy; đợi tác vụ hiện tại giải phóng tài nguyên. Kết quả muộn sẽ bị bỏ."
                    .into(),
            );
        })
    });
    app.global::<AutoShorts>().on_toggle_watch(|| {
        Shell::with(|_, app| {
            if app.global::<AutoShorts>().get_busy() {
                return;
            }
            let enabled = !app.global::<AutoShorts>().get_watch_enabled();
            if enabled && STATE.with(|slot| slot.borrow().watch.is_none()) {
                return;
            }
            STATE.with(|slot| slot.borrow_mut().watch_paused = false);
            app.global::<AutoShorts>().set_watch_enabled(enabled);
            app.global::<AutoShorts>().set_status(
                if enabled {
                    "Đã bật theo dõi. Chỉ phân tích; mỗi kết quả chờ bạn duyệt."
                } else {
                    "Đã tắt theo dõi; giữ nguyên kết quả và video."
                }
                .into(),
            );
        })
    });
    let timer = slint::Timer::default();
    let weak = app.as_weak();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_secs(3),
        move || {
            if let Some(app) = weak.upgrade() {
                poll_watch(&app)
            }
        },
    );
    STATE.with(|slot| slot.borrow_mut().timer = Some(timer));
}

pub(crate) fn shutdown() {
    STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        if let Some(cancel) = state.cancel.take() {
            cancel.store(true, Ordering::Release)
        }
        if let Some(engine) = state.analysis_engine.take() {
            engine.cancel_asr();
        }
        state.timer = None;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn candidate(
        id: &str,
        title: &str,
        start: f64,
        duration: f64,
        score: f64,
        needs_review: bool,
    ) -> Highlight {
        Highlight {
            id: id.into(),
            parts: vec![concat_agent::highlight::SourceRange {
                start,
                end: start + duration,
            }],
            title: title.into(),
            hook: String::new(),
            reason: String::new(),
            score,
            confidence: 0.5,
            needs_review,
            boundary: concat_agent::highlight::Boundary::Segment,
            evidence: vec![],
            gate_notes: vec![],
            provenance: "fixture".into(),
        }
    }
    #[test]
    fn result_filters_and_sorts_keep_underlying_selections() {
        let items = vec![
            candidate("a", "Podcast đầu tiên", 40.0, 20.0, 25.0, true),
            candidate("b", "Game", 5.0, 12.0, 80.0, false),
        ];
        let selected = BTreeSet::from(["b".to_string()]);
        let ids =
            |rows: Vec<&Highlight>| rows.into_iter().map(|h| h.id.clone()).collect::<Vec<_>>();
        assert_eq!(
            ids(visible_highlights(&items, &selected, "", 0, false, false)),
            vec!["b", "a"]
        );
        assert_eq!(
            ids(visible_highlights(&items, &selected, "", 1, false, false)),
            vec!["b", "a"]
        );
        assert_eq!(
            ids(visible_highlights(
                &items, &selected, "podcast", 2, false, false
            )),
            vec!["a"]
        );
        assert_eq!(
            ids(visible_highlights(&items, &selected, "", 0, true, false)),
            vec!["a"]
        );
        assert_eq!(
            ids(visible_highlights(&items, &selected, "", 0, false, true)),
            vec!["b"]
        );
        assert!(visible_highlights(&items, &selected, "không tồn tại", 0, false, false).is_empty());
        assert_eq!(selected, BTreeSet::from(["b".to_string()]));
    }
    #[test]
    fn thumbnails_are_bounded_in_both_axes_even_for_extreme_aspects() {
        for (w, h) in [
            (1920, 1080),
            (1080, 1920),
            (3840, 2160),
            (1, 100000),
            (100000, 1),
            (0, 0),
        ] {
            let (tw, th) = thumbnail_size(w, h);
            assert!((2..=160).contains(&tw));
            assert!((2..=160).contains(&th));
            assert_eq!(tw % 2, 0);
            assert_eq!(th % 2, 0);
        }
    }
    #[test]
    fn output_shapes_are_even_and_have_expected_ratios() {
        for quality in 0..3 {
            for aspect in 0..4 {
                let (w, h) = dimensions(aspect, quality);
                assert_eq!(w % 2, 0);
                assert_eq!(h % 2, 0);
                assert!(w >= 720 && h >= 720);
                let expected = match aspect {
                    1 => 16.0 / 9.0,
                    2 => 1.0,
                    3 => 4.0 / 5.0,
                    _ => 9.0 / 16.0,
                };
                assert!((f64::from(w) / f64::from(h) - expected).abs() < 0.002);
            }
        }
    }
    #[test]
    fn draft_cannot_cross_project_reopen_or_timeline_revision() {
        assert!(scope_matches(Some((2, 7)), 2, Some(7)));
        assert!(!scope_matches(Some((2, 7)), 3, Some(7)));
        assert!(!scope_matches(Some((2, 7)), 2, Some(8)));
        assert!(!scope_matches(Some((2, 7)), 2, None));
        assert!(scope_matches(None, 3, Some(0)));
    }
}
