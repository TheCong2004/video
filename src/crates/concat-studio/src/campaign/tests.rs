use super::store::{StageOutput, TaskFailure};
use super::*;
use std::{
    path::PathBuf,
    sync::{Arc, Barrier},
};
fn plan(count: u32, publish: bool) -> Plan {
    Plan {
        objective: "Sản xuất theo kế hoạch đã duyệt".into(),
        video_count: count,
        video_seconds: 10.0,
        topic: "garden kitchen".into(),
        language: "vi-VN".into(),
        style: "natural".into(),
        unique_content: true,
        facts: vec!["Operator supplied test fact".into()],
        pages: if publish {
            vec![
                PageRule {
                    page_id: "123".into(),
                    name: "Page A".into(),
                    topics: vec![],
                    daily_limit: 3,
                    slots: vec!["08:00".into(), "13:00".into(), "19:00".into()],
                },
                PageRule {
                    page_id: "456".into(),
                    name: "Page B".into(),
                    topics: vec![],
                    daily_limit: 3,
                    slots: vec!["08:00".into(), "13:00".into(), "19:00".into()],
                },
            ]
        } else {
            vec![]
        },
        publish,
        timezone: "Asia/Bangkok".into(),
        start_date: "2030-01-01".into(),
        end_date: "2030-12-31".into(),
        completion_deadline_utc: None,
        allow_cross_page: false,
        api_budget_micros: 1_000_000,
        render_budget_seconds: count as u64 * 300,
        max_workers: 2,
        mode: Automation::FullAuto,
        approve_content: false,
        approve_publish: false,
        max_attempts: 3,
        retry_delay_seconds: 5,
        providers: Providers {
            text_model: "fixture-model".into(),
            grok_profile: String::new(),
            anchor: None,
            assets: vec![std::env::temp_dir().join("campaign-fixture.mp4")],
            allowed_fallbacks: vec![],
            text_cost_micros: 100,
            media_cost_micros: 100,
            render_reserve_seconds: 300,
        },
        locked_fields: vec![
            "video_count".into(),
            "video_seconds".into(),
            "pages".into(),
            "api_budget_micros".into(),
        ],
    }
}
fn store() -> (CampaignStore, PathBuf) {
    let root = std::env::temp_dir().join(format!("campaign-tests-{:032x}", rand::random::<u128>()));
    (CampaignStore::open(&root).unwrap(), root)
}
#[test]
fn progress_requires_live_worker_and_exposes_blockers_in_the_correct_stage() {
    let (store, root) = store();
    let mut record = activate(&store, plan(2, false));
    assert_eq!(progress::stages(&record, 100).len(), 4);
    assert!(progress::stages(&record, 100).iter().all(|step| step.state == "waiting"));
    record.jobs[0].state = JobState::Unknown;
    record.jobs[0].error = Some("Đăng nhập AI web trước".into());
    let stages = progress::stages(&record, 100);
    assert_eq!(stages[0].state, "blocked");
    assert_eq!(stages[0].error, "Đăng nhập AI web trước");
    assert_eq!(stages[1].state, "waiting");
    record.jobs[1].state = JobState::Running;
    assert_eq!(progress::stages(&record, 100)[0].state, "blocked");
    record.jobs[1].lease = Some(Lease { token:"fixture".into(), worker:"fixture".into(), expires_at:101, plan_revision:1, job_revision:1, reserved_micros:0, reserved_seconds:0 });
    assert_eq!(progress::stages(&record, 100)[0].state, "running");
    assert_eq!(progress::stages(&record, 101)[0].state, "blocked");
    record.jobs[0].state = JobState::Pending;
    record.jobs[0].error = None;
    record.jobs[1].state = JobState::Pending;
    record.state = CampaignState::Paused;
    assert!(progress::stages(&record, 102).iter().all(|step| step.state == "paused"));
    std::fs::remove_dir_all(root).unwrap();
}
fn activate(store: &CampaignStore, plan: Plan) -> Record {
    let r = store.create(plan).unwrap();
    let r = store.approve(&r.id, r.revision, &r.plan().hash()).unwrap();
    store.control(&r.id, "start").unwrap()
}
fn script(text: &str) -> Script {
    Script {
        idea: text.into(),
        topic: "garden".into(),
        audience: "home".into(),
        hook: text.into(),
        narration: text.into(),
        title: text.into(),
        caption: text.into(),
        hashtags: vec![],
        image_prompt: "operator fixture".into(),
        motion_prompt: "operator fixture".into(),
        originality: None,
    }
}
#[test]
fn production_only_needs_no_facebook_pages_and_one_grok_profile_can_serve_multiple_pages() {
    let mut p = plan(2, false);
    p.providers.assets.clear();
    p.providers.grok_profile = "one-browser-profile".into();
    assert!(p.pages.is_empty());
    p.validate().unwrap();

    p.publish = true;
    assert!(p.validate().unwrap_err().contains("Facebook Page"));
    p.pages = plan(2, true).pages;
    assert_eq!(p.pages.len(), 2);
    p.validate().unwrap();
}

