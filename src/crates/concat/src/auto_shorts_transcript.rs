// SPDX-License-Identifier: AGPL-3.0-or-later
//! Session-local transcript review. File dialogs/writes use workers; no timeline mutation.
use super::*;
use crate::ui::TranscriptRow;
use concat_agent::transcript::{Repetition, Review};

const PAGE: usize = 20;
#[derive(Default)]
pub(super) struct Workbench {
    review: Option<Review>,
    page: usize,
    first: Option<u32>,
    last: Option<u32>,
    editing: Option<u32>,
    serial: u64,
    repetitions: Vec<Repetition>,
    repetitions_scanned: bool,
}
impl Workbench {
    pub(super) fn new(review: Review) -> Self {
        Self {
            review: Some(review),
            ..Default::default()
        }
    }
}

pub(super) fn reset_fields(app: &App) {
    let ui = app.global::<AutoShorts>();
    ui.set_transcript_query("".into());
    ui.set_cue_editor_label("".into());
    ui.set_cue_editor_text("".into());
    ui.set_transcript_notice("".into());
    ui.set_repetitions_only(false);
    ui.set_repetitions_scanned(false);
    ui.set_repetition_count(0);
}

pub(super) fn publish(app: &App) {
    let ui = app.global::<AutoShorts>();
    let query = ui.get_transcript_query();
    STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        let w = &mut state.transcript_review;
        let Some(r) = w.review.as_ref() else {
            ui.set_cue_count(0);
            ui.set_transcript_rows(ModelRc::default());
            ui.set_transcript_undo(false);
            ui.set_transcript_redo(false);
            ui.set_transcript_has_range(false);
            ui.set_cue_editor_label("".into());
            return;
        };
        if !w.repetitions_scanned || w.repetitions.is_empty() {
            ui.set_repetitions_only(false);
        }
        ui.set_repetitions_scanned(w.repetitions_scanned);
        ui.set_repetition_count(w.repetitions.len() as i32);
        let mut labels: BTreeMap<u32, Vec<String>> = BTreeMap::new();
        let times = r
            .cues()
            .iter()
            .map(|c| (c.id, c.start))
            .collect::<BTreeMap<_, _>>();
        for hit in &w.repetitions {
            for (id, partner) in [(hit.earlier, hit.later), (hit.later, hit.earlier)] {
                if let Some(time) = times.get(&partner) {
                    labels.entry(id).or_default().push(format!(
                        "Có thể lặp câu tại {time:.2}s · độ giống {:.0}%",
                        hit.similarity * 100.0
                    ));
                }
            }
        }
        let hits = r
            .search(&query)
            .into_iter()
            .filter(|i| !ui.get_repetitions_only() || labels.contains_key(&r.cues()[*i].id))
            .collect::<Vec<_>>();
        w.page = w.page.min(hits.len().saturating_sub(1) / PAGE);
        let rows = hits
            .iter()
            .skip(w.page * PAGE)
            .take(PAGE)
            .map(|i| {
                let c = &r.cues()[*i];
                let marker = match (w.first == Some(c.id), w.last == Some(c.id)) {
                    (true, true) => "Đầu & cuối",
                    (true, false) => "Bắt đầu",
                    (false, true) => "Kết thúc",
                    _ => "",
                };
                TranscriptRow {
                    id: c.id.to_string().into(),
                    time: format!("{:.2}–{:.2}s", c.start, c.end).into(),
                    text: c.text.clone().into(),
                    marker: [
                        marker.to_string(),
                        labels.get(&c.id).map(|v| v.join(" · ")).unwrap_or_default(),
                    ]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ")
                    .into(),
                }
            })
            .collect::<Vec<_>>();
        ui.set_cue_count(r.cues().len() as i32);
        ui.set_transcript_rows(ModelRc::new(VecModel::from(rows)));
        ui.set_transcript_page(
            format!(
                "{} câu nguồn · {} câu khớp · trang {}/{}{}",
                r.cues().len(),
                hits.len(),
                w.page + 1,
                hits.len().div_ceil(PAGE).max(1),
                if query.is_empty() {
                    ""
                } else {
                    " · tối đa 2.000 lần khớp"
                }
            )
            .into(),
        );
        ui.set_transcript_previous(w.page > 0);
        ui.set_transcript_next((w.page + 1) * PAGE < hits.len());
        ui.set_transcript_undo(r.can_history(false));
        ui.set_transcript_redo(r.can_history(true));
        let range = w.first.zip(w.last).and_then(|(a, b)| {
            let i = r.cues().iter().position(|c| c.id == a)?;
            let j = r.cues().iter().position(|c| c.id == b)?;
            (i <= j).then(|| (r.cues()[i].start, r.cues()[j].end))
        });
        ui.set_transcript_has_range(range.is_some());
        ui.set_transcript_range(
            range
                .map(|(a, b)| format!("Vùng chọn: {a:.2}–{b:.2}s · {:.2} giây", b - a))
                .unwrap_or_else(|| "Chọn câu Bắt đầu, rồi chọn câu Kết thúc nằm sau nó.".into())
                .into(),
        );
    });
}

