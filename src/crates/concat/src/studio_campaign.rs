// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native campaign view over the durable service and independent worker process.
use crate::ui::App;
use concat_control_ui::{CampaignRow, CampaignStageRow, CampaignUi, CampaignVideoRow, ControlWindow};
use concat_studio::campaign::{
    engine::{Engine, EngineSettings},
    facebook::Facebook,
    *,
};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

struct Form {
    request: String,
    topic: String,
    facts: String,
    count: String,
    seconds: String,
    language: String,
    style: String,
    model: String,
    start: String,
    end: String,
    deadline: String,
    timezone: String,
    slots: String,
    daily: String,
    workers: String,
    budget: String,
    render_budget: String,
    text_cost: String,
    media_cost: String,
    render_seconds: String,
    pages: String,
    assets: String,
    profile: String,
    grok: bool,
    publish: bool,
    unique: bool,
    mode: i32,
    selected: String,
    revision: String,
    hash: String,
    video: String,
    video_revision: String,
    narration: String,
    title: String,
    caption: String,
    token: String,
    version: String,
}
fn snapshot(panel: &ControlWindow, model: &str, secret: bool) -> Form {
    let ui = panel.global::<CampaignUi>();
    Form {
        request: ui.get_request().into(),
        topic: ui.get_topic().into(),
        facts: ui.get_facts().into(),
        count: ui.get_count().into(),
        seconds: ui.get_seconds().into(),
        language: ui.get_language().into(),
        style: ui.get_style().into(),
        model: if ui.get_text_model().is_empty() {
            model.into()
        } else {
            ui.get_text_model().into()
        },
        start: ui.get_start_date().into(),
        end: ui.get_end_date().into(),
        deadline: ui.get_deadline().into(),
        timezone: ui.get_timezone().into(),
        slots: ui.get_slots().into(),
        daily: ui.get_daily().into(),
        workers: ui.get_workers().into(),
        budget: ui.get_api_budget().into(),
        render_budget: ui.get_render_budget().into(),
        text_cost: ui.get_text_unit_cost().into(),
        media_cost: ui.get_media_unit_cost().into(),
        render_seconds: ui.get_render_unit_seconds().into(),
        pages: ui.get_page_ids().into(),
        assets: ui.get_assets().into(),
        profile: ui
            .get_profile_ids()
            .row_data(ui.get_profile_index().max(0) as usize)
            .filter(|_| ui.get_profile_index() >= 0)
            .map(|s| s.to_string())
            .unwrap_or_default(),
        grok: ui.get_grok(),
        publish: ui.get_publish(),
        unique: ui.get_unique_content(),
        mode: ui.get_mode(),
        selected: ui.get_selected_id().into(),
        revision: ui.get_expected_revision().into(),
        hash: ui.get_plan_hash().into(),
        video: ui.get_selected_video().into(),
        video_revision: ui.get_video_revision().into(),
        narration: ui.get_narration().into(),
        title: ui.get_video_title().into(),
        caption: ui.get_caption().into(),
        token: if secret {
            ui.get_user_token().into()
        } else {
            String::new()
        },
        version: ui.get_graph_version().into(),
    }
}
fn settings() -> Result<(CampaignStore, EngineSettings), String> {
    let dirs = concat_host::AppDirs::locate()?;
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .map_err(|_| "Không tìm thấy nguồn runtime")?;
    let (node, script) = crate::studio_ai_control::web_runtime()?;
    let settings = EngineSettings {
        config_root: dirs.config,
        data_root: dirs.data.clone(),
        ffmpeg: crate::studio_scenes::ffmpeg()?,
        python: concat_studio::factory_edit::python(&repo),
        font: crate::studio_scenes::font()?,
        node,
        web_script: script,
        repo,
    };
    Ok((
        CampaignStore::open(&dirs.data.join("ai-campaigns"))?,
        settings,
    ))
}
fn lines(value: &str) -> Vec<String> {
    value
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(Into::into)
        .collect()
}
fn number<T: std::str::FromStr>(value: &str, label: &str) -> Result<T, String> {
    value
        .trim()
        .parse()
        .map_err(|_| format!("{label} không hợp lệ"))
}
fn money(value: &str) -> Result<u64, String> {
    let n: f64 = number(value, "Số tiền USD")?;
    if !n.is_finite() || n < 0.0 || n > 1_000_000_000.0 {
        return Err("Ngân sách USD không hợp lệ".into());
    }
    Ok((n * 1_000_000.0).round() as u64)
}
fn form_plan(form: &Form, config: &EngineSettings) -> Result<Plan, String> {
    let completion_deadline_utc = if form.deadline.trim().is_empty() {
        None
    } else {
        use chrono::TimeZone;
        let zone: chrono_tz::Tz = form.timezone.parse().map_err(|_| "Múi giờ không hợp lệ")?;
        let local = chrono::NaiveDateTime::parse_from_str(form.deadline.trim(), "%Y-%m-%d %H:%M")
            .map_err(|_| "Hạn hoàn tất phải là YYYY-MM-DD HH:MM")?;
        Some(
            zone.from_local_datetime(&local)
                .single()
                .ok_or("Giờ hoàn tất không tồn tại hoặc lặp do DST; chọn giờ khác")?
                .timestamp(),
        )
    };
    let connected = Facebook::new(&config.config_root).connected()?;
    let daily = number(&form.daily, "Hạn mức Page")?;
    let slots: Vec<String> = form
        .slots
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(Into::into)
        .collect();
    let pages=lines(&form.pages).into_iter().map(|row|{let (id,topics)=row.split_once('|').unwrap_or((&row,""));let id=id.trim().to_string();let page=connected.iter().find(|p|p.page_id==id).ok_or_else(||format!("Page {id} chưa được cấp quyền Facebook API; hồ sơ Nexora không thay thế quyền này"))?;Ok(PageRule{page_id:id,name:page.name.clone(),topics:topics.split(',').map(str::trim).filter(|s|!s.is_empty()).map(Into::into).collect(),daily_limit:daily,slots:slots.clone()})}).collect::<Result<Vec<_>,String>>()?;
    let assets: Vec<PathBuf> = lines(&form.assets).into_iter().map(PathBuf::from).collect();
    let plan = Plan {
        objective: form.request.clone(),
        video_count: number(&form.count, "Số video")?,
        video_seconds: number(&form.seconds, "Thời lượng")?,
        topic: form.topic.clone(),
        language: form.language.clone(),
        style: form.style.clone(),
        unique_content: form.unique,
        facts: lines(&form.facts),
        pages,
        publish: form.publish,
        timezone: form.timezone.clone(),
        start_date: form.start.clone(),
        end_date: form.end.clone(),
        completion_deadline_utc,
        allow_cross_page: false,
        api_budget_micros: money(&form.budget)?,
        render_budget_seconds: number(&form.render_budget, "Ngân sách render")?,
        max_workers: number(&form.workers, "Số worker")?,
        mode: match form.mode {
            0 => Automation::Manual,
            2 => Automation::FullAuto,
            _ => Automation::SemiAuto,
        },
        approve_content: form.mode == 1,
        approve_publish: form.mode != 2,
        max_attempts: 3,
        retry_delay_seconds: 30,
        providers: Providers {
            text_model: form.model.clone(),
            grok_profile: form.profile.clone(),
            anchor: if form.grok {
                assets.first().cloned()
            } else {
                None
            },
            assets: if form.grok { vec![] } else { assets },
            allowed_fallbacks: vec![],
            text_cost_micros: money(&form.text_cost)?,
            media_cost_micros: money(&form.media_cost)?,
            render_reserve_seconds: number(&form.render_seconds, "Giây render dự trù")?,
        },
        locked_fields: Plan::policy_fields()
            .iter()
            .map(|s| s.to_string())
            .collect(),
    };
    plan.validate()?;
    Ok(plan)
}
fn state(state: &CampaignState) -> &'static str {
    match state {
        CampaignState::Draft => "draft",
        CampaignState::Approved => "approved",
        CampaignState::Running => "running",
        CampaignState::Paused => "paused",
        CampaignState::Cancelled => "cancelled",
        CampaignState::Completed => "completed",
    }
}
fn stage(stage: &Stage) -> &'static str {
    match stage {
        Stage::Content => "Nội dung",
        Stage::Media => "Tư liệu",
        Stage::Render => "Render",
        Stage::Quality => "Kiểm tra",
        Stage::Publish => "Xuất bản",
        Stage::Done => "Hoàn thành",
    }
}
fn job_state(state: &JobState) -> &'static str {
    match state {
        JobState::Pending => "Đang chờ",
        JobState::Running => "Đang chạy",
        JobState::NeedsApproval => "Chờ duyệt",
        JobState::NeedsReview => "Cần xử lý",
        JobState::RetryWait => "Chờ thử lại",
        JobState::ReadyToPublish => "Đạt QC · chờ đăng",
        JobState::Published => "Đã đăng có xác minh",
        JobState::Completed => "Đã xuất có QC",
        JobState::Failed => "Thất bại",
        JobState::Unknown => "Kết quả chưa rõ",
        JobState::Cancelled => "Đã hủy",
    }
}
struct View {
    records: Vec<Record>,
    selected: Option<Record>,
    profiles: Vec<(String, String)>,
    pages: String,
}
fn view(
    store: &CampaignStore,
    settings: &EngineSettings,
    id: &str,
    discover: bool,
) -> Result<View, String> {
    let records = store.list()?;
    let selected = if id.is_empty() {
        None
    } else {
        Some(store.read(id)?)
    };
    let profiles = if discover {
        concat_studio::nexora::NexoraSettings::load(&settings.config_root)
            .and_then(|s| s.client())
            .and_then(|c| c.profiles())
            .unwrap_or_default()
            .into_iter()
            .map(|p| (p.id, p.name))
            .collect()
    } else {
        vec![]
    };
    let pages = Facebook::new(&settings.config_root)
        .connected()?
        .iter()
        .map(|p| format!("{} · {}", p.page_id, p.name))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(View {
        records,
        selected,
        profiles,
        pages,
    })
}
fn paint(panel: &ControlWindow, view: View, restore: bool) {
    let ui = panel.global::<CampaignUi>();
    ui.set_campaigns(ModelRc::new(VecModel::from(
        view.records
            .iter()
            .map(|r| CampaignRow {
                id: r.id.clone().into(),
                title: r
                    .plan()
                    .objective
                    .chars()
                    .take(60)
                    .collect::<String>()
                    .into(),
                summary: format!(
                    "{} · {} video · v{}",
                    state(&r.state),
                    r.plan().video_count,
                    r.plan_revision()
                )
                .into(),
            })
            .collect::<Vec<_>>(),
    )));
    ui.set_connected_pages(view.pages.into());
    if !view.profiles.is_empty() {
        let selected = ui
            .get_profile_ids()
            .row_data(ui.get_profile_index().max(0) as usize)
            .map(|s| s.to_string());
        ui.set_profile_ids(ModelRc::new(VecModel::from(
            view.profiles
                .iter()
                .map(|p| p.0.clone().into())
                .collect::<Vec<slint::SharedString>>(),
        )));
        ui.set_profile_names(ModelRc::new(VecModel::from(
            view.profiles
                .iter()
                .map(|p| p.1.clone().into())
                .collect::<Vec<slint::SharedString>>(),
        )));
        ui.set_profile_index(
            selected
                .and_then(|id| view.profiles.iter().position(|p| p.0 == id))
                .map(|i| i as i32)
                .unwrap_or(if view.profiles.len() == 1 { 0 } else { -1 }),
        );
    }
    let Some(record) = view.selected else {
        return;
    };
    let plan = record.plan();
    let progress = concat_studio::campaign::progress::stages(&record, concat_studio::campaign::store::now());
    let running = progress.iter().filter(|step| step.state == "running").count();
    let blocked = progress.iter().any(|step| step.state == "blocked");
    let summary = if running > 0 {
        format!("Đang xử lý tại {running} công đoạn · khối sáng là bước đang chạy")
    } else if blocked {
        "Đang bị chặn · xem lý do trong khối màu cam bên dưới".into()
    } else if record.state == CampaignState::Paused {
        "Đã tạm dừng · hiện không có công đoạn đang xử lý".into()
    } else if record.state == CampaignState::Completed {
        "Chiến dịch đã hoàn tất".into()
    } else if record.state == CampaignState::Cancelled {
        "Chiến dịch đã hủy".into()
    } else {
        "Đang chờ · chưa có công đoạn được worker xác nhận đang xử lý".into()
    };
    ui.set_execution_summary(summary.into());
    ui.set_stages(ModelRc::new(VecModel::from(progress.into_iter().map(|step| CampaignStageRow {
        title:step.title.into(), state:step.state.into(), label:step.label.into(), detail:step.detail.into(), error:step.error.into(),
    }).collect::<Vec<_>>())));
    ui.set_selected_id(record.id.clone().into());
    ui.set_campaign_state(state(&record.state).into());
    ui.set_review(
        format!(
            "v{} · {} video × {} giây/video\n{} → {} · {}\n{} · {} worker tối đa",
            record.plan_revision(),
            plan.video_count,
            plan.video_seconds,
            plan.start_date,
            plan.end_date,
            plan.timezone,
            state(&record.state),
            plan.max_workers
        )
        .into(),
    );
    let counts = record.counts();
    let count = |key: &str| counts.get(key).copied().unwrap_or(0);
    ui.set_counts(format!("Mục tiêu {} · Kịch bản {} · Tư liệu {}\nRender {} · Đạt QC {} · Có lịch {}\nĐã đăng {} · Lỗi {} · Cần xử lý {}",plan.video_count,count("scripted"),count("media"),count("rendered"),count("quality_checked"),count("scheduled"),count("published"),count("failed"),count("needs_action")).into());
    if restore {
        ui.set_request(plan.objective.clone().into());
        ui.set_topic(plan.topic.clone().into());
        ui.set_facts(plan.facts.join("\n").into());
        ui.set_count(plan.video_count.to_string().into());
        ui.set_seconds(plan.video_seconds.to_string().into());
        ui.set_language(plan.language.clone().into());
        ui.set_style(plan.style.clone().into());
        ui.set_text_model(plan.providers.text_model.clone().into());
        if let Some(provider) = match plan.providers.text_model.as_str() {
            "web:chatgpt" => Some(0), "web:gemini" => Some(1), "web:claude" => Some(2), _ => None,
        } {
            panel.global::<concat_control_ui::AiControl>().set_web_provider(provider);
        }
        ui.set_start_date(plan.start_date.clone().into());
        ui.set_end_date(plan.end_date.clone().into());
        ui.set_timezone(plan.timezone.clone().into());
        {
            use chrono::TimeZone;
            let deadline = plan
                .completion_deadline_utc
                .and_then(|stamp| {
                    plan.timezone
                        .parse::<chrono_tz::Tz>()
                        .ok()?
                        .timestamp_opt(stamp, 0)
                        .single()
                })
                .map(|time| time.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_default();
            ui.set_deadline(deadline.into());
        }
        ui.set_page_ids(
            plan.pages
                .iter()
                .map(|p| {
                    if p.topics.is_empty() {
                        p.page_id.clone()
                    } else {
                        format!("{} | {}", p.page_id, p.topics.join(", "))
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
                .into(),
        );
        ui.set_slots(
            plan.pages
                .first()
                .map(|p| p.slots.join(","))
                .unwrap_or_else(|| "08:00,13:00,19:00".into())
                .into(),
        );
        ui.set_daily(
            plan.pages
                .first()
                .map(|p| p.daily_limit)
                .unwrap_or(3)
                .to_string()
                .into(),
        );
        ui.set_workers(plan.max_workers.to_string().into());
        ui.set_api_budget(format!("{}", plan.api_budget_micros as f64 / 1e6).into());
        ui.set_render_budget(plan.render_budget_seconds.to_string().into());
        ui.set_text_unit_cost(format!("{}", plan.providers.text_cost_micros as f64 / 1e6).into());
        ui.set_media_unit_cost(format!("{}", plan.providers.media_cost_micros as f64 / 1e6).into());
        ui.set_render_unit_seconds(plan.providers.render_reserve_seconds.to_string().into());
        ui.set_assets(
            plan.providers
                .anchor
                .iter()
                .chain(plan.providers.assets.iter())
                .map(|p| p.to_string_lossy())
                .collect::<Vec<_>>()
                .join("\n")
                .into(),
        );
        ui.set_grok(plan.providers.uses_grok());
        ui.set_unique_content(plan.unique_content);
        ui.set_publish(plan.publish);
        ui.set_mode(match plan.mode {
            Automation::Manual => 0,
            Automation::SemiAuto => 1,
            Automation::FullAuto => 2,
        });
        ui.set_plan_hash(plan.hash().into());
        ui.set_expected_revision(record.revision.to_string().into());
        ui.set_dirty(false);
        let index = ui
            .get_profile_ids()
            .iter()
            .position(|id| id.as_str() == plan.providers.grok_profile);
        ui.set_profile_index(index.map(|i| i as i32).unwrap_or(-1));
    }
    let filter = ui.get_filter();
    let jobs: Vec<_> = record
        .jobs
        .iter()
        .filter(|j| match filter.as_str() {
            "needs_action" => matches!(
                j.state,
                JobState::Unknown
                    | JobState::NeedsReview
                    | JobState::NeedsApproval
                    | JobState::Failed
            ),
            "rendered" => j.rendered.is_some(),
            "published" => j.state == JobState::Published,
            "running" => j.state == JobState::Running,
            _ => true,
        })
        .collect();
    let start = ui.get_page().max(0) as usize * 25;
    ui.set_paging(
        format!(
            "{}–{} / {} video",
            (start + 1).min(jobs.len()),
            (start + 25).min(jobs.len()),
            jobs.len()
        )
        .into(),
    );
    ui.set_videos(ModelRc::new(VecModel::from(
        jobs.iter()
            .skip(start)
            .take(25)
            .map(|j| CampaignVideoRow {
                id: j.id.clone().into(),
                title: format!(
                    "{} · {}",
                    j.ordinal,
                    j.script
                        .as_ref()
                        .map(|s| s.title.as_str())
                        .unwrap_or("Chưa có nội dung")
                )
                .into(),
                summary: format!(
                    "{} · {}{}",
                    stage(&j.stage),
                    job_state(&j.state),
                    if j.hold {
                        " · Đang giữ thủ công"
                    } else {
                        ""
                    }
                )
                .into(),
                error: j.error.clone().unwrap_or_default().into(),
                post_url: j.post_url.clone().unwrap_or_default().into(),
            })
            .collect::<Vec<_>>(),
    )));
    let zone = plan.timezone.parse::<chrono_tz::Tz>().ok();
    use chrono::TimeZone;
    ui.set_calendar(
        record
            .slots
            .iter()
            .take(100)
            .map(|s| {
                format!(
                    "{} · {} · {}",
                    s.page_id,
                    zone.and_then(|z| z.timestamp_opt(s.utc, 0).single())
                        .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                        .unwrap_or_else(|| s.utc.to_string()),
                    s.video_id.as_deref().unwrap_or("trống")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
            .into(),
    );
    let spent = record
        .usage
        .iter()
        .map(|u| u.micros as f64 / 1e6)
        .sum::<f64>();
    let rendered = record.usage.iter().map(|u| u.render_seconds).sum::<u64>();
    let logs = record
        .audit
        .iter()
        .rev()
        .take(40)
        .map(|a| format!("{} · {} · {} · {}", a.at, a.actor, a.action, a.detail))
        .collect::<Vec<_>>()
        .join("\n");
    ui.set_log(format!("Đã hạch toán dự trù: ${spent:.6} / ${:.6}\nRender đã ghi nhận: {rendered}s / {}s\n{logs}",plan.api_budget_micros as f64/1e6,plan.render_budget_seconds).into());
}
fn worker_exe() -> Result<PathBuf, String> {
    let path = std::env::current_exe().map_err(|e| e.to_string())?;
    let folder = path.parent().ok_or("Không có thư mục app")?;
    [
        folder.join("concat-campaign-worker.exe"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/debug/concat-campaign-worker.exe"),
    ]
    .into_iter()
    .find(|p| p.is_file())
    .ok_or_else(|| {
        "Bộ cài thiếu concat-campaign-worker.exe; cần cập nhật bản build có worker nền".into()
    })
}
fn start_worker(store: &CampaignStore, settings: &EngineSettings, id: &str) -> Result<(), String> {
    Engine::new(settings.clone()).preflight(store.read(id)?.plan())?;
    let path = store.directory(id)?.join("engine-settings.json");
    std::fs::write(
        &path,
        serde_json::to_vec(settings).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let exe = worker_exe()?;
    let dir = store.directory(id)?;
    let mut command = Command::new(exe);
    command
        .arg("--run")
        .arg(store.root())
        .arg(id)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("worker-output.log"))
                .map_err(|e| e.to_string())?,
        )
        .stderr(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("worker-error.log"))
                .map_err(|e| e.to_string())?,
        );
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    // Deliberately not an app-owned kill-on-close Job: this is the approved
    // background worker. OS file leases and the coordinator lock own execution.
    command.spawn().map_err(|_| "Không mở được worker nền")?;
    Ok(())
}
fn action(
    store: &CampaignStore,
    settings: &EngineSettings,
    form: &Form,
    action: &str,
) -> Result<(String, bool), String> {
    match action {
        "save" | "propose" => {
            let template = form_plan(form, settings)?;
            let plan = if action == "propose" {
                Engine::new(settings.clone())
                    .propose(&form.request, &template, &AtomicBool::new(false))
                    .map_err(|e| e.message)?
            } else {
                template
            };
            let record = if form.selected.is_empty() {
                store.create(plan)?
            } else {
                store.revise(
                    &form.selected,
                    plan,
                    number(&form.revision, "Phiên bản")?,
                    action == "save",
                )?
            };
            Ok((record.id, true))
        }
        "approve" => {
            let record = store.read(&form.selected)?;
            let p = record.plan();
            if number::<u32>(&form.count, "Số video")? != p.video_count
                || number::<f64>(&form.seconds, "Thời lượng")? != p.video_seconds
                || form.topic != p.topic
                || form.start != p.start_date
                || form.end != p.end_date
                || form.timezone != p.timezone
                || form.publish != p.publish
                || number::<u8>(&form.workers, "Worker")? != p.max_workers
                || money(&form.budget)? != p.api_budget_micros
                || number::<u64>(&form.render_budget, "Render budget")? != p.render_budget_seconds
            {
                return Err(
                    "Bạn đã sửa biểu mẫu. Lưu phiên bản mới rồi xem lại trước khi duyệt.".into(),
                );
            }
            store.approve(
                &form.selected,
                number(&form.revision, "Phiên bản")?,
                &form.hash,
            )?;
            Ok((form.selected.clone(), true))
        }
        "start" => {
            let record = store.read(&form.selected)?;
            if form.hash != record.plan().hash() {
                return Err("Tải lại và kiểm tra đúng phiên bản chiến dịch trước khi chạy".into());
            }
            Engine::new(settings.clone()).preflight(record.plan())?;
            store.control(&form.selected, "start")?;
            if let Err(error) = start_worker(store, settings, &form.selected) {
                let _ = store.control(&form.selected, "pause");
                return Err(error);
            }
            Ok((form.selected.clone(), true))
        }
        "reattach-worker" => {
            let record = store.read(&form.selected)?;
            if record.state != CampaignState::Running
                || record.approved_hash.as_deref() != Some(record.plan().hash().as_str())
            {
                return Err("Chỉ nối lại worker cho đúng kế hoạch đã duyệt đang chạy".into());
            }
            start_worker(store, settings, &form.selected)?;
            Ok((form.selected.clone(), false))
        }
        "pause" | "cancel" => {
            store.control(&form.selected, action)?;
            Ok((form.selected.clone(), true))
        }
        "schedule" => {
            store.allocate(&form.selected)?;
            Ok((form.selected.clone(), false))
        }
        "hold-video" | "continue-video" => {
            store.hold(&form.selected, &form.video, action == "hold-video")?;
            Ok((form.selected.clone(), false))
        }
        "approve-video" => {
            store.approve_video(
                &form.selected,
                &form.video,
                number(&form.video_revision, "Phiên bản video")?,
            )?;
            Ok((form.selected.clone(), false))
        }
        "edit-video" => {
            let record = store.read(&form.selected)?;
            let mut script = record
                .jobs
                .iter()
                .find(|j| j.id == form.video)
                .and_then(|j| j.script.clone())
                .ok_or("Chưa có kịch bản để sửa")?;
            script.narration = form.narration.clone();
            script.title = form.title.clone();
            script.caption = form.caption.clone();
            store.manual_edit(
                &form.selected,
                &form.video,
                number(&form.video_revision, "Phiên bản video")?,
                Some(script),
                None,
            )?;
            Ok((form.selected.clone(), false))
        }
        "retry-video" => {
            store.transact(&form.selected,|r|{let max=r.plan().max_attempts;let job=r.jobs.iter_mut().find(|j|j.id==form.video).ok_or("Video không tồn tại")?;if job.lease.is_some()||!matches!(job.state,JobState::Failed|JobState::NeedsReview|JobState::RetryWait){return Err("Không được gửi lại kết quả Unknown; dùng Đối chiếu bài đăng hoặc kiểm tra biên nhận".into());}job.state=JobState::Pending;job.attempt=0;job.retry_at=0;job.error=None;if job.stage==Stage::Quality{job.stage=Stage::Render;job.rendered=None;job.quality=None;}r.audit("operator","explicit_known_retry",Some(&form.video),format!("Tối đa {max} lần theo kế hoạch"),concat_studio::campaign::store::now());Ok(())})?;
            Ok((form.selected.clone(), false))
        }
        "reconcile" => {
            let record = store.read(&form.selected)?;
            let job = record
                .jobs
                .iter()
                .find(|j| j.id == form.video)
                .ok_or("Video không tồn tại")?;
            let page = job.page_id.as_deref().ok_or("Chưa có Page")?;
            let id = job
                .upload_id
                .as_deref()
                .ok_or("Chưa có video_id; không tự tạo phiên đăng mới")?;
            let api = Facebook::new(&settings.config_root);
            let response = api.status(page, id).map_err(|e| e.message)?;
            let url = Facebook::published(&response, id)
                .ok_or("Facebook chưa xác nhận xuất bản công khai; không gửi lại")?;
            store.reconcile_published(
                &form.selected,
                &form.video,
                job.revision,
                id,
                &url,
                concat_studio::campaign::store::now(),
            )?;
            Ok((form.selected.clone(), false))
        }
        "connect-pages" => {
            Facebook::new(&settings.config_root).connect(
                &form.version,
                &form.token,
                &lines(&form.pages)
                    .iter()
                    .map(|row| row.split('|').next().unwrap_or("").trim().to_string())
                    .collect::<Vec<_>>(),
            )?;
            Ok((form.selected.clone(), false))
        }
        "play-video" => {
            let r = store.read(&form.selected)?;
            let path = &r
                .jobs
                .iter()
                .find(|j| j.id == form.video)
                .and_then(|j| j.rendered.as_ref())
                .ok_or("Chưa có file video")?
                .path;
            opener::open(path).map_err(|e| e.to_string())?;
            Ok((form.selected.clone(), false))
        }
        _ => Ok((form.selected.clone(), false)),
    }
}
pub(crate) fn bind(app: &App, panel: &ControlWindow) {
    if std::env::var("CONCAT_CAMPAIGN_ENABLED").as_deref() == Ok("0") {
        panel.set_campaign_enabled(false);
        return;
    }
    use chrono::TimeZone;
    let zone = chrono_tz::Asia::Bangkok;
    let today = zone
        .timestamp_opt(concat_studio::campaign::store::now(), 0)
        .single()
        .unwrap()
        .date_naive();
    panel
        .global::<CampaignUi>()
        .set_start_date(today.to_string().into());
    panel
        .global::<CampaignUi>()
        .set_end_date((today + chrono::Duration::days(30)).to_string().into());
    panel
        .global::<CampaignUi>()
        .set_text_model(crate::studio_ai_control::selected_ai_model(app).into());
    let working = Arc::new(AtomicBool::new(false));
    let weak = panel.as_weak();
    let main = app.as_weak();
    let flag = working.clone();
    panel.global::<CampaignUi>().on_action(move|action_name|{
        let Some(panel)=weak.upgrade()else{return;};let action_name=action_name.to_string();
        if action_name=="enable-motion" {
            if let Some(app)=main.upgrade(){
                if !app.global::<crate::ui::Theme>().get_system_reduced_motion(){app.global::<crate::ui::LauncherUi>().invoke_reduced_motion_changed(false);}
            }
            return;
        }
        if action_name!="poll" {panel.global::<CampaignUi>().set_action_error("".into());}
        if matches!(action_name.as_str(),"approve"|"start") && panel.global::<CampaignUi>().get_dirty(){panel.global::<CampaignUi>().set_action_error("Có thay đổi chưa lưu. Lưu phiên bản mới rồi kiểm tra lại trước khi duyệt hoặc chạy.".into());return;}
        if action_name=="new"{
            let ui=panel.global::<CampaignUi>();
            // A fresh campaign used to be visually indistinguishable from the
            // already-empty initial form. Reset it to a safe one-video draft
            // and acknowledge the click in the status strip.
            ui.set_selected_id("".into());
            ui.set_stages(ModelRc::new(VecModel::from(Vec::<CampaignStageRow>::new())));
            ui.set_execution_summary("".into());
            ui.set_campaign_state("draft".into());
            ui.set_editing_video(false);
            ui.set_expected_revision("".into());
            ui.set_plan_hash("".into());
            ui.set_review("".into());
            ui.set_counts("".into());
            ui.set_request("".into());
            ui.set_topic("".into());
            ui.set_facts("".into());
            ui.set_count("1".into());
            ui.set_seconds("10".into());
            ui.set_language("vi-VN".into());
            ui.set_style("Tự nhiên, rõ ràng".into());
            ui.set_deadline("".into());
            ui.set_page_ids("".into());
            ui.set_assets("".into());
            ui.set_grok(false);
            ui.set_publish(false);
            ui.set_unique_content(true);
            ui.set_mode(1);
            ui.set_profile_index(-1);
            ui.set_selected_video("".into());
            ui.set_video_revision("".into());
            ui.set_narration("".into());
            ui.set_video_title("".into());
            ui.set_caption("".into());
            ui.set_dirty(true);
            ui.set_status("Đã mở bản nháp chiến dịch mới · nhập yêu cầu rồi chọn Lưu bản nháp hoặc AI lập bản nháp. Chưa có video nào được tạo.".into());
            return;
        }
        let priority=matches!(action_name.as_str(),"pause"|"cancel"|"hold-video");
        if action_name=="poll"&&flag.load(Ordering::Acquire){return;}
        if action_name!="poll"&&!priority&&flag.swap(true,Ordering::AcqRel){return;}
        let model=main.upgrade().map(|app|crate::studio_ai_control::selected_ai_model(&app)).unwrap_or_default();let form=snapshot(&panel,&model,action_name=="connect-pages");
        let page=panel.global::<CampaignUi>().get_page();let filter=panel.global::<CampaignUi>().get_filter();
        if action_name=="connect-pages"{panel.global::<CampaignUi>().set_user_token("".into());}
        let progress=match action_name.as_str(){
            "propose"=>"Đang gửi yêu cầu cho AI để lập bản nháp…",
            "start"=>"Đang khởi động worker chiến dịch…",
            "reattach-worker"=>"Đang kết nối lại worker…",
            "approve"=>"Đang duyệt phiên bản kế hoạch…",
            "save"=>"Đang lưu bản nháp…",
            "poll"=>"Đang kiểm tra tiến độ chiến dịch…",
            _=>"Đang thực hiện thao tác…",
        };
        if action_name!="poll" {panel.global::<CampaignUi>().set_status(progress.into());panel.global::<CampaignUi>().set_busy(true);}
        let reply=panel.as_weak();let flag=flag.clone();let main=main.clone();let expected_id=form.selected.clone();
        std::thread::spawn(move||{
            let result=(||->Result<Option<(View,bool)>,String>{let(store,settings)=settings()?;
                if action_name=="assets"||action_name=="replace-media"{
                    let picker=rfd::FileDialog::new();let paths=if form.grok&&action_name=="assets"{picker.add_filter("Ảnh nguồn",&["png","jpg","jpeg"]).pick_file().map(|p|vec![p])}else{picker.add_filter("Media",&["mp4","mov","mkv","png","jpg","jpeg"]).pick_files()};
                    if let Some(paths)=paths{if action_name=="replace-media"{if paths.len()!=1{return Err("Mỗi lần thay một tư liệu".into());}let p=&paths[0];let asset=Asset{path:p.clone(),sha256:concat_studio::campaign::quality::hash(p)?,rights:"operator-supplied".into(),provider:"manual".into()};store.manual_edit(&form.selected,&form.video,number(&form.video_revision,"Phiên bản video")?,None,Some(asset))?;}else{let text=paths.iter().map(|p|p.to_string_lossy()).collect::<Vec<_>>().join("\n");let ui=reply.clone();let _=ui.upgrade_in_event_loop(move|panel|{let ui=panel.global::<CampaignUi>();ui.set_assets(text.into());ui.set_dirty(true);});}}
                    return Ok(Some((view(&store,&settings,&form.selected,true)?,false)));
                }
                if action_name=="open-editor"{
                    store.hold(&form.selected,&form.video,true)?;let r=store.read(&form.selected)?;let path=r.jobs.iter().find(|j|j.id==form.video).and_then(|j|j.rendered.as_ref()).ok_or("Chưa có file video")?.path.clone();
                    let _=main.upgrade_in_event_loop(move|app|{crate::studio_control_window::reveal_main(&app);crate::host::Shell::with(|shell,app|{let mut studio=shell.studio.borrow_mut();if studio.session.is_none(){studio.notify("Mở hoặc tạo dự án Concat để nhập video; video chiến dịch đang được giữ để sửa.",false);return;}studio.handle(crate::panes::Msg::Media(crate::panes::media_bin::MediaMsg::Import(vec![path])));studio.publish(&app,&shell.models);});});
                    return Ok(Some((view(&store,&settings,&form.selected,false)?,false)));
                }
                let(id,restore)=action(&store,&settings,&form,&action_name)?;Ok(Some((view(&store,&settings,&id,action_name!="poll")?,restore)))
            })();
            if !priority&&action_name!="poll"{flag.store(false,Ordering::Release);}
            let _=reply.upgrade_in_event_loop(move|panel|{let ui=panel.global::<CampaignUi>();if action_name=="poll"&&(ui.get_busy()||ui.get_selected_id().as_str()!=expected_id.as_str()||ui.get_filter()!=filter||ui.get_page()!=page){return;}if action_name!="poll"{ui.set_busy(false);}ui.set_page(page);ui.set_filter(filter);match result{Ok(Some((view,restore)))=>{
                let status:String=match action_name.as_str(){
                    "propose"=>"AI đã lập bản nháp. Kiểm tra nội dung rồi duyệt kế hoạch.".into(),
                    "start"=>"Đã khởi động worker. Tiến độ và lỗi sẽ hiện tự động ở đây.".into(),
                    "reattach-worker"=>"Đã gửi yêu cầu kết nối worker; tiến độ được làm mới tự động.".into(),
                    "approve"=>"Đã duyệt kế hoạch. Kiểm tra lại cấu hình trước khi chạy.".into(),
                    "save"=>"Đã lưu bản nháp chiến dịch.".into(),
                    _=>"Đã cập nhật dữ liệu chiến dịch.".into(),
                };
                paint(&panel,view,restore);
                ui.set_status(if action_name=="poll" {ui.get_execution_summary()} else {status.into()});
            },Ok(None)=>{},Err(error)=>{ui.set_status(error.clone().into());if action_name!="poll"{ui.set_action_error(error.into());}}}});
        });
    });
    let weak = panel.as_weak();
    panel.global::<CampaignUi>().on_select_campaign(move |id| {
        let Some(panel) = weak.upgrade() else {
            return;
        };
        panel.global::<CampaignUi>().set_selected_id(id);
        panel.global::<CampaignUi>().set_action_error("".into());
        panel.global::<CampaignUi>().set_editing_video(false);
        let reply = panel.as_weak();
        let id = panel.global::<CampaignUi>().get_selected_id().to_string();
        std::thread::spawn(move || {
            let result = settings().and_then(|(store, cfg)| view(&store, &cfg, &id, true));
            let _ = reply.upgrade_in_event_loop(move |panel| match result {
                Ok(view) => paint(&panel, view, true),
                Err(error) => panel.global::<CampaignUi>().set_status(error.into()),
            });
        });
    });
    let weak = panel.as_weak();
    panel.global::<CampaignUi>().on_select_video(move |id| {
        let Some(panel) = weak.upgrade() else {
            return;
        };
        let campaign = panel.global::<CampaignUi>().get_selected_id().to_string();
        let video = id.to_string();
        let reply = panel.as_weak();
        std::thread::spawn(move || {
            let result = settings().and_then(|(store, _)| {
                let r = store.read(&campaign)?;
                r.jobs
                    .into_iter()
                    .find(|j| j.id == video)
                    .ok_or("Video không tồn tại".into())
            });
            let _ = reply.upgrade_in_event_loop(move |panel| {
                let ui = panel.global::<CampaignUi>();
                match result {
                    Ok(job) => {
                        ui.set_selected_video(job.id.into());
                        ui.set_video_revision(job.revision.to_string().into());
                        ui.set_editing_video(true);
                        if let Some(script) = job.script {
                            ui.set_narration(script.narration.into());
                            ui.set_video_title(script.title.into());
                            ui.set_caption(script.caption.into());
                        }
                    }
                    Err(error) => ui.set_status(error.into()),
                }
            });
        });
    });
    // Reuse the existing panel timer; no second UI event loop or campaign owner.
    let weak = panel.as_weak();
    let timer = slint::Timer::default();
    let flag = working;
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_secs(3),
        move || {
            let Some(panel) = weak.upgrade() else {
                return;
            };
            if !panel.get_show_campaign() || flag.load(Ordering::Acquire) {
                return;
            }
            panel.global::<CampaignUi>().invoke_action("poll".into());
        },
    );
    TIMERS.with(|timers| timers.borrow_mut().push(timer));
    panel.global::<CampaignUi>().invoke_action("refresh".into());
}
thread_local! {static TIMERS:std::cell::RefCell<Vec<slint::Timer>>=const{std::cell::RefCell::new(Vec::new())};}

/// Additive operations on the existing protected MCP bridge. Approval, token
/// setup and releasing a human hold are deliberately not remote operations.
pub(crate) fn remote(
    app: &App,
    operation: String,
    input: serde_json::Value,
    call: concat_studio::edit_bridge::Pending,
) {
    if std::env::var("CONCAT_CAMPAIGN_ENABLED").as_deref() == Ok("0")
        || !app.global::<crate::ui::AiControl>().get_allow_mcp()
    {
        call.respond(serde_json::json!({"ok":false,"error":"Bật MCP Control và Campaign Manager trong app trước"}));
        return;
    }
    let attached = app
        .global::<crate::ui::AiControl>()
        .get_attachments()
        .lines()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    std::thread::spawn(move || {
        let result = (|| -> Result<serde_json::Value, String> {
            if !call.active() {
                return Err("Yêu cầu đã hết hạn; chưa thay đổi chiến dịch".into());
            }
            let (store, cfg) = settings()?;
            use serde_json::json;
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Lookup {
                campaign_id: String,
                #[serde(default)]
                video_id: Option<String>,
                #[serde(default)]
                offset: usize,
            }
            match operation.as_str() {
                "campaign.list" => {
                    if input != json!({}) {
                        return Err("List không nhận tham số".into());
                    }
                    Ok(
                        json!({"ok":true,"campaigns":store.list()?.iter().take(100).map(|r|json!({"id":r.id,"revision":r.revision,"state":r.state,"objective":r.plan().objective,"counts":r.counts()})).collect::<Vec<_>>()}),
                    )
                }
                "campaign.get" => {
                    let args: Lookup = serde_json::from_value(input)
                        .map_err(|_| "Tham số đọc chiến dịch không hợp lệ")?;
                    let r = store.read(&args.campaign_id)?;
                    if let Some(id) = args.video_id {
                        let job = r
                            .jobs
                            .iter()
                            .find(|j| j.id == id)
                            .ok_or("Video không tồn tại")?;
                        Ok(json!({"ok":true,"campaign_id":r.id,"video":job}))
                    } else {
                        Ok(
                            json!({"ok":true,"campaign_id":r.id,"revision":r.revision,"plan_revision":r.plan_revision(),"state":r.state,"plan":r.plan(),"counts":r.counts(),"total":r.jobs.len(),"offset":args.offset,"videos":r.jobs.iter().skip(args.offset).take(25).map(|j|json!({"id":j.id,"revision":j.revision,"stage":j.stage,"state":j.state,"hold":j.hold,"page_id":j.page_id,"slot_utc":j.slot_utc,"post_id":j.post_id,"post_url":j.post_url,"error":j.error})).collect::<Vec<_>>()}),
                        )
                    }
                }
                "campaign.draft" => {
                    #[derive(serde::Deserialize)]
                    #[serde(deny_unknown_fields)]
                    struct Draft {
                        plan: Plan,
                        #[serde(default)]
                        campaign_id: Option<String>,
                        #[serde(default)]
                        expected_revision: Option<u64>,
                    }
                    let args: Draft = serde_json::from_value(input)
                        .map_err(|_| "Bản nháp chiến dịch không hợp lệ")?;
                    let allowed = attached
                        .iter()
                        .filter_map(|p| p.canonicalize().ok())
                        .collect::<Vec<_>>();
                    for path in args
                        .plan
                        .providers
                        .anchor
                        .iter()
                        .chain(args.plan.providers.assets.iter())
                    {
                        let path = path.canonicalize().map_err(|_| "Tư liệu không tồn tại")?;
                        if !allowed.contains(&path) {
                            return Err("Chỉ dùng tư liệu người dùng đã đính kèm trong AI Control; không chọn đường dẫn từ xa".into());
                        }
                    }
                    if !call.active() {
                        return Err("Yêu cầu đã hết hạn; chưa tạo bản nháp".into());
                    }
                    let r = if let Some(id) = args.campaign_id {
                        store.revise(
                            &id,
                            args.plan,
                            args.expected_revision.ok_or("Thiếu expected_revision")?,
                            false,
                        )?
                    } else {
                        store.create(args.plan)?
                    };
                    Ok(
                        json!({"ok":true,"campaign_id":r.id,"revision":r.revision,"state":r.state,"requires_operator_approval":true}),
                    )
                }
                "campaign.control" => {
                    #[derive(serde::Deserialize)]
                    #[serde(deny_unknown_fields)]
                    struct Control {
                        campaign_id: String,
                        action: String,
                    }
                    let args: Control = serde_json::from_value(input)
                        .map_err(|_| "Lệnh chiến dịch không hợp lệ")?;
                    if !call.active() {
                        return Err("Yêu cầu hết hạn; chưa thay đổi trạng thái".into());
                    }
                    match args.action.as_str(){
                        "pause"|"cancel"=>{let r=store.control(&args.campaign_id,&args.action)?;Ok(json!({"ok":true,"campaign_id":r.id,"state":r.state}))}
                        "resume"=>{let r=store.control(&args.campaign_id,"resume")?;let id=r.id.clone();std::thread::spawn(move||{if let Err(error)=start_worker(&store,&cfg,&id){let _=store.control(&id,"pause");let _=store.transact(&id,|r|{r.audit("worker","launch_blocked",None,error,concat_studio::campaign::store::now());Ok(())});}});Ok(json!({"ok":true,"campaign_id":r.id,"accepted":true,"state":"starting_worker","summary":"Chỉ tiếp tục kế hoạch đã được người dùng duyệt. Theo dõi trạng thái thật; chưa coi là hoàn thành."}))}
                        "schedule"=>{let r=store.allocate(&args.campaign_id)?;Ok(json!({"ok":true,"campaign_id":r.id,"slots":r.slots}))}
                        _=>Err("MCP không được duyệt kế hoạch, cấp token, mở khóa bước thủ công hoặc đổi quyền".into())
                    }
                }
                _ => Err("API chiến dịch không được hỗ trợ".into()),
            }
        })();
        call.respond(result.unwrap_or_else(|error| serde_json::json!({"ok":false,"error":error})));
    });
}
