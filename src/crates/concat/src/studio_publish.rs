// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native controls for Publish Queue workflow.

use crate::ui::{App, StudioPublish, StudioPublishRowItem};
use slint::{ComponentHandle, ModelRc, VecModel};

pub(crate) fn bind(app: &App) {
    let weak = app.as_weak();
    app.global::<StudioPublish>().on_refresh(move || {
        let Some(app) = weak.upgrade() else { return };
        let publish = app.global::<StudioPublish>();
        publish.set_busy(true);
        let weak_app = app.as_weak();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            let _ = weak_app.upgrade_in_event_loop(move |app| {
                let publish = app.global::<StudioPublish>();
                publish.set_busy(false);
                let demo_items = vec![
                    StudioPublishRowItem {
                        id: "pub_demo_1".into(),
                        title: "Cyberpunk 2099 Trailer".into(),
                        page_name: "Kênh Công Nghệ AI".into(),
                        platform: "tiktok".into(),
                        status: "pending".into(),
                        scheduled_time: "20:00 hôm nay".into(),
                        error_message: "".into(),
                    },
                    StudioPublishRowItem {
                        id: "pub_demo_2".into(),
                        title: "Khám Phá Vũ Trụ 4K".into(),
                        page_name: "Khoa Học Đời Sống".into(),
                        platform: "youtube".into(),
                        status: "scheduled".into(),
                        scheduled_time: "08:30 ngày mai".into(),
                        error_message: "".into(),
                    },
                ];
                let filter = publish.get_platform_filter().to_string();
                let filtered: Vec<_> = demo_items.into_iter().filter(|i| {
                    if filter == "all" { true } else { i.platform.to_string() == filter }
                }).collect();
                publish.set_pending_count(filtered.len() as i32);
                publish.set_items(ModelRc::new(VecModel::from(filtered)));
                publish.set_status("Đã cập nhật hàng đợi đăng bài.".into());
            });
        });
    });

    let weak = app.as_weak();
    app.global::<StudioPublish>().on_approve(move |id| {
        let Some(app) = weak.upgrade() else { return };
        let publish = app.global::<StudioPublish>();
        publish.set_status(format!("Đã duyệt xuất bản bài đăng {}", id).into());
    });

    let weak = app.as_weak();
    app.global::<StudioPublish>().on_post_now(move |id| {
        let Some(app) = weak.upgrade() else { return };
        let publish = app.global::<StudioPublish>();
        publish.set_status(format!("Đang thực hiện đăng bài ngay cho {}", id).into());
    });

    let weak = app.as_weak();
    app.global::<StudioPublish>().on_reject(move |id| {
        let Some(app) = weak.upgrade() else { return };
        let publish = app.global::<StudioPublish>();
        publish.set_status(format!("Đã từ chối bài đăng {}", id).into());
    });

    let weak = app.as_weak();
    app.global::<StudioPublish>().on_retry(move |id| {
        let Some(app) = weak.upgrade() else { return };
        let publish = app.global::<StudioPublish>();
        publish.set_status(format!("Đang thử lại đăng bài cho {}", id).into());
    });

    // Initial load
    app.global::<StudioPublish>().invoke_refresh();
}