#[test]
fn missing_upload_uses_grok_and_keeps_media_budget_reservation() {
    let mut p = plan(1, false);
    p.providers.assets.clear();
    assert!(p.validate().unwrap_err().contains("Nexora"));
    p.providers.grok_profile = "profile-a".into();
    assert!(p.providers.uses_grok());
    p.validate().unwrap();
    p.api_budget_micros = 199;
    assert!(p.validate().unwrap_err().contains("API"));
    p.api_budget_micros = 200;
    let (store, root) = store();
    let record = activate(&store, p);
    let content = store.claim(&record.id, "fixture", store::now()).unwrap().unwrap();
    store.settle(&record.id, &content.id, &content.lease.as_ref().unwrap().token, Ok(StageOutput::Content(script("Unique story"))), store::now()).unwrap();
    let media = store.claim(&record.id, "fixture", store::now()).unwrap().unwrap();
    assert_eq!(media.stage, Stage::Media);
    assert_eq!(media.lease.unwrap().reserved_micros, 100);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn unread_or_invalid_ai_response_pauses_the_entire_batch_without_new_claims() {
    for failure in [TaskFailure::unknown("Unparsed web reply"), TaskFailure::input("Invalid script schema")] {
        let (store, root) = store();
        let r = activate(&store, plan(9, false));
        let first = store.claim(&r.id, "fixture", store::now()).unwrap().unwrap();
        store.settle(&r.id, &first.id, &first.lease.as_ref().unwrap().token, Err(failure), store::now()).unwrap();
        let paused = store.read(&r.id).unwrap();
        assert_eq!(paused.state, CampaignState::Paused);
        assert!(store.claim(&r.id, "fixture", store::now()+1000).unwrap().is_none());
        store.control(&r.id, "start").unwrap();
        assert!(store.claim(&r.id, "fixture", store::now()+1001).unwrap().is_none());
        assert_eq!(store.read(&r.id).unwrap().state, CampaignState::Paused);
        assert_eq!(paused.jobs[0].attempt, 1);
        assert!(paused.jobs[1..].iter().all(|j| j.attempt == 0 && j.state == JobState::Pending));
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
#[test]
fn recovered_content_is_correlated_fenced_and_never_rewrites_the_script() {
    let (store,root)=store();
    let r=activate(&store,plan(1,false));
    let job=store.claim(&r.id,"fixture",store::now()).unwrap().unwrap();
    store.settle(&r.id,&job.id,&job.lease.as_ref().unwrap().token,Err(TaskFailure::unknown("Unread reply")),store::now()).unwrap();
    let held=store.read(&r.id).unwrap();
    let expected=held.jobs[0].revision;
    let request=format!("{}-content",held.jobs[0].dispatch_id.as_ref().unwrap());
    let content=script("One two three four five six seven eight nine ten eleven twelve");
    assert!(store.recover_content(&r.id,&job.id,expected,"wrong-id",content.clone()).is_err());
    assert!(store.recover_content(&r.id,&job.id,expected+1,&request,content.clone()).is_err());
    let recovered=store.recover_content(&r.id,&job.id,expected,&request,content.clone()).unwrap();
    assert_eq!(recovered.state,CampaignState::Paused);
    assert_eq!(recovered.jobs[0].state,JobState::Pending);
    assert!(recovered.jobs[0].script_locked);
    assert_eq!(recovered.jobs[0].script.as_ref().unwrap().narration,content.narration);
    assert!(recovered.jobs[0].lease.is_none());
    assert!(store.recover_content(&r.id,&job.id,expected,&request,content).is_err());
    let rebound=store.bind_grok_source(&r.id,recovered.revision,"profile-grok").unwrap();
    assert_eq!(rebound.state,CampaignState::Draft);
    assert_eq!(rebound.plan().providers.grok_profile,"profile-grok");
    assert!(rebound.plan().providers.assets.is_empty());
    assert!(rebound.jobs[0].script_locked);
    assert_eq!(rebound.jobs[0].script.as_ref().unwrap().narration,recovered.jobs[0].script.as_ref().unwrap().narration);
    assert!(rebound.approved_hash.is_none());
    drop(store);std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn locked_script_checks_other_videos_for_duplicates_but_never_itself() {
    for duplicate_other in [false,true] {
        let (store,root)=store();
        let r=activate(&store,plan(if duplicate_other{2}else{1},false));
        let content=script("One two three four five six seven eight nine ten eleven twelve");
        store.transact(&r.id,|r|{for j in &mut r.jobs{j.script=Some(content.clone());j.script_locked=true;}Ok(())}).unwrap();
        let job=store.claim(&r.id,"fixture",store::now()).unwrap().unwrap();
        let settings=engine::EngineSettings{config_root:root.clone(),data_root:root.clone(),repo:root.clone(),ffmpeg:root.clone(),python:root.clone(),font:root.clone(),node:root.clone(),web_script:root.clone()};
        let result=engine::Engine::new(settings).execute(&store,&store.read(&r.id).unwrap(),&job,Arc::new(std::sync::atomic::AtomicBool::new(false)));
        if duplicate_other {assert!(result.err().unwrap().message.contains("trùng"));}
        else {assert!(matches!(result,Ok(StageOutput::Content(_))),"locked content must validate without any AI connection");}
        drop(store);std::fs::remove_dir_all(root).unwrap();
    }
}
#[test]
fn operator_discard_is_fenced_and_resets_only_the_chosen_unknown_content() {
    let(store,root)=store();let r=activate(&store,plan(2,false));
    let job=store.claim(&r.id,"fixture",store::now()).unwrap().unwrap();
    store.settle(&r.id,&job.id,&job.lease.as_ref().unwrap().token,Err(TaskFailure::unknown("Uncertain")),store::now()).unwrap();
    let paused=store.read(&r.id).unwrap();let old=&paused.jobs[0];let request=format!("{}-content",old.dispatch_id.as_ref().unwrap());
    assert!(store.discard_content_reply(&r.id,&old.id,old.revision,"wrong").is_err());
    let updated=store.discard_content_reply(&r.id,&old.id,old.revision,&request).unwrap();
    assert_eq!(updated.state,CampaignState::Paused);assert_eq!(updated.jobs[0].state,JobState::Pending);
    assert!(updated.jobs[0].dispatch_id.is_none());assert!(updated.jobs[0].revision>old.revision);
    assert_eq!(updated.jobs[1].revision,paused.jobs[1].revision);assert_eq!(updated.jobs[1].attempt,0);
    assert!(store.discard_content_reply(&r.id,&old.id,old.revision,&request).is_err());
    drop(store);std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn review_hash_versions_and_locked_agent_policy_are_enforced() {
    let (store, root) = store();
    let r = store.create(plan(9, false)).unwrap();
    assert_eq!(r.state, CampaignState::Draft);
    assert!(store
        .claim(&r.id, "worker", store::now())
        .unwrap()
        .is_none());
    assert!(store.control(&r.id, "start").is_err());
    assert!(store.approve(&r.id, r.revision, "wrong").is_err());
    let mut changed = r.plan().clone();
    changed.video_count = 30;
    changed.render_budget_seconds = 9000;
    assert!(store
        .revise(&r.id, changed.clone(), r.revision, false)
        .is_err());
    let revised = store.revise(&r.id, changed, r.revision, true).unwrap();
    assert_eq!(revised.versions.len(), 2);
    assert_eq!(revised.jobs.len(), 30);
    assert!(revised.approved_hash.is_none());
    assert!(store.approve(&r.id, r.revision, &r.plan().hash()).is_err());
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn concurrent_claims_have_distinct_fenced_leases_and_respect_limit() {
    let (store, root) = store();
    let r = activate(&store, plan(9, false));
    let barrier = Arc::new(Barrier::new(9));
    let mut threads = vec![];
    for i in 0..8 {
        let (store, id, b) = (store.clone(), r.id.clone(), barrier.clone());
        threads.push(std::thread::spawn(move || {
            b.wait();
            store.claim(&id, &format!("w{i}"), store::now()).unwrap()
        }));
    }
    barrier.wait();
    let jobs: Vec<_> = threads
        .into_iter()
        .filter_map(|t| t.join().unwrap())
        .collect();
    assert_eq!(jobs.len(), 2);
    assert_ne!(jobs[0].id, jobs[1].id);
    assert_ne!(
        jobs[0].lease.as_ref().unwrap().token,
        jobs[1].lease.as_ref().unwrap().token
    );
    assert!(store
        .settle(
            &r.id,
            &jobs[0].id,
            "stale",
            Ok(StageOutput::Content(script("valid content"))),
            store::now()
        )
        .is_err());
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn crash_marks_dispatched_ai_unknown_but_safe_render_can_retry() {
    let (store, root) = store();
    let r = activate(&store, plan(2, false));
    let first = store.claim(&r.id, "crashed", 100).unwrap().unwrap();
    let _ = store.claim(&r.id, "other", 300).unwrap();
    let loaded = store.read(&r.id).unwrap();
    assert_eq!(loaded.jobs[0].state, JobState::Unknown);
    assert_eq!(loaded.jobs[0].attempt, 1);
    assert!(store
        .settle(
            &r.id,
            &first.id,
            &first.lease.as_ref().unwrap().token,
            Ok(StageOutput::Content(script("late receipt"))),
            301
        )
        .is_err());
    assert_eq!(
        loaded
            .jobs
            .iter()
            .filter(|j| j.id == first.id && j.state == JobState::Running)
            .count(),
        0
    );
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn budget_reservations_precede_network_and_include_semantic_reviews() {
    let (store, root) = store();
    let mut p = plan(3, false);
    p.api_budget_micros = 300;
    let r = activate(&store, p);
    let job = store.claim(&r.id, "w", 10).unwrap().unwrap();
    let token = job.lease.as_ref().unwrap().token.clone();
    store
        .ensure_reservation(&r.id, &job.id, &token, 300)
        .unwrap();
    assert!(store
        .ensure_reservation(&r.id, &job.id, &token, 301)
        .is_err());
    assert!(store.claim(&r.id, "w2", 10).unwrap().is_none());
    assert_eq!(store.read(&r.id).unwrap().state, CampaignState::Paused);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn completed_steps_survive_resume_and_manual_edit_invalidates_only_dependencies() {
    let (store, root) = store();
    let r = activate(&store, plan(2, false));
    let job = store.claim(&r.id, "w", 10).unwrap().unwrap();
    store
        .settle(
            &r.id,
            &job.id,
            &job.lease.as_ref().unwrap().token,
            Ok(StageOutput::Content(script("A distinct narration"))),
            11,
        )
        .unwrap();
    let loaded = store.read(&r.id).unwrap();
    assert_eq!(loaded.jobs[0].stage, Stage::Media);
    store.control(&r.id, "pause").unwrap();
    let edited = store
        .manual_edit(
            &r.id,
            &job.id,
            loaded.jobs[0].revision,
            Some(script("Human revision")),
            None,
        )
        .unwrap();
    assert_eq!(
        edited.jobs[0].script.as_ref().unwrap().narration,
        "Human revision"
    );
    assert!(edited.jobs[0].hold);
    assert_eq!(edited.jobs[1].revision, 0);
    assert!(store
        .manual_edit(
            &r.id,
            &job.id,
            loaded.jobs[0].revision,
            Some(script("Stale overwrite")),
            None
        )
        .is_err());
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn scheduler_scales_9_30_100_1000_without_slot_collision_or_quota_overflow() {
    use chrono::TimeZone;
    let now = chrono::Utc
        .with_ymd_and_hms(2029, 12, 31, 0, 0, 0)
        .unwrap()
        .timestamp();
    for count in [9, 30, 100, 1000] {
        let (store, root) = store();
        let mut r = store.create(plan(count, true)).unwrap();
        let started = std::time::Instant::now();
        schedule::assign(&mut r, now).unwrap();
        let mut seen = std::collections::BTreeSet::new();
        let mut daily = std::collections::BTreeMap::<(String, String), usize>::new();
        for slot in &r.slots {
            assert!(seen.insert((slot.page_id.clone(), slot.utc)));
            *daily
                .entry((slot.page_id.clone(), slot.local_date.clone()))
                .or_default() += 1;
        }
        assert_eq!(r.slots.len(), count as usize);
        assert!(daily.values().all(|n| *n <= 3));
        assert!(started.elapsed().as_secs() < 15);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
#[test]
fn insufficient_capacity_never_silently_extends_end_date() {
    let mut p = plan(1000, true);
    p.end_date = p.start_date.clone();
    let capacity = schedule::capacity(&p, 0).unwrap();
    assert_eq!(capacity.shortage, 994);
    assert_eq!(p.end_date, "2030-01-01");
}
#[test]
fn timezone_dst_skips_nonexistent_hour_and_uses_repeated_hour_once() {
    let mut p = plan(2, true);
    p.pages.truncate(1);
    p.timezone = "America/New_York".into();
    p.start_date = "2030-03-10".into();
    p.end_date = p.start_date.clone();
    p.pages[0].slots = vec!["02:30".into(), "03:30".into()];
    let c = schedule::capacity(&p, 0).unwrap();
    assert_eq!(c.slots.len(), 1);
    p.start_date = "2030-11-03".into();
    p.end_date = p.start_date.clone();
    p.pages[0].slots = vec!["01:30".into()];
    assert_eq!(schedule::capacity(&p, 0).unwrap().slots.len(), 1);
}
#[test]
fn topic_matching_reassigns_flexible_content_to_leave_scarce_page_capacity() {
    let (store, root) = store();
    let mut p = plan(2, true);
    p.start_date = "2030-01-01".into();
    p.end_date = p.start_date.clone();
    for page in &mut p.pages {
        page.daily_limit = 1;
        page.slots = vec!["08:00".into()];
    }
    p.pages[0].topics = vec!["garden".into()];
    p.pages[1].topics = vec!["kitchen".into()];
    let mut r = store.create(p).unwrap();
    r.jobs[0].script = Some(script("Flexible"));
    r.jobs[0].script.as_mut().unwrap().topic = "garden kitchen".into();
    r.jobs[1].script = Some(script("Garden only"));
    schedule::assign(&mut r, 0).unwrap();
    assert_eq!(r.jobs[1].page_id.as_deref(), Some("123"));
    assert_eq!(r.jobs[0].page_id.as_deref(), Some("456"));
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn global_reservations_prevent_cross_campaign_duplicate_slots() {
    let (store, root) = store();
    let a = store.create(plan(9, true)).unwrap();
    let b = store.create(plan(9, true)).unwrap();
    let a = store.allocate(&a.id).unwrap();
    let b = store.allocate(&b.id).unwrap();
    assert!(a.slots.iter().all(|one| !b
        .slots
        .iter()
        .any(|two| one.page_id == two.page_id && one.utc == two.utc)));
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn cancelling_releases_only_undispatched_page_slots_for_other_campaigns() {
    let (store, root) = store();
    let a = store.create(plan(9, true)).unwrap();
    let b = store.create(plan(9, true)).unwrap();
    let a = store.allocate(&a.id).unwrap();
    let b_before = store.allocate(&b.id).unwrap();
    assert!(!a.slots.iter().any(|one| b_before
        .slots
        .iter()
        .any(|two| one.page_id == two.page_id && one.utc == two.utc)));
    store.control(&a.id, "cancel").unwrap();
    let b_after = store.allocate(&b.id).unwrap();
    assert!(b_after.slots.len() >= b_before.slots.len());
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn read_only_publish_receipt_recovery_is_revision_and_upload_bound() {
    let (store, root) = store();
    let r = store.create(plan(1, true)).unwrap();
    store
        .transact(&r.id, |record| {
            let job = &mut record.jobs[0];
            job.stage = Stage::Publish;
            job.state = JobState::Unknown;
            job.publish_dispatched = true;
            job.upload_id = Some("platform-video-1".into());
            job.page_id = Some("123".into());
            job.revision = 3;
            Ok(())
        })
        .unwrap();
    assert!(store
        .reconcile_published(
            &r.id,
            "video-00001",
            2,
            "platform-video-1",
            "https://facebook.com/reel/1",
            store::now()
        )
        .is_err());
    assert!(store
        .reconcile_published(
            &r.id,
            "video-00001",
            3,
            "other-video",
            "https://facebook.com/reel/1",
            store::now()
        )
        .is_err());
    store
        .reconcile_published(
            &r.id,
            "video-00001",
            3,
            "platform-video-1",
            "https://facebook.com/reel/1",
            store::now(),
        )
        .unwrap();
    let r = store.read(&r.id).unwrap();
    assert_eq!(r.state, CampaignState::Completed);
    assert_eq!(r.counts().get("published"), Some(&1));
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn unknown_failures_and_local_files_never_increment_published_count() {
    let (store, root) = store();
    let r = activate(&store, plan(1, false));
    let job = store.claim(&r.id, "w", 10).unwrap().unwrap();
    store
        .settle(
            &r.id,
            &job.id,
            &job.lease.as_ref().unwrap().token,
            Err(TaskFailure::unknown("Transport lost")),
            11,
        )
        .unwrap();
    let r = store.read(&r.id).unwrap();
    assert_eq!(r.counts().get("published").copied().unwrap_or(0), 0);
    assert_eq!(r.jobs[0].state, JobState::Unknown);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn explicit_thousand_count_and_clip_duration_are_not_replaced_by_campaign_deadline() {
    let p = plan(1000, false);
    assert!(check_request_targets(
        "Tạo 1.000 video, mỗi video dài 10 giây, hoàn thành trong 60 phút",
        &p
    )
    .is_ok());
    let mut wrong = p.clone();
    wrong.video_count = 100;
    assert!(check_request_targets("Tạo 1.000 video, mỗi video dài 10 giây", &wrong).is_err());
    wrong = p.clone();
    wrong.video_seconds = 3600.0;
    assert!(check_request_targets(
        "Tạo 1.000 video, mỗi video dài 10 giây rồi hoàn thành trong 60 phút",
        &wrong
    )
    .is_err());
}
