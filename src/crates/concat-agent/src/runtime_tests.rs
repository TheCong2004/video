// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat contributors
use crate::runtime::*;
use concat_project::{Command, Editor};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

struct Fixture {
    replies: VecDeque<Value>,
    calls: usize,
    cancel: Option<Arc<AtomicBool>>,
}
impl TurnProvider for Fixture {
    fn turn(
        &mut self,
        messages: &[Value],
        _: &[Value],
        _: u32,
        _: Duration,
    ) -> Result<Value, RunError> {
        self.calls += 1;
        assert!(
            !serde_json::to_string(messages)
                .unwrap()
                .contains("fixture-private")
        );
        if let Some(cancel) = &self.cancel {
            cancel.store(true, Ordering::Release);
        }
        self.replies.pop_front().ok_or(RunError::UnknownOutcome)
    }
}
fn reply(id: &str, name: &str, args: Value) -> Value {
    json!({"model":"fixture","finishReason":"tool_calls","content":"","usage":{"totalTokens":20},
        "toolCalls":[{"id":id,"type":"function","function":{"name":name,"arguments":args.to_string()}}]})
}
#[test]
fn multi_turn_reads_activates_tools_and_proposes_without_live_mutation() {
    let mut editor = Editor::new();
    editor
        .apply(Command::AddTextClip {
            track_id: None,
            above: true,
            start: 0.0,
            style: None,
            duration: Some(10.0),
            offset_y: None,
        })
        .unwrap();
    let before = editor.project().clone();
    let input = json!({"version":1,"units":"seconds","timelineId":before.active_timeline_id,"baseRevision":editor.revision(),
        "operations":[{"op":"title","text":"Xin chào","start":0,"duration":2,"as":"title"}]});
    let mut provider = Fixture {
        replies: VecDeque::from(vec![
            reply("read", "read_project", json!({})),
            reply(
                "activate",
                "activate_tools",
                json!({"names":["read_clips","propose_edit"]}),
            ),
            reply("clips", "read_clips", json!({})),
            reply("proposal", "propose_edit", json!({"plan":input})),
        ]),
        calls: 0,
        cancel: None,
    };
    let result = run(
        &editor.planning_snapshot(),
        &[],
        "Thêm tiêu đề",
        RunLimits::default(),
        &AtomicBool::new(false),
        &mut provider,
    )
    .unwrap();
    assert_eq!(provider.calls, 4);
    assert_eq!(result.tokens, 80);
    assert!(
        result
            .activity
            .iter()
            .any(|line| line.contains("read_clips"))
    );
    assert_eq!(editor.project(), &before);
    editor.commit_draft(result.proposal.into_draft()).unwrap();
    editor.undo();
    assert_eq!(editor.project(), &before);
}
#[test]
fn cancel_unknown_outcome_and_unreported_usage_never_repeat_dispatch() {
    let editor = Editor::new();
    let cancel = Arc::new(AtomicBool::new(false));
    let mut provider = Fixture {
        replies: VecDeque::from(vec![reply("read", "read_project", json!({}))]),
        calls: 0,
        cancel: Some(cancel.clone()),
    };
    assert!(matches!(
        run(
            &editor.planning_snapshot(),
            &[],
            "test",
            RunLimits::default(),
            &cancel,
            &mut provider
        ),
        Err(RunError::Cancelled)
    ));
    assert_eq!(provider.calls, 1);
    let mut provider = Fixture {
        replies: VecDeque::new(),
        calls: 0,
        cancel: None,
    };
    assert!(matches!(
        run(
            &editor.planning_snapshot(),
            &[],
            "test",
            RunLimits::default(),
            &AtomicBool::new(false),
            &mut provider
        ),
        Err(RunError::UnknownOutcome)
    ));
    assert_eq!(provider.calls, 1);
    let mut unreported = reply("read", "read_project", json!({}));
    unreported.as_object_mut().unwrap().remove("usage");
    let mut provider = Fixture {
        replies: VecDeque::from(vec![unreported]),
        calls: 0,
        cancel: None,
    };
    assert!(matches!(
        run(
            &editor.planning_snapshot(),
            &[],
            "test",
            RunLimits::default(),
            &AtomicBool::new(false),
            &mut provider
        ),
        Err(RunError::UsageUnknown)
    ));
    assert_eq!(provider.calls, 1);
}
#[test]
fn unknown_tools_and_proposal_before_read_cannot_mutate() {
    let editor = Editor::new();
    let mut provider = Fixture {
        replies: VecDeque::from(vec![
            reply("mutate", "commit", json!({})),
            json!({"model":"fixture","finishReason":"stop","content":"done","usage":{"totalTokens":20}}),
        ]),
        calls: 0,
        cancel: None,
    };
    assert!(
        run(
            &editor.planning_snapshot(),
            &[],
            "test",
            RunLimits::default(),
            &AtomicBool::new(false),
            &mut provider
        )
        .is_err()
    );
    assert!(!editor.can_undo());
}

#[test]
fn run_ledger_records_turns_tokens_and_roundtrips() {
    let mut ledger = RunLedger::new("Test prompt");
    assert_eq!(ledger.status, "running");
    ledger.finish_success(120, 3, vec!["read_project".into(), "propose_edit".into()]);
    assert_eq!(ledger.status, "success");
    assert_eq!(ledger.tokens, 120);
    assert_eq!(ledger.turns, 3);
    assert_eq!(ledger.activity.len(), 2);
    let json = ledger.to_json().unwrap();
    let reloaded = RunLedger::from_json(&json).unwrap();
    assert_eq!(reloaded, ledger);

    let mut failed = RunLedger::new("Cancel test");
    failed.finish_error(&RunError::Cancelled, 40, 1, vec!["read_project".into()]);
    assert_eq!(failed.status, "cancelled");
    assert!(failed.error.is_some());
}