fn sync_corrections(state: &mut State) {
    state.transcript_review.repetitions.clear();
    state.transcript_review.repetitions_scanned = false;
    if let (Some(r), Some(a)) = (&state.transcript_review.review, &mut state.analysis) {
        a.segments = r.cues().to_vec();
        // Never retain implicit approval for evidence whose wording has changed.
        state.selected.clear();
        for h in &mut a.highlights {
            h.needs_review = true;
            let note = "Lời thoại đã được sửa: xem lại nội dung và phụ đề trước khi xuất.";
            if !h.gate_notes.iter().any(|n| n == note) {
                h.gate_notes.push(note.into());
            }
        }
    }
}

fn refresh_editor(app: &App) {
    STATE.with(|slot| {
        let s = slot.borrow();
        let w = &s.transcript_review;
        if let Some(c) = w
            .review
            .as_ref()
            .and_then(|r| r.cues().iter().find(|c| Some(c.id) == w.editing))
        {
            app.global::<AutoShorts>().set_cue_editor_label(
                format!("Sửa câu {:.2}–{:.2}s · tối đa 180 ký tự", c.start, c.end).into(),
            );
            app.global::<AutoShorts>()
                .set_cue_editor_text(c.text.clone().into());
        }
    });
}

fn create(app: &App) {
    if app.global::<AutoShorts>().get_busy() {
        return;
    }
    if reviewed_analysis(app).is_none() {
        let ui = app.global::<AutoShorts>();
        ui.set_transcript_notice(ui.get_status());
        return;
    }
    let result = STATE.with(|slot| -> Result<(), &'static str> {
        let mut s = slot.borrow_mut();
        let State {
            transcript_review: w,
            analysis,
            ..
        } = &mut *s;
        let a = analysis.as_mut().ok_or("Phân tích video trước.")?;
        if a.highlights.len() >= 48 {
            return Err("Đã có 48 đoạn. Tìm lại đoạn để bắt đầu phiên chọn mới.");
        }
        let first = w.first.ok_or("Chọn câu bắt đầu.")?;
        let last = w.last.ok_or("Chọn câu kết thúc.")?;
        let r = w.review.as_ref().ok_or("Chưa có lời thoại nguồn.")?;
        let next = w.serial.checked_add(1).ok_or("Đã vượt giới hạn phiên.")?;
        let h = r.candidate(first, last, &a.config.options(), next)?;
        if a.highlights.iter().any(|old| old.parts == h.parts) {
            return Err("Vùng này đã có trong danh sách clip.");
        }
        w.serial = next;
        a.highlights.push(h);
        Ok(())
    });
    let ui = app.global::<AutoShorts>();
    match result {
        Ok(()) => {
            ui.set_query("".into());
            ui.set_selected_only(false);
            ui.set_review_only(false);
            ui.set_sort_index(1);
            publish_rows(app);
            ui.set_status("Đã thêm vùng chọn vào danh sách clip; chưa xuất video. Bấm Xem đoạn để kiểm tra rồi chọn xuất.".into());
        }
        Err(e) => ui.set_status(e.into()),
    }
    ui.set_transcript_notice(ui.get_status());
}

