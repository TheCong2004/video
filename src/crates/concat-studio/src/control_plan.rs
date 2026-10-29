// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded model-plan compiler for the native Control runtime.
//! Only executable registry entries can become pending workflow steps.
use crate::ai_control::{Mode, Registry, Status, Step, Ticket, Workflow};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Requirement {
    pub description: String,
    pub tools: Vec<String>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlannedStep {
    pub id: String,
    pub tool: String,
    pub dependencies: Vec<String>,
    pub input: Value,
}
#[derive(Debug, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum Proposal {
    Plan {
        version: u32,
        summary: String,
        requirements: Vec<Requirement>,
        steps: Vec<PlannedStep>,
    },
    NeedsInput {
        question: String,
    },
    Unsupported {
        reason: String,
        missing: Vec<String>,
    },
}
pub struct CompiledPlan {
    pub summary: String,
    pub requirements: Vec<Requirement>,
    pub steps: Vec<Step>,
}
#[derive(Debug, PartialEq)]
pub enum PlanError {
    Invalid(String),
    NeedsInput(String),
    Unsupported {
        reason: String,
        missing: Vec<String>,
    },
}
fn invalid(message: &str) -> PlanError {
    PlanError::Invalid(message.into())
}
fn bounded(text: &str, max: usize) -> bool {
    !text.trim().is_empty() && text.len() <= max && !text.contains('\0')
}
fn is_reference(v: &Value, id: &str, pointer: &str) -> bool {
    v.as_object().is_some_and(|o| o.len() == 2) && v["$step"] == id && v["pointer"] == pointer
}
fn has_reference(v: &Value) -> bool {
    match v {
        Value::Object(o) => o.contains_key("$step") || o.values().any(has_reference),
        Value::Array(a) => a.iter().any(has_reference),
        _ => false,
    }
}
/// Parse the complete response, never a successful-looking fragment in prose.
pub fn compile(raw: &str, command: &str, registry: &Registry) -> Result<CompiledPlan, PlanError> {
    if raw.len() > 65536 {
        return Err(invalid("Model plan exceeds 64 KiB"));
    }
    let proposal: Proposal = serde_json::from_str(raw)
        .map_err(|_| invalid("AI đã trả lời nhưng kế hoạch thiếu trường bắt buộc hoặc sai định dạng. Chưa có thao tác nào từ kế hoạch này được chạy. Quay lại yêu cầu, bổ sung tên và thông tin sản phẩm nếu cần, rồi tạo yêu cầu mới."))?;
    let (version, summary, requirements, proposed) = match proposal {
        Proposal::NeedsInput { question } if bounded(&question, 2000) => {
            return Err(PlanError::NeedsInput(question));
        }
        Proposal::Unsupported { reason, missing }
            if bounded(&reason, 2000)
                && !missing.is_empty()
                && missing.len() <= 32
                && missing.iter().all(|s| bounded(s, 128)) =>
        {
            return Err(PlanError::Unsupported { reason, missing });
        }
        Proposal::Plan {
            version,
            summary,
            requirements,
            steps,
        } => (version, summary, requirements, steps),
        _ => return Err(invalid("Invalid blocked-plan response")),
    };
    if version != 1
        || !bounded(&summary, 2000)
        || requirements.is_empty()
        || requirements.len() > 32
        || proposed.is_empty()
        || proposed.len() > 16
    {
        return Err(invalid("Invalid model plan limits"));
    }
    let mut declared = BTreeSet::new();
    for requirement in &requirements {
        if !bounded(&requirement.description, 1000)
            || requirement.tools.is_empty()
            || requirement.tools.len() > 16
        {
            return Err(invalid("Each requirement must name executable tools"));
        }
        for name in &requirement.tools {
            registry.get(name).map_err(PlanError::Invalid)?;
            declared.insert(name.as_str());
        }
    }
    let used: BTreeSet<_> = proposed.iter().map(|s| s.tool.as_str()).collect();
    if declared != used {
        return Err(invalid("Requirements and planned tools disagree"));
    }
    for render in proposed.iter().filter(|s| s.tool == "factory.render") {
        if !proposed.iter().any(|s| {
            s.tool == "artifact.validate"
                && s.dependencies.contains(&render.id)
                && is_reference(&s.input["path"], &render.id, "/path")
        }) {
            return Err(invalid(
                "Every rendered artifact requires dependent validation of its actual path",
            ));
        }
    }
    let mut steps = Vec::new();
    for s in proposed {
        if s.tool == "ai.plan" || s.id.len() > 64 {
            return Err(invalid("Recursive planning or oversized local step ID"));
        }
        // Deferred values are validated again after dependency resolution by the executor boundary.
        if !has_reference(&s.input) {
            registry
                .validate(&s.tool, &s.input, false)
                .map_err(PlanError::Invalid)?;
        }
        steps.push(Step {
            id: s.id,
            tool: s.tool,
            dependencies: s.dependencies,
            input: s.input,
            status: Status::Pending,
            output: None,
            attempts: 0,
            error: None,
            approval_digest: None,
        });
    }
    // Reuse the durable core's tool/identifier/input/dependency/cycle validation.
    Workflow::new(command.into(), Mode::FullAuto, steps.clone(), registry)
        .map_err(PlanError::Invalid)?;
    Ok(CompiledPlan {
        summary,
        requirements,
        steps,
    })
}

