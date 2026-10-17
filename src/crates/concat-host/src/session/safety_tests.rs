// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat contributors
use super::*;
use std::time::Duration;

fn fresh(name:&str)->(Session,std::path::PathBuf) {
    let root=std::env::temp_dir().join(format!("concat-safety-{name}-{}",std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let info=projects::create(&root.to_string_lossy(),"Test",128,128,30,1).unwrap();
    (Session::open_info(&info).unwrap(),root)
}

#[test]
fn checkpoints_restore_native_history_without_reusing_ids_or_reviving_stale_drafts() {
    let (mut session,root)=fresh("checkpoint");
    let checkpoint=session.create_checkpoint("Before",EditActor::User).unwrap();
    let original=session.project().clone();
    let draft=session.prepare_batch(vec![Command::AddTrack{bottom:false}]).unwrap();
    let first=session.apply(Command::AddTrack{bottom:false}).unwrap().created_id.unwrap();
    let edited=session.project().clone();
    session.restore_named_checkpoint(checkpoint.id,EditActor::User).unwrap();
    assert_eq!(session.project(),&original);
    assert!(session.commit_draft(draft).is_err());
    assert_eq!(session.undo().project,edited);
    assert_eq!(session.redo().project,original);
    let next=session.apply(Command::AddTrack{bottom:false}).unwrap().created_id.unwrap();
    assert_ne!(next,first);
    session.delete_checkpoint(checkpoint.id).unwrap();
    assert!(session.checkpoints().is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn owned_session_lease_history_lock_disconnect_and_duplicate_commit_are_independent() {
    let (mut session,root)=fresh("lease");
    let work=session.begin_agent_work("one","Cut interview").unwrap();
    assert_eq!(session.begin_agent_work("one","Repeated").unwrap().id,work.id);
    assert!(session.begin_agent_work("two","Other job").is_err());
    // Thinking alone does not lock user edits; native revision makes old plans stale.
    session.apply(Command::AddTrack{bottom:false}).unwrap();
    session.lock_history("two","Reviewing").unwrap();
    session.end_agent_work("one").unwrap();
    assert!(session.try_undo().is_err(),"ending one must not release two's lock");
    session.disconnect_agent("two");
    assert!(session.try_undo().is_ok());
    session.begin_agent_work("one","Commit").unwrap();
    let lease=session.acquire_agent_write("one",Duration::from_secs(30)).unwrap();
    assert!(session.acquire_agent_write("two",Duration::from_secs(30)).is_err());
    let before=session.project().clone();
    assert!(session.apply(Command::AddTrack{bottom:false}).is_err());
    assert_eq!(session.project(),&before);
    let draft=session.prepare_batch(vec![Command::AddTrack{bottom:false}]).unwrap();
    session.commit_agent_draft(&lease,"op-one",draft).unwrap();
    let revision=session.revision();
    let second=session.prepare_batch(vec![Command::AddTrack{bottom:false}]).unwrap();
    assert!(session.commit_agent_draft(&lease,"op-one",second).is_err());
    assert_eq!(session.revision(),revision);
    assert!(session.activities().iter().any(|entry|entry.actor==EditActor::Agent{connection:"one".into()} && entry.operation=="approvedBatch"));
    session.disconnect_agent("two");
    assert!(session.apply(Command::AddTrack{bottom:false}).is_err());
    session.disconnect_agent("one");
    assert!(session.apply(Command::AddTrack{bottom:false}).is_ok());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn lease_from_another_opening_cannot_write_and_expired_lease_cannot_resume() {
    let (mut one,root)=fresh("foreign");
    let mut two=Session::open(one.path(),one.settings()).unwrap();
    one.begin_agent_work("same","First").unwrap();
    two.begin_agent_work("same","Second").unwrap();
    let lease=one.acquire_agent_write("same",Duration::from_secs(30)).unwrap();
    let draft=two.prepare_batch(vec![Command::AddTrack{bottom:false}]).unwrap();
    assert!(two.commit_agent_draft(&lease,"foreign",draft).is_err());
    assert!(one.acquire_agent_write("same",Duration::ZERO).is_err());
    one.expire_agent_write();
    let draft=one.prepare_batch(vec![Command::AddTrack{bottom:false}]).unwrap();
    assert!(one.commit_agent_draft(&lease,"expired",draft).is_err());
    std::fs::remove_dir_all(root).unwrap();
}