fn export_srt(app: &App) {
    if app.global::<AutoShorts>().get_busy() {
        return;
    }
    let Some(a) = reviewed_analysis(app) else {
        return;
    };
    if a.segments.is_empty() {
        return;
    }
    let Some((id, cancel)) = begin(app, "Lưu bản phụ đề đã sửa…") else {
        return;
    };
    host::spawn(
        move || -> Result<bool, String> {
            use std::io::Write;
            let Some(path) = rfd::FileDialog::new()
                .add_filter("Phụ đề SRT UTF-8", &["srt"])
                .set_file_name("loi-thoai-da-sua.srt")
                .save_file()
            else {
                return Ok(false);
            };
            if cancel.load(Ordering::Acquire) {
                return Ok(false);
            }
            if !a.fingerprint.unchanged().map_err(|e| e.to_string())? {
                return Err("Video nguồn đã thay đổi. Phân tích lại trước khi lưu.".into());
            }
            let bytes = concat_agent::transcript::to_srt(&a.segments);
            if cancel.load(Ordering::Acquire) {
                return Ok(false);
            }
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|_| {
                    "Không tạo được SRT. Chọn tên tệp mới; không ghi đè phụ đề có sẵn.".to_string()
                })?;
            let result = f.write_all(bytes.as_bytes()).and_then(|_| f.sync_all());
            drop(f);
            if result.is_err() {
                let _ = std::fs::remove_file(&path);
                return Err("Không ghi được bản SRT.".into());
            }
            Ok(true)
        },
        move |_, app, _, result| {
            if !finish(app, id) {
                return;
            }
            app.global::<AutoShorts>().set_status(match result {
                Ok(true) => "Đã lưu bản SRT mới. Phụ đề gốc và video gốc không thay đổi.".into(),
                Ok(false) => "Đã hủy lưu SRT; giữ nguyên bản sửa trong phiên này.".into(),
                Err(e) => e.into(),
            });
            let ui = app.global::<AutoShorts>();
            ui.set_transcript_notice(ui.get_status());
        },
    );
}