/// Build a candidate settlement + expanded DAG. The caller persists it once,
/// before dispatching any child. Failure leaves the original running receipt intact.
pub fn expand(
    current: &Workflow,
    ticket: &Ticket,
    plan: CompiledPlan,
    registry: &Registry,
) -> Result<Workflow, PlanError> {
    if !matches!(ticket.tool.as_str(), "ai.plan" | "batch.expand") || ticket.step.len() > 32 || current.cancelled {
        return Err(invalid("Invalid planner dispatch"));
    }
    let mut next = current.clone();
    let proposal = serde_json::json!({
        "outcome":"plan", "version":1, "summary":plan.summary,
        "requirements":plan.requirements,
        "steps":plan.steps.iter().map(|s| serde_json::json!({
            "id":s.id,"tool":s.tool,"dependencies":s.dependencies,"input":s.input
        })).collect::<Vec<_>>()
    });
    next.settle(ticket,Ok(serde_json::json!({"proposal":proposal,"summary":plan.summary,"requirements":plan.requirements,"plannedSteps":plan.steps.len()}))).map_err(PlanError::Invalid)?;
    let names: std::collections::BTreeMap<_, _> = plan
        .steps
        .iter()
        .map(|s| (s.id.clone(), format!("{}--{}", ticket.step, s.id)))
        .collect();
    fn rename(
        value: &mut Value,
        names: &std::collections::BTreeMap<String, String>,
        depth: u32,
    ) -> Result<(), PlanError> {
        if depth > 32 {
            return Err(invalid("Plan reference nesting limit"));
        }
        match value {
            Value::Object(object) => {
                if let Some(id) = object.get("$step") {
                    let replacement = names
                        .get(
                            id.as_str()
                                .ok_or_else(|| invalid("Invalid step reference"))?,
                        )
                        .ok_or_else(|| invalid("Reference outside proposed plan"))?
                        .clone();
                    object.insert("$step".into(), Value::String(replacement));
                }
                for v in object.values_mut() {
                    rename(v, names, depth + 1)?;
                }
            }
            Value::Array(items) => {
                for v in items {
                    rename(v, names, depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    for mut step in plan.steps {
        step.id = names[&step.id].clone();
        step.dependencies = step
            .dependencies
            .iter()
            .map(|id| {
                names
                    .get(id)
                    .cloned()
                    .ok_or_else(|| invalid("Unknown proposed dependency"))
            })
            .collect::<Result<_, _>>()?;
        step.dependencies.push(ticket.step.clone());
        step.dependencies.sort();
        step.dependencies.dedup();
        rename(&mut step.input, &names, 0)?;
        next.steps.push(step);
    }
    // Validate a scratch graph; never reset the real old steps or their receipts.
    let mut shape = next.steps.clone();
    for s in &mut shape {
        s.status = Status::Pending;
        s.output = None;
        s.attempts = 0;
        s.error = None;
        s.approval_digest = None;
    }
    Workflow::new(next.command.clone(), next.mode, shape, registry).map_err(PlanError::Invalid)?;
    Ok(next)
}

/// Change a user-selected AI source only before any model dispatch. Invalidate
/// old approvals/descendants using the same revision-checked input patch path.
pub fn rebind_model(current: &Workflow, model: &str) -> Result<Workflow, String> {
    if !bounded(model, 500) {
        return Err("Chọn nguồn AI trước khi tiếp tục".into());
    }
    if current
        .steps
        .iter()
        .any(|s| s.tool.starts_with("ai.") && s.attempts > 0)
    {
        return Err("Tác vụ này đã gọi AI. Giữ nguồn AI cũ hoặc tạo yêu cầu mới để đổi nguồn; không tự gửi lại.".into());
    }
    let mut next = current.clone();
    let ids = next
        .steps
        .iter()
        .filter(|s| s.tool.starts_with("ai."))
        .map(|s| s.id.clone())
        .collect::<Vec<_>>();
    for id in ids {
        let step = next
            .steps
            .iter()
            .find(|s| s.id == id)
            .ok_or("Missing step")?;
        let mut input = step.input.clone();
        if !input.is_object() {
            return Err("Invalid AI step input".into());
        }
        input["model"] = Value::String(model.into());
        if step.tool == "ai.plan" && input["context"].is_object() {
            input["context"]["model"] = Value::String(model.into());
        }
        let revision = next.revision;
        next.patch_input(&id, input, revision)?;
    }
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_control::Capability;
    use serde_json::json;
    fn registry() -> Registry {
        let mut r = Registry::default();
        for name in [
            "library.list",
            "factory.render",
            "artifact.validate",
            "ai.plan",
        ] {
            r.register(Capability {
                name: name.into(),
                description: name.into(),
                module: "test".into(),
                input_schema: json!({"type":"object"}),
                output_schema: json!({"type":"object"}),
                scope: "test".into(),
                read_only: name != "factory.render",
                replay_safe: true,
                approval_required: false,
                timeout_seconds: 30,
            })
            .unwrap();
        }
        r
    }
    fn plan() -> Value {
        json!({"outcome":"plan","version":1,"summary":"Render supplied media and validate its actual output","requirements":[{"description":"Produce a checked artifact","tools":["factory.render","artifact.validate"]}],"steps":[{"id":"render","tool":"factory.render","dependencies":[],"input":{}},{"id":"validate","tool":"artifact.validate","dependencies":["render"],"input":{"path":{"$step":"render","pointer":"/path"}}}]})
    }
    #[test]
    fn valid_plan_only_creates_pending_unapproved_steps() {
        let p = compile(&plan().to_string(), "render supplied media", &registry()).unwrap();
        assert_eq!(p.steps.len(), 2);
        assert!(p.steps.iter().all(|s| s.status == Status::Pending
            && s.output.is_none()
            && s.approval_digest.is_none()));
    }
    #[test]
    fn blocked_intents_never_create_a_partial_successful_workflow() {
        assert!(matches!(
            compile(
                r#"{"outcome":"unsupported","reason":"Downloader is not connected","missing":["media.download"]}"#,
                "download videos",
                &registry()
            ),
            Err(PlanError::Unsupported { .. })
        ));
        assert!(matches!(
            compile(
                r#"{"outcome":"needs_input","question":"Which product should the video describe?"}"#,
                "create product video",
                &registry()
            ),
            Err(PlanError::NeedsInput(_))
        ));
    }
    #[test]
    fn malformed_unsupported_response_is_not_misreported_as_missing_capability() {
        let response = r#"{"outcome":"unsupported","reason":"Product details are absent"}"#;
        let Err(PlanError::Invalid(message)) = compile(response, "write product introduction", &registry()) else {
            panic!("Malformed model response must not create steps or invent missing capabilities");
        };
        assert!(message.contains("thiếu trường bắt buộc"));
        assert!(message.contains("Quay lại yêu cầu"));
        assert!(matches!(compile(r#"{"outcome":"needs_input","question":"Bạn muốn giới thiệu sản phẩm nào?"}"#, "write product introduction", &registry()), Err(PlanError::NeedsInput(_))));
    }
    #[test]
    fn model_cannot_inject_success_approval_or_unknown_tools() {
        for key in ["status", "output", "approvalDigest"] {
            let mut p = plan();
            p["steps"][0][key] = json!("done");
            assert!(compile(&p.to_string(), "render", &registry()).is_err());
        }
        let mut p = plan();
        p["steps"][0]["tool"] = json!("shell.execute");
        assert!(compile(&p.to_string(), "render", &registry()).is_err());
    }
    #[test]
    fn validation_cannot_substitute_a_different_file_or_missing_dependency() {
        for input in [
            json!({"path":"old.mp4"}),
            json!({"path":{"$step":"render","pointer":"/other"}}),
        ] {
            let mut p = plan();
            p["steps"][1]["input"] = input;
            assert!(compile(&p.to_string(), "render", &registry()).is_err());
        }
        let mut p = plan();
        p["steps"][1]["dependencies"] = json!([]);
        assert!(compile(&p.to_string(), "render", &registry()).is_err());
    }
    #[test]
    fn incomplete_declared_requirements_and_cycles_are_rejected() {
        let mut p = plan();
        p["requirements"][0]["tools"] =
            json!(["factory.render", "artifact.validate", "library.list"]);
        assert!(compile(&p.to_string(), "render and library", &registry()).is_err());
        let mut p = plan();
        p["steps"][0]["dependencies"] = json!(["validate"]);
        assert!(compile(&p.to_string(), "render", &registry()).is_err());
    }
    #[test]
    fn expansion_is_transactional_and_keeps_children_pending_under_the_same_workflow() {
        let r = registry();
        let raw = plan().to_string();
        let compiled = compile(&raw, "render", &r).unwrap();
        let step = Step {
            id: "plan".into(),
            tool: "ai.plan".into(),
            dependencies: vec![],
            input: json!({}),
            status: Status::Pending,
            output: None,
            attempts: 0,
            error: None,
            approval_digest: None,
        };
        let mut w = Workflow::new("render".into(), Mode::FullAuto, vec![step], &r).unwrap();
        let root = std::env::temp_dir().join(format!("concat-plan-expand-{}", w.id));
        let store = crate::ai_control::Store::open(&root).unwrap();
        let t = store
            .claim(&mut w, &r, &BTreeSet::from(["test".into()]))
            .unwrap()
            .unwrap();
        let next = expand(&w, &t, compiled, &r).unwrap();
        assert_eq!(w.steps[0].status, Status::Running);
        assert_eq!(next.id, w.id);
        assert_eq!(next.steps[0].status, Status::Done);
        let receipt = next.steps[0].output.as_ref().unwrap();
        assert!(compile(&receipt["proposal"].to_string(), "render", &r).is_ok());
        assert!(!next.finished());
        assert_eq!(next.steps[2].input["path"]["$step"], "plan--render");
        assert!(next.steps[2].dependencies.contains(&"plan".into()));
        store.save(&next).unwrap();
        drop(store);
        let store = crate::ai_control::Store::open(&root).unwrap();
        let loaded = store.load(&next.id).unwrap();
        assert_eq!(loaded.steps.len(), 3);
        assert_eq!(loaded.steps[1].status, Status::Pending);
        assert!(expand(&next, &t, compile(&raw, "render", &r).unwrap(), &r).is_err());
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn source_change_before_dispatch_invalidates_approval_and_is_transactional() {
        let step = Step {
            id: "plan".into(),
            tool: "ai.plan".into(),
            dependencies: vec![],
            input: json!({"model":"api-model","context":{"model":"api-model"}}),
            status: Status::Pending,
            output: None,
            attempts: 0,
            error: None,
            approval_digest: None,
        };
        let mut w = Workflow::new(
            "List library".into(),
            Mode::FullAuto,
            vec![step],
            &registry(),
        )
        .unwrap();
        w.steps[0].status = Status::WaitingApproval;
        let approved = crate::ai_control::digest(&w.review_input("plan").unwrap());
        w.approve("plan", &approved).unwrap();
        let changed = rebind_model(&w, "web:chatgpt").unwrap();
        assert_eq!(w.steps[0].input["model"], "api-model");
        assert_eq!(changed.steps[0].input["model"], "web:chatgpt");
        assert_eq!(changed.steps[0].input["context"]["model"], "web:chatgpt");
        assert!(changed.steps[0].approval_digest.is_none());
        let mut attempted = changed.clone();
        attempted.steps[0].attempts = 1;
        attempted.steps[0].status = Status::Unknown;
        assert!(rebind_model(&attempted, "web:gemini").is_err());
        assert_eq!(attempted.steps[0].input["model"], "web:chatgpt");
    }
}
