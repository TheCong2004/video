// SPDX-License-Identifier: AGPL-3.0-or-later
//! IANA-zone scheduling: daily caps, unique slots, DST, Page/topic compatibility.
use super::*;
use chrono::{Duration, LocalResult, NaiveDate, NaiveTime, TimeZone};

pub struct Capacity {
    pub slots: Vec<PublishingSlot>,
    pub shortage: usize,
}
pub fn capacity(plan: &Plan, now: i64) -> Result<Capacity, String> {
    plan.validate()?;
    if !plan.publish {
        return Ok(Capacity {
            slots: vec![],
            shortage: 0,
        });
    }
    let zone = plan
        .timezone
        .parse::<chrono_tz::Tz>()
        .map_err(|_| "Invalid timezone")?;
    let mut date =
        NaiveDate::parse_from_str(&plan.start_date, "%Y-%m-%d").map_err(|_| "Invalid date")?;
    let end = NaiveDate::parse_from_str(&plan.end_date, "%Y-%m-%d").map_err(|_| "Invalid date")?;
    let mut slots = vec![];
    while date <= end {
        for page in &plan.pages {
            let mut page_slots = 0usize;
            let mut times = page.slots.clone();
            times.sort();
            let mut used = 0;
            for time in times {
                if used >= page.daily_limit {
                    break;
                }
                let local = date.and_time(
                    NaiveTime::parse_from_str(&time, "%H:%M").map_err(|_| "Invalid time")?,
                );
                // One local slot at an autumn transition, no invented spring hour.
                let utc = match zone.from_local_datetime(&local) {
                    LocalResult::Single(t) => t.timestamp(),
                    LocalResult::Ambiguous(a, b) => a.timestamp().min(b.timestamp()),
                    LocalResult::None => continue,
                };
                used += 1;
                if utc < now
                    || plan
                        .completion_deadline_utc
                        .is_some_and(|deadline| utc > deadline)
                {
                    continue;
                }
                slots.push(PublishingSlot {
                    page_id: page.page_id.clone(),
                    utc,
                    local_date: date.to_string(),
                    video_id: None,
                });
                page_slots += 1;
                if page_slots >= plan.video_count as usize { break; }
            }
        }
        date = date
            .checked_add_signed(Duration::days(1))
            .ok_or("Date overflow")?;
    }
    slots.sort_by(|a, b| (a.utc, &a.page_id).cmp(&(b.utc, &b.page_id)));
    let shortage = (plan.video_count as usize).saturating_sub(slots.len());
    Ok(Capacity { slots, shortage })
}
pub fn assign(record: &mut Record, now: i64) -> Result<(), String> {
    assign_reserved(record, now, &[])
}
pub fn assign_reserved(
    record: &mut Record,
    now: i64,
    reserved: &[PublishingSlot],
) -> Result<(), String> {
    let mut capacity = capacity(record.plan(), now)?;
    // Completed/dispatched jobs retain their past slots below. Capacity for a
    // resumed campaign is measured against remaining jobs, not its original total.
    // Preserve already dispatched/posted slots. They are never moved by a replan.
    for job in &record.jobs {
        if job.publish_dispatched || job.state == JobState::Published {
            if let (Some(page), Some(utc)) = (&job.page_id, job.slot_utc) {
                if !capacity
                    .slots
                    .iter()
                    .any(|s| &s.page_id == page && s.utc == utc)
                {
                    let old = record
                        .slots
                        .iter()
                        .find(|s| &s.page_id == page && s.utc == utc)
                        .ok_or("Không thể thay đổi slot đã gửi đăng")?;
                    capacity.slots.push(old.clone());
                    capacity.slots.last_mut().unwrap().video_id = None;
                }
                let slot = capacity
                    .slots
                    .iter_mut()
                    .find(|s| &s.page_id == page && s.utc == utc)
                    .ok_or("Không thể thay đổi slot đã gửi đăng")?;
                if slot.video_id.is_some() {
                    return Err("Slot đã bị chiếm".into());
                }
                slot.video_id = Some(job.id.clone());
            }
        }
    }
    let mut other_days = std::collections::BTreeMap::<(String, String), usize>::new();
    for slot in reserved {
        *other_days
            .entry((slot.page_id.clone(), slot.local_date.clone()))
            .or_default() += 1;
    }
    let mut admitted = std::collections::BTreeMap::<(String, String), usize>::new();
    // Immutable slots count before free slots, regardless of chronological order.
    for slot in capacity.slots.iter().filter(|s| s.video_id.is_some()) {
        *admitted
            .entry((slot.page_id.clone(), slot.local_date.clone()))
            .or_default() += 1;
    }
    capacity.slots.retain(|slot| {
        if slot.video_id.is_some() {
            return true;
        }
        if reserved
            .iter()
            .any(|s| s.page_id == slot.page_id && s.utc == slot.utc)
        {
            return false;
        }
        let key = (slot.page_id.clone(), slot.local_date.clone());
        let limit = record
            .plan()
            .pages
            .iter()
            .find(|p| p.page_id == slot.page_id)
            .map(|p| p.daily_limit as usize)
            .unwrap_or(0);
        let used = admitted.entry(key.clone()).or_default();
        let allowed = *used + other_days.get(&key).copied().unwrap_or(0) < limit;
        if allowed {
            *used += 1;
        }
        allowed
    });
    let pages = record.plan().pages.clone();
    let free: Vec<Vec<usize>> = pages
        .iter()
        .map(|page| {
            capacity
                .slots
                .iter()
                .enumerate()
                .filter(|(_, s)| s.page_id == page.page_id && s.video_id.is_none())
                .map(|(i, _)| i)
                .collect()
        })
        .collect();
    let eligible: Vec<usize> = record
        .jobs
        .iter()
        .enumerate()
        .filter(|(_, j)| !j.publish_dispatched && j.state != JobState::Published)
        .map(|(i, _)| i)
        .collect();
    let edges: Vec<Vec<usize>> = record
        .jobs
        .iter()
        .map(|job| {
            let topic = job
                .script
                .as_ref()
                .map(|s| s.topic.to_lowercase())
                .unwrap_or_else(|| record.plan().topic.to_lowercase());
            pages
                .iter()
                .enumerate()
                .filter(|(_, page)| {
                    page.topics.is_empty()
                        || page
                            .topics
                            .iter()
                            .any(|t| topic.contains(&t.to_lowercase()))
                })
                .map(|(i, _)| i)
                .collect()
        })
        .collect();
    fn place(
        video: usize,
        edges: &[Vec<usize>],
        free: &[Vec<usize>],
        assigned: &mut [Vec<usize>],
        visited: &mut std::collections::BTreeSet<usize>,
    ) -> bool {
        for &page in &edges[video] {
            if !visited.insert(page) {
                continue;
            }
            if assigned[page].len() < free[page].len() {
                assigned[page].push(video);
                return true;
            }
            let old = assigned[page].clone();
            for (position, other) in old.into_iter().enumerate() {
                if place(other, edges, free, assigned, visited) {
                    assigned[page][position] = video;
                    return true;
                }
            }
        }
        false
    }
    let mut assigned = vec![vec![]; pages.len()];
    let mut missing = 0;
    for video in eligible {
        if !place(
            video,
            &edges,
            &free,
            &mut assigned,
            &mut std::collections::BTreeSet::new(),
        ) {
            missing += 1;
        }
    }
    if missing > 0 {
        return Err(format!("Thiếu {missing} slot phù hợp chủ đề/hạn mức hoặc slot đã được chiến dịch khác giữ. Không tự kéo dài thời hạn."));
    }
    for (page, videos) in assigned.iter_mut().enumerate() {
        videos.sort_unstable();
        for (video, slot) in videos.iter().zip(free[page].iter()) {
            let slot = &mut capacity.slots[*slot];
            let job = &mut record.jobs[*video];
            slot.video_id = Some(job.id.clone());
            job.page_id = Some(slot.page_id.clone());
            job.slot_utc = Some(slot.utc);
        }
    }
    record.slots = capacity
        .slots
        .into_iter()
        .filter(|s| s.video_id.is_some())
        .collect();
    Ok(())
}
