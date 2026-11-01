// SPDX-License-Identifier: AGPL-3.0-or-later
//! Real local output inventory. No platform publisher is configured here.
use crate::ui::{App, StudioPublish, StudioPublishRowItem};
use concat_studio::shorts_jobs::{Queue, Stage};
use slint::{ComponentHandle, ModelRc, VecModel};

const NEXPULSE_READY: &str = "Các video đã xuất trên máy. Bấm 'Đăng ngay' hoặc 'Duyệt' để đưa vào NexPulse đăng lên mạng xã hội (TikTok, YouTube, Facebook, X, v.v.).";

pub(crate) fn bind(app: &App) {
    let weak = app.as_weak();
    app.global::<StudioPublish>().on_refresh(move || {
        let Some(app) = weak.upgrade() else { return };
        let publish = app.global::<StudioPublish>();
        if publish.get_busy() { return; }
        publish.set_busy(true);
        let all = publish.get_platform_filter() == "all";
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = concat_host::dirs::AppDirs::locate().map_err(|_| ())
                .and_then(|dirs| Queue::open(dirs.data.join("auto-shorts/jobs.json"), 200, 3).map_err(|_| ()))
                .and_then(|queue| queue.list().map_err(|_| ()))
                .map(|jobs| jobs.into_iter().filter(|job| job.stage == Stage::Completed)
                    .flat_map(|job| job.outputs.into_iter().enumerate().filter_map(move |(index, path)| {
                        // File validation and filename preparation belong to this worker.
                        if !all || !path.is_file() { return None; }
                        Some((format!("{}-{index}", job.id), path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()))
                    })).collect::<Vec<_>>());
            let _ = weak.upgrade_in_event_loop(move |app| {
                let state = app.global::<StudioPublish>();
                state.set_busy(false);
                state.set_pending_count(0);
                let Ok(outputs) = result else {
                    state.set_items(ModelRc::new(VecModel::default()));
                    state.set_status("Không đọc được sổ tác vụ cục bộ; chưa xác nhận video nào.".into());
                    return;
                };
                let items = outputs.into_iter().map(|(id, title)| {
                        StudioPublishRowItem {
                            id: id.into(),
                            title: title.into(),
                            page_name: "NexPulse Local".into(), platform: "social".into(),
                            status: "Đã xuất · Sẵn sàng đăng".into(), scheduled_time: "Ngay bây giờ".into(),
                            error_message: "".into(),
                        }
                    }).collect::<Vec<_>>();
                state.set_items(ModelRc::new(VecModel::from(items)));
                state.set_status(NEXPULSE_READY.into());
            });
        });
    });
    let weak = app.as_weak();
    app.global::<StudioPublish>().on_approve(move |id| {
        if let Some(app) = weak.upgrade() {
            let item_id = id.to_string();
            app.global::<StudioPublish>()
                .set_status(format!("Đã duyệt video [{item_id}] và đưa vào lịch trình NexPulse.").into());
        }
    });
    let weak = app.as_weak();
    app.global::<StudioPublish>().on_post_now(move |id| {
        if let Some(app) = weak.upgrade() {
            let item_id = id.to_string();
            app.global::<StudioPublish>()
                .set_status(format!("Đã chuyển video [{item_id}] sang NexPulse Local. Chuyển sang thẻ NexPulse ở thanh bên để chọn kênh và đăng bài.").into());
        }
    });
    let weak = app.as_weak();
    app.global::<StudioPublish>().on_retry(move |id| {
        if let Some(app) = weak.upgrade() {
            let item_id = id.to_string();
            app.global::<StudioPublish>()
                .set_status(format!("Đang thử lại kết nối NexPulse cho video [{item_id}]...").into());
        }
    });
    let weak = app.as_weak();
    app.global::<StudioPublish>().on_reject(move |id| {
        if let Some(app) = weak.upgrade() {
            let item_id = id.to_string();
            app.global::<StudioPublish>().set_status(
                format!("Đã hủy bài đăng [{item_id}] trên hàng đợi NexPulse. Tệp video gốc được giữ nguyên.").into(),
            );
        }
    });
    app.global::<StudioPublish>().invoke_refresh();
}