pub(super) fn bind(app: &App) {
    app.global::<AutoShorts>().on_find_repetitions(|| Shell::with(|_, app| {
        if app.global::<AutoShorts>().get_busy() { return; }
        let snapshot = STATE.with(|slot| {
            let s = slot.borrow();
            s.analysis.as_ref().map(|a| (a.segments.clone(), a.source.duration))
        });
        let Some((cues, duration)) = snapshot else { return; };
        let Some((id, cancel)) = begin(&app, "Đang tìm các câu có thể nói lặp…") else { return; };
        host::spawn(move || {
            let hits = Review::new(cues, duration)?.repetitions();
            if cancel.load(Ordering::Acquire) { return Err("Đã hủy tìm câu lặp."); }
            Ok(hits)
        }, move |_, app, _, result: Result<Vec<Repetition>, &'static str>| {
            if !finish(app, id) { return; }
            match result {
                Ok(hits) => {
                    STATE.with(|slot| {
                        let mut s = slot.borrow_mut();
                        s.transcript_review.repetitions = hits;
                        s.transcript_review.repetitions_scanned = true;
                        s.transcript_review.page = 0;
                    });
                    let ui = app.global::<AutoShorts>();
                    ui.set_repetitions_only(true);
                    ui.set_transcript_query("".into());
                    ui.set_transcript_notice("Đã tìm câu tương tự để bạn đối chiếu. Không sửa tiếng nói, không xóa clip hay tự xuất.".into());
                    publish(app);
                }
                Err(e) => app.global::<AutoShorts>().set_transcript_notice(e.into()),
            }
        });
    }));
    app.global::<AutoShorts>().on_transcript_search(|| {
        Shell::with(|_, app| {
            if app.global::<AutoShorts>().get_busy() {
                return;
            }
            STATE.with(|s| s.borrow_mut().transcript_review.page = 0);
            publish(&app);
        })
    });
    app.global::<AutoShorts>().on_transcript_paginate(|delta| {
        Shell::with(move |_, app| {
            if app.global::<AutoShorts>().get_busy() {
                return;
            }
            STATE.with(|s| {
                let mut s = s.borrow_mut();
                let w = &mut s.transcript_review;
                w.page = if delta > 0 {
                    w.page.saturating_add(1)
                } else {
                    w.page.saturating_sub(1)
                };
            });
            publish(&app);
        })
    });
    app.global::<AutoShorts>()
        .on_transcript_select(|id, action| {
            Shell::with(move |_, app| {
                if app.global::<AutoShorts>().get_busy() {
                    return;
                }
                let Ok(id) = id.parse::<u32>() else {
                    return;
                };
                STATE.with(|slot| {
                    let mut s = slot.borrow_mut();
                    let w = &mut s.transcript_review;
                    if !w
                        .review
                        .as_ref()
                        .is_some_and(|r| r.cues().iter().any(|c| c.id == id))
                    {
                        return;
                    }
                    match action {
                        0 => {
                            w.first = Some(id);
                            w.last = Some(id);
                        }
                        1 => {
                            if w.first.is_none() {
                                w.first = Some(id);
                            }
                            w.last = Some(id);
                        }
                        2 => w.editing = Some(id),
                        _ => return,
                    }
                });
                if action == 2 {
                    refresh_editor(&app);
                }
                publish(&app);
            })
        });
    app.global::<AutoShorts>().on_transcript_save(|| Shell::with(|_, app| {
        if app.global::<AutoShorts>().get_busy() { return; }
        let text = app.global::<AutoShorts>().get_cue_editor_text();
        let result = STATE.with(|slot| {
            let mut s = slot.borrow_mut(); let w = &mut s.transcript_review;
            let result = match (w.review.as_mut(), w.editing) { (Some(r), Some(id)) => r.edit(id, &text), _ => Err("Chọn câu cần sửa.") };
            if matches!(result, Ok(true)) { sync_corrections(&mut s); }
            result
        });
        app.global::<AutoShorts>().set_status(match result {
            Ok(true) => "Đã sửa phụ đề trong phiên này; bỏ các lựa chọn xuất cũ để bạn duyệt lại. Lưu SRT để giữ bản sửa.".into(),
            Ok(false) => "Câu không thay đổi.".into(), Err(e) => e.into(),
        });
        let ui = app.global::<AutoShorts>();
        ui.set_transcript_notice(ui.get_status());
        publish_rows(&app);
    }));
    app.global::<AutoShorts>().on_transcript_history(|redo| {
        Shell::with(move |_, app| {
            if app.global::<AutoShorts>().get_busy() {
                return;
            }
            let changed = STATE.with(|slot| {
                let mut s = slot.borrow_mut();
                let changed = s
                    .transcript_review
                    .review
                    .as_mut()
                    .is_some_and(|r| r.history(redo));
                if changed {
                    sync_corrections(&mut s);
                }
                changed
            });
            if changed {
                refresh_editor(&app);
                publish_rows(&app);
                app.global::<AutoShorts>().set_status(
                    "Đã cập nhật lời thoại. Xem lại và chọn các clip muốn xuất.".into(),
                );
                let ui = app.global::<AutoShorts>();
                ui.set_transcript_notice(ui.get_status());
            }
        })
    });
    app.global::<AutoShorts>()
        .on_transcript_create(|| Shell::with(|_, app| create(&app)));
    app.global::<AutoShorts>()
        .on_transcript_export(|| Shell::with(|_, app| export_srt(&app)));
}
