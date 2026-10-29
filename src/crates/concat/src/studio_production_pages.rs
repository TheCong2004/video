// SPDX-License-Identifier: AGPL-3.0-or-later
//! Catalogue comes from Nexora. Local channel presets never create extra Pages.
use crate::ui::{App, ProductionStudio};
use concat_studio::{
    nexora::NexoraSettings,
    pages::ContentPageStore,
    production_pages::{ProductionPage, catalogue},
};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::sync::{Arc, Mutex};

static SELECTED: Mutex<Option<ProductionPage>> = Mutex::new(None);
pub(crate) fn disconnect(app: &App) {
    if let Ok(mut selected) = SELECTED.lock() {
        *selected = None;
    }
    let s = app.global::<ProductionStudio>();
    s.set_pages_loaded(false);
    s.set_busy(false);
    s.invoke_invalidate();
    s.set_page_index(0);
    s.set_page_names(ModelRc::new(VecModel::from(vec![
        "Chọn Page/hồ sơ Nexora…".into(),
    ])));
    s.set_workflow_index(-1);
    s.set_workflow_ids(Default::default());
    s.set_workflow_names(Default::default());
    s.set_page_summary("Kết nối Nexora đã đổi. Tải lại Page trước khi chạy.".into());
}
pub(crate) fn selected() -> Option<ProductionPage> {
    SELECTED.lock().ok()?.clone()
}

pub(crate) fn bind(app: &App) {
    let pages = Arc::new(Mutex::new(Vec::<ProductionPage>::new()));
    let weak = app.as_weak();
    let rows = pages.clone();
    app.global::<ProductionStudio>().on_load_pages(move || {
        let Some(app) = weak.upgrade() else { return };
        let s = app.global::<ProductionStudio>();
        if s.get_busy() { return }
        let previous = selected().map(|page| page.profile.id);
        s.set_busy(true);
        s.invoke_invalidate();
        crate::studio_session::connection_changed();
        let generation = crate::studio_session::session_generation();
        *SELECTED.lock().unwrap() = None;
        rows.lock().unwrap().clear();
        s.set_page_index(0);
        s.set_workflow_index(-1);
        s.set_workflow_ids(Default::default());
        s.set_workflow_names(Default::default());
        s.set_page_names(ModelRc::new(VecModel::from(vec!["Chọn Page/hồ sơ Nexora…".into()])));
        s.set_page_summary("Đang lấy danh sách từ Nexora…".into());
        let weak = app.as_weak();
        let rows = rows.clone();
        let task = std::thread::Builder::new().name("production-nexora-pages".into()).spawn(move || {
            let result = (|| -> Result<_, String> {
                let dirs = concat_host::AppDirs::locate()?;
                let client = NexoraSettings::load(&dirs.config).and_then(|s| s.client())
                    .map_err(|_| "Cấu hình kết nối Nexora chưa hợp lệ.")?;
                client.health().map_err(|_| "Chưa kết nối Nexora. Mở Nexora rồi tải lại danh sách.")?;
                let profiles = client.profiles().map_err(|_| "Không lấy được danh sách Page/hồ sơ từ Nexora.")?;
                let presets = ContentPageStore::new(&dirs.config).list(false)?;
                catalogue(profiles, presets)
            })();
            let _ = weak.upgrade_in_event_loop(move |app| {
                if generation != crate::studio_session::session_generation() {
                    return;
                }
                let s = app.global::<ProductionStudio>();
                s.set_busy(false);
                match result {
                    Ok(pages) => {
                        let restore = previous.as_ref().and_then(|id| pages.iter().position(|p| &p.profile.id == id));
                        let mut names = vec!["Chọn Page/hồ sơ Nexora…".into()];
                        names.extend(pages.iter().map(|p| p.profile.name.clone().into()));
                        s.set_page_names(ModelRc::new(VecModel::from(names)));
                        let count = pages.len();
                        *rows.lock().unwrap() = pages;
                        s.set_pages_loaded(true);
                        s.set_page_summary(if count == 0 { "Nexora chưa có Page/hồ sơ. Tạo trong Nexora rồi bấm tải lại.".into() }
                            else { format!("Đã lấy {count} Page/hồ sơ từ Nexora. Chọn mục cần dùng; không cần tạo lại trong Concat.").into() });
                        if let Some(index) = restore {
                            s.set_page_index(index as i32 + 1);
                            s.invoke_apply_page(index as i32 + 1);
                        }
                    }
                    Err(message) => s.set_page_summary(message.into()),
                }
            });
        });
        if task.is_err() { s.set_busy(false); s.set_page_summary("Không mở được tác vụ kết nối Nexora.".into()); }
    });
    let weak = app.as_weak();
    app.global::<ProductionStudio>()
        .on_apply_page(move |index| {
            let Some(app) = weak.upgrade() else { return };
            let s = app.global::<ProductionStudio>();
            if s.get_busy() {
                return;
            }
            s.invoke_invalidate();
            crate::studio_session::connection_changed();
            let page = usize::try_from(index.saturating_sub(1))
                .ok()
                .and_then(|i| pages.lock().ok()?.get(i).cloned());
            *SELECTED.lock().unwrap() = page.clone();
            s.set_workflow_index(-1);
            let Some(page) = page else {
                s.set_page_summary("Chưa chọn Page/hồ sơ Nexora.".into());
                return;
            };
            if let Some(preset) = &page.preset {
                s.set_image_prompt(
                    preset
                        .default_image_prompt
                        .clone()
                        .unwrap_or_default()
                        .into(),
                );
                s.set_expand_prompt(
                    preset
                        .default_expand_9_16_prompt
                        .clone()
                        .unwrap_or_default()
                        .into(),
                );
                s.set_motion_prompt(
                    preset
                        .default_video_prompt
                        .clone()
                        .unwrap_or_default()
                        .into(),
                );
                s.set_aspect_index(match preset.default_aspect_ratio.as_deref() {
                    Some("16:9") => 1,
                    Some("1:1") => 2,
                    _ => 0,
                });
            }
            if let Some(id) = page.workflow_id() {
                let found = s.get_workflow_ids().iter().position(|v| v.as_str() == id);
                s.set_workflow_index(found.map_or(-1, |i| i as i32));
            }
            s.set_page_summary(
                format!(
                    "{} · Nexora ID: {} · {}\n{}",
                    page.profile.name,
                    page.profile.id,
                    if page.profile.running {
                        "đang mở"
                    } else {
                        "chưa mở"
                    },
                    if page.preset.is_some() {
                        "Đã áp dụng preset đang gắn với hồ sơ này."
                    } else {
                        "Dùng thiết lập sản xuất hiện tại; không tạo Page trùng trong Concat."
                    }
                )
                .into(),
            );
            s.invoke_load_workflows();
        });
}
