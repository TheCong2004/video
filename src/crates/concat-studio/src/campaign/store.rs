// SPDX-License-Identifier: AGPL-3.0-or-later
//! Campaign transactions use the same native atomic-file/OS-lock pattern as the
//! existing shorts queue. No editor project or provider credentials are owned here.
use super::*;
use serde_json::Value;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub struct CampaignStore {
    root: PathBuf,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Reservation {
    campaign: String,
    slot: PublishingSlot,
}
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
fn id_valid(id: &str) -> bool {
    id.starts_with("campaign-") && id.len() == 41 && id[9..].bytes().all(|b| b.is_ascii_hexdigit())
}
fn atomic(path: &Path, value: &impl serde::Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err(
            "Dữ liệu chiến dịch vượt 32 MiB; cần lưu trữ lịch sử trước khi tiếp tục".into(),
        );
    }
    let temp = path.with_extension(format!("{:016x}.tmp", rand::random::<u64>()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.write_all(b"\n"))
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    drop(file);
    fs::rename(&temp, path).map_err(|e| e.to_string())
}
impl CampaignStore {
    pub fn open(root: &Path) -> Result<Self, String> {
        fs::create_dir_all(root).map_err(|e| e.to_string())?;
        Ok(Self {
            root: root.canonicalize().map_err(|e| e.to_string())?,
        })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn directory(&self, id: &str) -> Result<PathBuf, String> {
        if !id_valid(id) {
            return Err("Campaign ID không hợp lệ".into());
        }
        Ok(self.root.join(id))
    }
    fn lock(&self, id: &str) -> Result<File, String> {
        let dir = self.directory(id)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(dir.join("campaign.lock"))
            .map_err(|e| e.to_string())?;
        file.lock().map_err(|e| e.to_string())?;
        Ok(file)
    }
    fn read_unlocked(&self, id: &str) -> Result<Record, String> {
        let path = self.directory(id)?.join("campaign.json");
        let info = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if !info.is_file() || info.file_type().is_symlink() || info.len() > 32 * 1024 * 1024 {
            return Err("Hồ sơ chiến dịch vượt giới hạn hoặc không hợp lệ".into());
        }
        let mut data = Vec::new();
        File::open(path)
            .map_err(|e| e.to_string())?
            .take(32 * 1024 * 1024 + 1)
            .read_to_end(&mut data)
            .map_err(|e| e.to_string())?;
        let record: Record = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
        if record.schema != 1
            || record.id != id
            || record.versions.is_empty()
            || record.jobs.len() > 1000
        {
            return Err("Hồ sơ chiến dịch không tương thích".into());
        }
        Ok(record)
    }
    pub fn read(&self, id: &str) -> Result<Record, String> {
        let _guard = self.lock(id)?;
        self.read_unlocked(id)
    }
    pub fn transact<T>(
        &self,
        id: &str,
        change: impl FnOnce(&mut Record) -> Result<T, String>,
    ) -> Result<T, String> {
        let _guard = self.lock(id)?;
        let mut record = self.read_unlocked(id)?;
        let result = change(&mut record)?;
        if record.audit.len() > 50000 || record.usage.len() > 50000 {
            return Err("Cần lưu trữ nhật ký trước khi chạy thêm".into());
        }
        atomic(&self.directory(id)?.join("campaign.json"), &record)?;
        Ok(result)
    }
    pub fn list(&self) -> Result<Vec<Record>, String> {
        let mut result = Vec::new();
        for entry in fs::read_dir(&self.root)
            .map_err(|e| e.to_string())?
            .flatten()
            .take(10000)
        {
            if let Some(id) = entry.file_name().to_str() {
                if id_valid(id) {
                    result.push(self.read(id)?);
                }
            }
        }
        result.sort_by_key(|r| std::cmp::Reverse(r.created_at));
        Ok(result)
    }
    pub fn create(&self, plan: Plan) -> Result<Record, String> {
        plan.validate()?;
        let id = format!("campaign-{:032x}", rand::random::<u128>());
        let dir = self.directory(&id)?;
        fs::create_dir(&dir).map_err(|e| e.to_string())?;
        let at = now();
        let mut record = Record {
            schema: 1,
            id,
            revision: 0,
            state: CampaignState::Draft,
            approved_hash: None,
            versions: vec![PlanVersion {
                revision: 1,
                hash: plan.hash(),
                plan: plan.clone(),
                created_at: at,
            }],
            jobs: (0..plan.video_count).map(VideoJob::new).collect(),
            slots: vec![],
            audit: vec![],
            usage: vec![],
            created_at: at,
        };
        record.audit(
            "operator",
            "draft_created",
            None,
            "Chưa phê duyệt hoặc gửi yêu cầu sản xuất",
            at,
        );
        atomic(&dir.join("campaign.json"), &record)?;
        Ok(record)
    }
    pub fn revise(
        &self,
        id: &str,
        plan: Plan,
        expected: u64,
        operator: bool,
    ) -> Result<Record, String> {
        plan.validate()?;
        self.transact(id,|r|{
            if r.revision!=expected || r.jobs.iter().any(|j|j.lease.is_some()) || matches!(r.state,CampaignState::Running|CampaignState::Cancelled|CampaignState::Completed){return Err("Tạm dừng worker và tải lại phiên bản trước khi sửa kế hoạch".into());}
            if !operator {
                let old=serde_json::to_value(r.plan()).map_err(|e|e.to_string())?;let next=serde_json::to_value(&plan).map_err(|e|e.to_string())?;
                for field in r.plan().locked_fields.iter().chain(["locked_fields".to_owned()].iter()){if old[field]!=next[field]{return Err(format!("AI không được thay trường đã khóa: {field}"));}}
            }
            if r.jobs.iter().any(|j|j.script.is_some()||j.rendered.is_some()||j.publish_dispatched) && plan.video_count!=r.plan().video_count {return Err("Không thay số lượng sau khi đã sản xuất. Tạo chiến dịch mới để giữ nguyên lịch sử.".into());}
            if r.jobs.iter().any(|j|j.script.is_some()) {
                let old=r.plan();
                if plan.video_seconds!=old.video_seconds || plan.topic!=old.topic || plan.facts!=old.facts || plan.language!=old.language || plan.style!=old.style || plan.providers!=old.providers || plan.unique_content!=old.unique_content {
                    return Err("Đã có nội dung theo phiên bản cũ. Dùng sửa từng video hoặc tạo chiến dịch mới khi thay yêu cầu sản xuất; không giữ kết quả cũ rồi tính đạt kế hoạch mới.".into());
                }
            }
            if plan.video_count!=r.plan().video_count {r.jobs=(0..plan.video_count).map(VideoJob::new).collect();}
            let version=r.plan_revision()+1;r.versions.push(PlanVersion{revision:version,hash:plan.hash(),plan,created_at:now()});
            r.approved_hash=None;r.state=CampaignState::Draft;r.audit(if operator{"operator"}else{"agent"},"plan_revised",None,format!("revision {version}; cần duyệt lại"),now());Ok(r.clone())
        })
    }
    pub fn approve(&self, id: &str, expected: u64, hash: &str) -> Result<Record, String> {
        self.transact(id, |r| {
            if r.revision != expected || r.plan().hash() != hash || r.state != CampaignState::Draft
            {
                return Err("Kế hoạch đã thay đổi; tải lại rồi duyệt đúng phiên bản".into());
            }
            let capacity = super::schedule::capacity(r.plan(), now())?;
            if capacity.shortage > 0 {
                return Err(format!(
                    "Thiếu {} slot trong thời hạn đã chọn",
                    capacity.shortage
                ));
            }
            r.approved_hash = Some(hash.into());
            r.state = CampaignState::Approved;
            r.audit("operator", "plan_approved", None, hash, now());
            Ok(r.clone())
        })
    }
    pub fn control(&self, id: &str, action: &str) -> Result<Record, String> {
        let record = self.transact(id, |r| {
            match action {
                "start" | "resume" => {
                    if r.approved_hash.as_deref() != Some(r.plan().hash().as_str())
                        || matches!(r.state, CampaignState::Cancelled | CampaignState::Completed)
                    {
                        return Err("Phải duyệt đúng phiên bản trước khi chạy".into());
                    }
                    r.state = CampaignState::Running;
                }
                "pause" => {
                    if matches!(r.state, CampaignState::Cancelled | CampaignState::Completed) {
                        return Err("Chiến dịch đã kết thúc".into());
                    }
                    r.state = CampaignState::Paused;
                }
                "cancel" => {
                    r.state = CampaignState::Cancelled;
                    for job in &mut r.jobs {
                        if job.lease.is_none()
                            && !matches!(job.state, JobState::Published | JobState::Completed)
                        {
                            job.state = JobState::Cancelled;
                        }
                    }
                }
                _ => return Err("Lệnh chiến dịch không hợp lệ".into()),
            }
            r.audit("operator", action, None, "", now());
            Ok(r.clone())
        })?;
        if action == "cancel" {
            self.release_cancelled_slots(id)?;
        }
        Ok(record)
    }
    pub fn claim(&self, id: &str, worker: &str, at: i64) -> Result<Option<VideoJob>, String> {
        self.transact(id,|r|{
        if r.state!=CampaignState::Running || r.approved_hash.as_deref()!=Some(r.plan().hash().as_str()){return Ok(None);}
        let plan=r.plan().clone();
        for job in &mut r.jobs {
            if job.lease.as_ref().is_some_and(|l|l.expires_at<at){
                job.lease=None;job.revision+=1;
                if matches!(job.stage,Stage::Render|Stage::Quality){job.state=JobState::RetryWait;job.retry_at=at+plan.retry_delay_seconds as i64;}else{job.state=JobState::Unknown;}
                job.error=Some("Worker bị ngắt; không tự gửi lại thao tác có kết quả chưa rõ".into());
            }
        }
        if r.jobs.iter().any(|j| matches!(j.stage,Stage::Content|Stage::Media) && matches!(j.state,JobState::Unknown|JobState::NeedsReview)) {
            r.state=CampaignState::Paused;
            return Ok(None);
        }
        if r.jobs.iter().filter(|j|j.lease.is_some()).count()>=plan.max_workers as usize{return Ok(None);}
        let spent=r.usage.iter().map(|u|u.micros).sum::<u64>();let rendered=r.usage.iter().map(|u|u.render_seconds).sum::<u64>();
        let reserved=r.jobs.iter().filter_map(|j|j.lease.as_ref()).map(|l|l.reserved_micros).sum::<u64>();
        let reserved_seconds=r.jobs.iter().filter_map(|j|j.lease.as_ref()).map(|l|l.reserved_seconds).sum::<u64>();
        let index=r.jobs.iter().position(|j|!j.hold && j.lease.is_none() && j.attempt<plan.max_attempts
            && matches!(j.state,JobState::Pending|JobState::ReadyToPublish|JobState::RetryWait) && j.retry_at<=at
            && (j.stage!=Stage::Publish || (j.slot_utc.is_some_and(|time|time<=at) && j.page_id.is_some())));
        let Some(index)=index else{return Ok(None);};
        let content_calls=1+if plan.unique_content{novelty_groups(r,&r.jobs[index].id).len() as u64}else{0};
        let job=&mut r.jobs[index];
        let needs=(job.stage==Stage::Media && plan.approve_content) || (job.stage==Stage::Publish && (plan.approve_publish || plan.mode==Automation::Manual));
        if needs && job.approval_revision!=Some(job.revision){job.state=JobState::NeedsApproval;return Ok(None);}
        let money=match job.stage {Stage::Content=>plan.providers.text_cost_micros.saturating_mul(content_calls),Stage::Media if plan.providers.uses_grok()=>plan.providers.media_cost_micros,_=>0};
        let seconds=if job.stage==Stage::Render{plan.providers.render_reserve_seconds}else{0};
        if spent.saturating_add(reserved).saturating_add(money)>plan.api_budget_micros || rendered.saturating_add(reserved_seconds).saturating_add(seconds)>plan.render_budget_seconds {
            r.state=CampaignState::Paused;r.audit("worker","budget_blocked",None,"Ngân sách dự trù không còn đủ; cần người dùng quyết định",at);return Ok(None);
        }
        job.attempt+=1;job.state=JobState::Running;
        job.dispatch_id=Some(crate::ai_control::digest(&serde_json::json!({"campaign":id,"video":job.id,"stage":job.stage,"revision":job.revision,"attempt":job.attempt})));
        job.lease=Some(Lease{token:format!("{:032x}",rand::random::<u128>()),worker:worker.into(),expires_at:at+90,plan_revision:r.versions.last().unwrap().revision,job_revision:job.revision,reserved_micros:money,reserved_seconds:seconds});
        let job=job.clone();r.audit(worker,"task_claimed",Some(&job.id),format!("{:?}, attempt {}",job.stage,job.attempt),at);Ok(Some(job))
    })
    }
    pub fn heartbeat(&self, id: &str, video: &str, token: &str, at: i64) -> Result<bool, String> {
        self.transact(id, |r| {
            let running = r.state == CampaignState::Running;
            let j = r
                .jobs
                .iter_mut()
                .find(|j| j.id == video)
                .ok_or("Video không tồn tại")?;
            let lease = j
                .lease
                .as_mut()
                .filter(|l| l.token == token && l.job_revision == j.revision)
                .ok_or("Lease không còn hợp lệ")?;
            lease.expires_at = at + 90;
            Ok(running && !j.hold)
        })
    }
    pub fn checkpoint(
        &self,
        id: &str,
        video: &str,
        token: &str,
        update: impl FnOnce(&mut VideoJob) -> Result<(), String>,
    ) -> Result<(), String> {
        self.transact(id, |r| {
            let j = r
                .jobs
                .iter_mut()
                .find(|j| j.id == video)
                .ok_or("Video không tồn tại")?;
            if !j
                .lease
                .as_ref()
                .is_some_and(|l| l.token == token && l.job_revision == j.revision)
            {
                return Err("Lease cũ không được ghi kết quả".into());
            }
            update(j)
        })
    }
    pub fn ensure_reservation(
        &self,
        id: &str,
        video: &str,
        token: &str,
        needed: u64,
    ) -> Result<(), String> {
        self.transact(id, |r| {
            let spent = r
                .usage
                .iter()
                .fold(0u64, |sum, u| sum.saturating_add(u.micros));
            let reserved = r
                .jobs
                .iter()
                .filter_map(|j| j.lease.as_ref())
                .fold(0u64, |sum, l| sum.saturating_add(l.reserved_micros));
            let j = r
                .jobs
                .iter_mut()
                .find(|j| j.id == video)
                .ok_or("Video không tồn tại")?;
            let lease = j
                .lease
                .as_mut()
                .filter(|l| l.token == token)
                .ok_or("Lease không hợp lệ")?;
            let extra = needed.saturating_sub(lease.reserved_micros);
            if spent.saturating_add(reserved).saturating_add(extra)
                > r.versions.last().unwrap().plan.api_budget_micros
            {
                return Err(
                    "Không đủ ngân sách cho số lượt kiểm tra ngữ nghĩa; chưa gửi thêm AI".into(),
                );
            }
            lease.reserved_micros = lease.reserved_micros.max(needed);
            Ok(())
        })
    }
    pub fn may_dispatch(&self, id: &str, video: &str, token: &str) -> Result<(), String> {
        let r = self.read(id)?;
        let j = r
            .jobs
            .iter()
            .find(|j| j.id == video)
            .ok_or("Video không tồn tại")?;
        if r.state != CampaignState::Running
            || j.hold
            || !j
                .lease
                .as_ref()
                .is_some_and(|l| l.token == token && l.job_revision == j.revision)
        {
            return Err("Chiến dịch đã dừng, video được giữ hoặc lease đã đổi".into());
        }
        Ok(())
    }
    pub fn settle(
        &self,
        id: &str,
        video: &str,
        token: &str,
        result: Result<StageOutput, TaskFailure>,
        at: i64,
    ) -> Result<(), String> {
        self.transact(id, |r| {
            let plan = r.plan().clone();
            let job = r
                .jobs
                .iter_mut()
                .find(|j| j.id == video)
                .ok_or("Video không tồn tại")?;
            let lease = job
                .lease
                .clone()
                .filter(|l| {
                    l.token == token
                        && l.job_revision == job.revision
                        && l.plan_revision == r.versions.last().unwrap().revision
                })
                .ok_or("Kết quả từ lease hoặc phiên bản cũ")?;
            let stage = job.stage.clone();
            job.lease = None;
            job.revision += 1;
            job.approval_revision = None;
            let mut usage_seconds = 0;
            match result {
                Ok(output) => {
                    match (stage.clone(), output) {
                        (Stage::Content, StageOutput::Content(script)) => {
                            let keep_media = job.script.as_ref().is_some_and(|old| {
                                old.image_prompt == script.image_prompt
                                    && old.motion_prompt == script.motion_prompt
                            }) && !job.media.is_empty();
                            if let Some(old) = &job.script {
                                if old.narration != script.narration {
                                    if job.script_versions.len() >= 20 {
                                        return Err(
                                            "Cần lưu trữ lịch sử kịch bản trước khi sửa thêm"
                                                .into(),
                                        );
                                    }
                                    job.script_versions.push(old.clone());
                                }
                            }
                            job.script = Some(script);
                            if keep_media {
                                job.stage = Stage::Render;
                            } else {
                                job.stage = Stage::Media;
                                job.media.clear();
                            }
                        }
                        (Stage::Media, StageOutput::Media(assets)) if !assets.is_empty() => {
                            job.media = assets;
                            job.stage = Stage::Render;
                        }
                        (Stage::Render, StageOutput::Render(asset, seconds)) => {
                            job.rendered = Some(asset);
                            job.stage = Stage::Quality;
                            usage_seconds = seconds;
                        }
                        (Stage::Quality, StageOutput::Quality(report))
                            if report.decoded
                                && report.audio
                                && report.audible
                                && report.subtitles_in_frame =>
                        {
                            job.quality = Some(report);
                            job.stage = if plan.publish {
                                Stage::Publish
                            } else {
                                Stage::Done
                            };
                        }
                        (
                            Stage::Publish,
                            StageOutput::Published {
                                post_id,
                                post_url,
                                published_at,
                            },
                        ) if !post_id.is_empty() && valid_post_url(&post_url) => {
                            job.post_id = Some(post_id);
                            job.post_url = Some(post_url);
                            job.published_at = Some(published_at);
                            job.stage = Stage::Done;
                        }
                        _ => {
                            return Err("Kết quả không khớp công đoạn hoặc chưa đạt kiểm tra".into())
                        }
                    }
                    job.attempt = 0;
                    job.error = None;
                    job.state = if job.stage == Stage::Done {
                        if plan.publish {
                            JobState::Published
                        } else {
                            JobState::Completed
                        }
                    } else if job.stage == Stage::Publish {
                        JobState::ReadyToPublish
                    } else {
                        JobState::Pending
                    };
                }
                Err(failure) => {
                    // A transport/schema problem is not permission to consume
                    // the rest of the batch. Retain all jobs and stop new claims.
                    if failure.unknown || failure.needs_user {
                        r.state = CampaignState::Paused;
                    }
                    usage_seconds = failure.render_seconds;
                    job.error = Some(failure.message);
                    job.state = if failure.unknown {
                        JobState::Unknown
                    } else if failure.needs_user {
                        JobState::NeedsReview
                    } else if failure.repair_content
                        && !job.script_locked
                        && job.timing_repairs < plan.max_attempts
                    {
                        job.timing_repairs += 1;
                        job.stage = Stage::Content;
                        job.attempt = 0;
                        job.retry_at = at + plan.retry_delay_seconds as i64;
                        JobState::RetryWait
                    } else if stage == Stage::Quality && job.repair_attempts < plan.max_attempts {
                        job.repair_attempts += 1;
                        job.stage = Stage::Render;
                        job.rendered = None;
                        job.quality = None;
                        job.attempt = 0;
                        job.retry_at = at + plan.retry_delay_seconds as i64;
                        JobState::RetryWait
                    } else if stage == Stage::Quality || failure.repair_content {
                        JobState::NeedsReview
                    } else if job.attempt < plan.max_attempts {
                        job.retry_at = at
                            + plan.retry_delay_seconds as i64
                                * 2_i64.pow((job.attempt - 1).min(5) as u32);
                        JobState::RetryWait
                    } else {
                        JobState::Failed
                    };
                }
            }
            job.actual_micros = job.actual_micros.saturating_add(lease.reserved_micros);
            job.render_seconds = job.render_seconds.saturating_add(usage_seconds);
            r.usage.push(UsageRecord {
                at,
                video_id: video.into(),
                stage,
                micros: lease.reserved_micros,
                render_seconds: usage_seconds,
                estimated: true,
            });
            r.audit(
                &lease.worker,
                "task_settled",
                Some(video),
                "Kết quả và chi phí dự trù được lưu; không suy diễn thành công",
                at,
            );
            if r.jobs
                .iter()
                .all(|j| matches!(j.state, JobState::Completed | JobState::Published))
            {
                r.state = CampaignState::Completed;
            }
            Ok(())
        })
    }
    pub fn manual_edit(
        &self,
        id: &str,
        video: &str,
        expected: u64,
        script: Option<Script>,
        replacement: Option<Asset>,
    ) -> Result<Record, String> {
        self.transact(id,|r|{
        if r.state==CampaignState::Cancelled{return Err("Chiến dịch đã hủy; không mở lại tác vụ tự động".into());}
        let changed_script=script.is_some();
        let job=r.jobs.iter_mut().find(|j|j.id==video).ok_or("Video không tồn tại")?;
        if job.revision!=expected || job.lease.is_some() || job.publish_dispatched || job.state==JobState::Published{return Err("Dừng và đợi tác vụ kết thúc; không sửa bài đã gửi đăng hoặc phiên bản cũ".into());}
        if let Some(mut script)=script{if script.narration.trim().is_empty()||script.narration.len()>12000{return Err("Lời đọc không được trống hoặc quá dài".into());}
            if job.script_versions.len()>=20{return Err("Cần lưu trữ lịch sử kịch bản trước khi sửa thêm".into());}
            let keep_media=job.script.as_ref().is_some_and(|old|old.image_prompt==script.image_prompt&&old.motion_prompt==script.motion_prompt);
            if let Some(old)=job.script.take(){job.script_versions.push(old);}script.originality=None;job.script=Some(script);job.script_locked=true;job.stage=Stage::Content;if !keep_media{job.media.clear();}
        }
        if let Some(asset)=replacement{job.media=vec![asset];if !changed_script{job.stage=Stage::Render;}}
        job.rendered=None;job.quality=None;job.slot_utc=None;job.page_id=None;job.revision+=1;job.approval_revision=None;job.attempt=0;job.error=None;job.state=JobState::Pending;job.hold=true;
        r.slots.iter_mut().filter(|s|s.video_id.as_deref()==Some(video)).for_each(|s|s.video_id=None);
        if r.state==CampaignState::Completed{r.state=CampaignState::Paused;}
        r.audit("operator","manual_edit",Some(video),"Giữ lời đọc của người dùng; chỉ kiểm tra lại, không tự viết đè; bảo toàn tư liệu còn hợp lệ",now());Ok(r.clone())
    })
    }
    pub fn hold(&self, id: &str, video: &str, hold: bool) -> Result<(), String> {
        self.transact(id, |r| {
            let job = r
                .jobs
                .iter_mut()
                .find(|j| j.id == video)
                .ok_or("Video không tồn tại")?;
            job.hold = hold;
            r.audit(
                "operator",
                if hold { "video_hold" } else { "video_continue" },
                Some(video),
                "",
                now(),
            );
            Ok(())
        })
    }
    /// Adopt an operator-supplied reply for the exact retained content dispatch.
    /// Never submits AI work or bypasses later originality/media validation.
    pub fn recover_content(&self, id: &str, video: &str, expected: u64, request_id: &str, mut script: Script) -> Result<Record, String> {
        self.transact(id, |r| {
            if r.state != CampaignState::Paused { return Err("Tạm dừng chiến dịch trước khi khôi phục phản hồi".into()); }
            let plan = r.plan();
            let words = script.narration.split_whitespace().count() as f64;
            if [&script.idea,&script.topic,&script.audience,&script.hook,&script.narration,&script.title,&script.image_prompt,&script.motion_prompt].iter().any(|s|s.trim().is_empty())
                || script.narration.len()>12000 || script.image_prompt.len()>4000 || script.motion_prompt.len()>4000
                || script.title.len()>1000 || script.caption.len()>8000 || script.hashtags.len()>30
                || words<plan.video_seconds || words>plan.video_seconds*6.5 {
                return Err("Phản hồi thiếu trường, vượt giới hạn hoặc không phù hợp thời lượng".into());
            }
            let normalize=|text:&str|text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
            if plan.unique_content && r.jobs.iter().filter(|j|j.id!=video).filter_map(|j|j.script.as_ref()).any(|s|normalize(&s.narration)==normalize(&script.narration)) {
                return Err("Kịch bản trùng video khác trong chiến dịch".into());
            }
            let job=r.jobs.iter_mut().find(|j|j.id==video).ok_or("Video không tồn tại")?;
            if job.revision!=expected || job.lease.is_some() || job.stage!=Stage::Content
                || !matches!(job.state,JobState::Unknown|JobState::NeedsReview|JobState::Failed)
                || job.dispatch_id.as_ref().map(|d|format!("{d}-content")).as_deref()!=Some(request_id) {
                return Err("Phản hồi không khớp lượt gửi, phiên bản hoặc trạng thái video".into());
            }
            if job.script_versions.len()>=20{return Err("Lịch sử kịch bản đã đầy".into());}
            if let Some(old)=job.script.take(){job.script_versions.push(old);}
            script.originality=None;
            job.script=Some(script);job.script_locked=true;job.state=JobState::Pending;job.hold=false;
            job.revision+=1;job.attempt=0;job.retry_at=0;job.error=None;job.approval_revision=None;
            r.audit("operator","content_reply_recovered",Some(video),format!("Đã nhận phản hồi {request_id}; không gọi AI viết lại; vẫn kiểm tra nội dung trước khi tạo tư liệu"),now());
            Ok(r.clone())
        })
    }
    /// Change only the unexecuted media source, preserving recovered scripts.
    /// A new plan version still requires approval before any provider dispatch.
    pub fn bind_grok_source(&self,id:&str,expected:u64,profile:&str)->Result<Record,String>{
        self.transact(id,|r|{
            if r.revision!=expected || r.state!=CampaignState::Paused || r.jobs.iter().any(|j|j.lease.is_some()||!j.media.is_empty()||j.rendered.is_some()||j.quality.is_some()||j.publish_dispatched){
                return Err("Chỉ đổi nguồn tạo tư liệu khi chiến dịch đã dừng và chưa tạo tư liệu/video".into());
            }
            if profile.is_empty()||profile.len()>128||!profile.bytes().all(|b|b.is_ascii_alphanumeric()||b"-_".contains(&b)){
                return Err("Hồ sơ Grok không hợp lệ".into());
            }
            let mut plan=r.plan().clone();
            plan.providers.grok_profile=profile.into();plan.providers.anchor=None;plan.providers.assets.clear();
            plan.validate()?;
            let version=r.plan_revision()+1;
            r.versions.push(PlanVersion{revision:version,hash:plan.hash(),plan,created_at:now()});
            r.approved_hash=None;r.state=CampaignState::Draft;
            r.audit("operator","media_source_rebound",None,"Đổi nguồn tư liệu chưa thực hiện sang Grok; giữ nguyên nội dung; cần duyệt phiên bản mới",now());
            Ok(r.clone())
        })
    }
    /// Explicit operator discard of an uncertain content reply, not an auto retry.
    pub fn discard_content_reply(&self,id:&str,video:&str,expected:u64,request_id:&str)->Result<Record,String>{
        self.transact(id,|r|{
            if r.state!=CampaignState::Paused{return Err("Tạm dừng trước khi bỏ lượt phản hồi cũ".into());}
            let job=r.jobs.iter_mut().find(|j|j.id==video).ok_or("Video không tồn tại")?;
            if job.revision!=expected||job.lease.is_some()||job.stage!=Stage::Content||job.state!=JobState::Unknown
                ||job.script.is_some()||!job.media.is_empty()||job.rendered.is_some()||job.publish_dispatched
                ||job.dispatch_id.as_ref().map(|d|format!("{d}-content")).as_deref()!=Some(request_id){
                return Err("Lượt cũ, phiên bản hoặc trạng thái không khớp; không thay đổi video".into());
            }
            job.dispatch_id=None;job.revision+=1;job.attempt=0;job.retry_at=0;job.error=None;job.state=JobState::Pending;job.approval_revision=None;
            r.audit("operator","content_reply_explicitly_discarded",Some(video),format!("Người dùng chọn tạo lượt mới; giữ biên nhận cũ {request_id}"),now());
            Ok(r.clone())
        })
    }
    pub fn approve_video(&self, id: &str, video: &str, expected: u64) -> Result<(), String> {
        self.transact(id, |r| {
            let job = r
                .jobs
                .iter_mut()
                .find(|j| j.id == video)
                .ok_or("Video không tồn tại")?;
            if job.revision != expected || job.state != JobState::NeedsApproval {
                return Err("Nội dung đã đổi hoặc không chờ duyệt".into());
            }
            job.approval_revision = Some(expected);
            job.state = JobState::Pending;
            r.audit(
                "operator",
                "video_approved",
                Some(video),
                format!("revision {expected}"),
                now(),
            );
            Ok(())
        })
    }
    fn publishing_lock(&self) -> Result<File, String> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.root.join("publishing.lock"))
            .map_err(|e| e.to_string())?;
        file.lock().map_err(|e| e.to_string())?;
        Ok(file)
    }
    /// Release only cancelled, undispatched reservations. Unknown writes remain reserved.
    pub fn release_cancelled_slots(&self, id: &str) -> Result<(), String> {
        let _global = self.publishing_lock()?;
        let record = self.read(id)?;
        if record.state != CampaignState::Cancelled {
            return Ok(());
        }
        let mut reservations = self.reservations()?;
        reservations.retain(|entry| {
            entry.campaign != id
                || record.jobs.iter().any(|job| {
                    entry.slot.video_id.as_deref() == Some(job.id.as_str())
                        && (job.lease.is_some()
                            || job.publish_dispatched
                            || job.state == JobState::Published)
                })
        });
        atomic(&self.root.join("publishing-slots.json"), &reservations)
    }
    /// Commit a verified read-only platform receipt against the exact saved upload.
    pub fn reconcile_published(
        &self,
        id: &str,
        video: &str,
        revision: u64,
        upload: &str,
        url: &str,
        at: i64,
    ) -> Result<(), String> {
        if !valid_post_url(url) {
            return Err("URL bài đăng chưa hợp lệ".into());
        }
        self.transact(id, |r| {
            let job = r
                .jobs
                .iter_mut()
                .find(|j| j.id == video)
                .ok_or("Video không tồn tại")?;
            if job.revision != revision
                || job.lease.is_some()
                || job.upload_id.as_deref() != Some(upload)
                || job.stage != Stage::Publish
                || !job.publish_dispatched
            {
                return Err(
                    "Biên nhận không khớp lần đăng đang chờ; không ghi đè phiên bản mới".into(),
                );
            }
            job.post_id = Some(upload.into());
            job.post_url = Some(url.into());
            job.published_at = Some(at);
            job.stage = Stage::Done;
            job.state = JobState::Published;
            job.error = None;
            job.revision += 1;
            r.audit(
                "reconciler",
                "platform_receipt_reconciled",
                Some(video),
                "GET xác nhận bài đã công khai; không gửi thêm bài",
                at,
            );
            if r.state != CampaignState::Cancelled
                && r.jobs
                    .iter()
                    .all(|j| matches!(j.state, JobState::Published | JobState::Completed))
            {
                r.state = CampaignState::Completed;
            }
            Ok(())
        })
    }
    fn reservations(&self) -> Result<Vec<Reservation>, String> {
        let path = self.root.join("publishing-slots.json");
        if !path.exists() {
            return Ok(vec![]);
        }
        let data = fs::read(path).map_err(|e| e.to_string())?;
        if data.len() > 32 * 1024 * 1024 {
            return Err("Chỉ mục lịch quá lớn".into());
        }
        serde_json::from_slice(&data).map_err(|e| e.to_string())
    }
    pub fn allocate(&self, id: &str) -> Result<Record, String> {
        let _global = self.publishing_lock()?;
        let mut reservations = self.reservations()?;
        let others: Vec<_> = reservations
            .iter()
            .filter(|r| r.campaign != id)
            .map(|r| r.slot.clone())
            .collect();
        let result = self.transact(id, |r| {
            if r.jobs
                .iter()
                .any(|j| j.stage == Stage::Publish && j.lease.is_some())
            {
                return Err("Đợi tác vụ đăng đang chạy trước khi đổi lịch".into());
            }
            super::schedule::assign_reserved(r, now(), &others)?;
            // Reserve new slots BEFORE committing the campaign. Crash leaves
            // conservative orphan reservations, never duplicate free slots.
            for slot in &r.slots {
                if !reservations.iter().any(|old| {
                    old.campaign == id
                        && old.slot.page_id == slot.page_id
                        && old.slot.utc == slot.utc
                        && old.slot.video_id == slot.video_id
                }) {
                    reservations.push(Reservation {
                        campaign: id.into(),
                        slot: slot.clone(),
                    });
                }
            }
            atomic(&self.root.join("publishing-slots.json"), &reservations)?;
            r.audit(
                "scheduler",
                "slots_allocated",
                None,
                "Giữ chỗ toàn cục trước khi ghi lịch; không trùng chiến dịch khác",
                now(),
            );
            Ok(r.clone())
        })?;
        reservations.retain(|old| {
            old.campaign != id
                || result.slots.iter().any(|s| {
                    s.page_id == old.slot.page_id
                        && s.utc == old.slot.utc
                        && s.video_id == old.slot.video_id
                })
        });
        atomic(&self.root.join("publishing-slots.json"), &reservations)?;
        Ok(result)
    }
    pub fn assert_slot(&self, id: &str, video: &str, page: &str, utc: i64) -> Result<(), String> {
        let _global = self.publishing_lock()?;
        if !self.reservations()?.iter().any(|r| {
            r.campaign == id
                && r.slot.video_id.as_deref() == Some(video)
                && r.slot.page_id == page
                && r.slot.utc == utc
        }) {
            return Err("Slot không còn được giữ cho video này".into());
        }
        let r = self.read(id)?;
        let plan = r.plan();
        let rule = plan
            .pages
            .iter()
            .find(|p| p.page_id == page)
            .ok_or("Page ngoài kế hoạch")?;
        let zone = plan
            .timezone
            .parse::<chrono_tz::Tz>()
            .map_err(|_| "Múi giờ không hợp lệ")?;
        use chrono::TimeZone;
        let at = now();
        let current = zone.timestamp_opt(at, 0).single().ok_or("Time invalid")?;
        let scheduled = zone.timestamp_opt(utc, 0).single().ok_or("Time invalid")?;
        if scheduled.date_naive() != current.date_naive() || at < utc || at - utc > 300 {
            return Err(
                "Đã lỡ slot hoặc chưa đến giờ; phải phân lại slot hợp lệ, không đăng dồn".into(),
            );
        }
        let published = r
            .jobs
            .iter()
            .filter(|j| {
                j.page_id.as_deref() == Some(page)
                    && j.published_at.is_some_and(|time| {
                        zone.timestamp_opt(time, 0)
                            .single()
                            .is_some_and(|t| t.date_naive() == current.date_naive())
                    })
            })
            .count();
        if published >= rule.daily_limit as usize {
            return Err("Đã chạm hạn mức Page trong ngày".into());
        }
        Ok(())
    }
}
pub enum StageOutput {
    Content(Script),
    Media(Vec<Asset>),
    Render(Asset, u64),
    Quality(QualityReport),
    Published {
        post_id: String,
        post_url: String,
        published_at: i64,
    },
}
#[derive(Debug)]
pub struct TaskFailure {
    pub message: String,
    pub unknown: bool,
    pub needs_user: bool,
    pub repair_content: bool,
    pub render_seconds: u64,
}
impl TaskFailure {
    pub fn known(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            unknown: false,
            needs_user: false,
            repair_content: false,
            render_seconds: 0,
        }
    }
    pub fn input(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            unknown: false,
            needs_user: true,
            repair_content: false,
            render_seconds: 0,
        }
    }
    pub fn unknown(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            unknown: true,
            needs_user: true,
            repair_content: false,
            render_seconds: 0,
        }
    }
}
pub fn valid_post_url(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|u| {
        u.scheme() == "https"
            && matches!(
                u.host_str(),
                Some("facebook.com" | "www.facebook.com" | "m.facebook.com")
            )
            && u.username().is_empty()
            && u.password().is_none()
    })
}
