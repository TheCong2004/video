//! Control adapters over the existing Nexora client and Grok production runner.
use crate::ui::{AiControl, App};
use concat_studio::{
    nexora::NexoraSettings,
    pages::{ContentPage, ContentPageStore},
    production::{ProductionClient, ProductionPlan},
    production_pages::{ProductionPage, catalogue},
};
use serde::Deserialize;
use serde_json::{Value, json};
use slint::ComponentHandle;
use std::{
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

fn connection() -> Result<(concat_host::AppDirs, concat_studio::browser::BrowserClient), String> {
    let dirs = concat_host::AppDirs::locate()?;
    let browser = NexoraSettings::load(&dirs.config)
        .and_then(|s| s.client())
        .map_err(|_| "Kết nối Nexora chưa hợp lệ. Dùng mục kết nối trong Xưởng phim AI.")?;
    browser
        .health()
        .map_err(|_| "Chưa kết nối Nexora. Bấm Mở Nexora hoặc dùng kết nối trong Xưởng phim AI.")?;
    Ok((dirs, browser))
}
fn pages(
    dirs: &concat_host::AppDirs,
    browser: &concat_studio::browser::BrowserClient,
) -> Result<Vec<ProductionPage>, String> {
    let profiles = browser
        .profiles()
        .map_err(|_| "Không đọc được hồ sơ Nexora")?;
    catalogue(profiles, ContentPageStore::new(&dirs.config).list(false)?)
}
pub(crate) fn discover() -> Result<Value, String> {
    let (dirs, browser) = connection()?;
    let pages = pages(&dirs, &browser)?;
    Ok(
        json!({"pages":pages.iter().take(500).map(|p|json!({"profile_id":p.profile.id,"name":p.profile.name,"running":p.profile.running,
        "image_prompt":p.preset.as_ref().and_then(|p|p.default_image_prompt.as_deref()),
        "video_prompt":p.preset.as_ref().and_then(|p|p.default_video_prompt.as_deref())})).collect::<Vec<_>>(),
        "publisher_ready":false,"summary":"Hồ sơ Nexora thật; chưa phải xác nhận quyền đăng Facebook Page"}),
    )
}
pub(crate) fn generate(
    input: &Value,
    root: &Path,
    cancel: Arc<AtomicBool>,
    weak: &slint::Weak<App>,
) -> Result<Value, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        profile_id: String,
        anchor: String,
        image_prompt: String,
        expand_prompt: String,
        video_prompt: String,
    }
    let input: Input =
        serde_json::from_value(input.clone()).map_err(|_| "Đầu vào Grok không hợp lệ")?;
    let (dirs, browser) = connection()?;
    let page = pages(&dirs, &browser)?
        .into_iter()
        .find(|p| p.profile.id == input.profile_id)
        .ok_or("Hồ sơ Nexora đã chọn không còn tồn tại")?;
    // A job snapshot only. Never creates a duplicate Page or changes its preset.
    let mut snapshot = page.preset.unwrap_or_else(|| ContentPage {
        id: page.profile.id.clone(),
        name: page.profile.name.clone(),
        slug: page.profile.id.clone(),
        output_root: String::new(),
        target_platform: None,
        default_model_id: None,
        default_workflow_id: None,
        default_language: None,
        default_tone: None,
        default_aspect_ratio: Some("9:16".into()),
        browser_profile_id: Some(page.profile.id.clone()),
        worker_pool_id: None,
        default_image_prompt: None,
        default_expand_9_16_prompt: None,
        default_video_prompt: None,
        is_archived: false,
        created_at: 0,
        updated_at: 0,
        revision: 0,
    });
    snapshot.output_root = root
        .canonicalize()
        .map_err(|_| "Không mở được thư mục tác vụ")?
        .to_string_lossy()
        .into_owned();
    let plan = ProductionPlan::new(
        snapshot,
        input.anchor.into(),
        &input.image_prompt,
        &input.expand_prompt,
        &input.video_prompt,
    )?;
    let client = ProductionClient::new(browser.clone())
        .with_recovery_root(dirs.config.join("studio/production-recovery"));
    client.preflight(&plan)?;
    browser.ensure_production_grok(&page.profile).map_err(
        |_| "Không mở được Grok trong hồ sơ Nexora đã chọn; kiểm tra cửa sổ đó, không tự gửi lại",
    )?;
    let _runtime = concat_studio::production_runtime::BrowserRuntime::ensure()?;
    let progress = weak.clone();
    let result = client.run(plan, cancel, move |status| {
        let message = format!("Grok · {:?} · {}", status.stage, status.state);
        let _ = progress
            .upgrade_in_event_loop(move |app| app.global::<AiControl>().set_status(message.into()));
    })?;
    Ok(json!({"path":result.path,"sha256":result.sha256,"profile_id":input.profile_id}))
}
