//! Expand a bounded video batch into ordinary durable Control steps.
//! This is a compiler, not another queue or service owner.
use crate::{
    ai_control::{Mode, Registry, Status, Step, Workflow},
    control_plan::{CompiledPlan, PlanError, Requirement},
};
use serde::Deserialize;
use serde_json::{Value, json};

/// Explicit per-video instructions; no product facts or paths are synthesized.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VideoJob {
    pub brief: String,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub profile_id: String,
    #[serde(default)]
    pub anchor: String,
    #[serde(default)]
    pub image_prompt: String,
    #[serde(default)]
    pub expand_prompt: String,
    #[serde(default)]
    pub video_prompt: String,
    #[serde(default)]
    pub script: String,
    #[serde(default)]
    pub audio_path: String,
    pub target_duration: f64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Batch {
    jobs: Vec<VideoJob>,
    model: String,
    #[serde(default)]
    same_story: bool,
}
fn invalid(message: &str) -> PlanError {
    PlanError::Invalid(message.into())
}
/// Enforce an explicit numeric video count, independently of model claims.
pub fn compile_for_command(
    input: &Value,
    command: &str,
    registry: &Registry,
) -> Result<CompiledPlan, PlanError> {
    let lower = command.to_lowercase();
    for (offset, _) in lower.match_indices("video") {
        let before = lower[..offset].trim_end();
        let number = before
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        if let Ok(expected) = number.parse::<usize>() {
            if expected == 0
                || expected > 100
                || input["jobs"]
                    .as_array()
                    .is_none_or(|jobs| jobs.len() != expected)
            {
                return Err(invalid(
                    "Số video trong kế hoạch không khớp số lượng bạn yêu cầu (giới hạn 100 video mỗi lô)",
                ));
            }
        }
    }
    compile(input, registry)
}
fn step(id: String, tool: &str, deps: Vec<String>, input: Value) -> Step {
    Step {
        id,
        tool: tool.into(),
        dependencies: deps,
        input,
        status: Status::Pending,
        output: None,
        attempts: 0,
        error: None,
        approval_digest: None,
    }
}
/// 1–100 independently described videos, expanded before any child dispatch.
/// Each generated asset passes through the existing editor and artifact gate.
pub fn compile(input: &Value, registry: &Registry) -> Result<CompiledPlan, PlanError> {
    let batch: Batch = serde_json::from_value(input.clone())
        .map_err(|_| invalid("Danh sách video không đúng định dạng"))?;
    if batch.jobs.is_empty()
        || batch.jobs.len() > 100
        || batch.model.trim().is_empty()
        || batch.model.len() > 500
    {
        return Err(invalid("Cần 1–100 video và một nguồn AI đã cấu hình"));
    }
    let mut briefs = std::collections::BTreeSet::new();
    let mut scripts = std::collections::BTreeSet::new();
    let mut steps = Vec::new();
    for (index, job) in batch.jobs.iter().enumerate() {
        if job.brief.trim().is_empty()
            || job.brief.len() > 4000
            || (!batch.same_story
                && !briefs.insert(
                    job.brief
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .to_lowercase(),
                ))
        {
            return Err(invalid(
                "Mỗi video cần nội dung riêng. Nếu làm phim nhiều cảnh, hãy mô tả riêng từng cảnh trong cùng câu chuyện.",
            ));
        }
        if !batch.same_story
            && !job.script.trim().is_empty()
            && !scripts.insert(
                job.script
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_lowercase(),
            )
        {
            return Err(invalid(
                "Các video độc lập không được dùng lại cùng một lời đọc",
            ));
        }
        if !job.target_duration.is_finite()
            || !(1.0..=600.0).contains(&job.target_duration)
            || job.script.len() > 12000
            || job.paths.len() > 100
        {
            return Err(invalid("Thời lượng hoặc tư liệu của video vượt giới hạn"));
        }
        let prefix = format!("v{:03}", index + 1);
        let mut render_deps = Vec::new();
        let script = if job.script.trim().is_empty() {
            let id = format!("{prefix}-script");
            steps.push(step(id.clone(),"ai.script",vec![],json!({"command":format!("{}\nViết lời đọc khoảng {} giây. Không lặp nội dung của video khác.",job.brief,job.target_duration),"model":batch.model})));
            render_deps.push(id.clone());
            json!({"$step":id,"pointer":"/text"})
        } else {
            json!(job.script)
        };
        let paths = if !job.anchor.is_empty() {
            if !job.paths.is_empty()
                || job.profile_id.is_empty()
                || job.image_prompt.trim().is_empty()
                || job.video_prompt.trim().is_empty()
            {
                return Err(invalid(
                    "Video qua Grok cần hồ sơ Nexora, ảnh nguồn và prompt ảnh/chuyển động; không trộn với danh sách tư liệu có sẵn",
                ));
            }
            let id = format!("{prefix}-generate");
            let input = json!({"profile_id":job.profile_id,"anchor":job.anchor,"image_prompt":job.image_prompt,"expand_prompt":job.expand_prompt,"video_prompt":job.video_prompt});
            registry
                .validate("production.generate", &input, false)
                .map_err(PlanError::Invalid)?;
            steps.push(step(id.clone(), "production.generate", vec![], input));
            render_deps.push(id.clone());
            json!([{"$step":id,"pointer":"/path"}])
        } else {
            if job.paths.is_empty() || !job.profile_id.is_empty() {
                return Err(invalid(
                    "Video cần ảnh nguồn Grok hoặc tư liệu thật đã đính kèm",
                ));
            }
            json!(job.paths)
        };
        let render = format!("{prefix}-render");
        steps.push(step(render.clone(),"factory.render",render_deps,json!({"paths":paths,"script":script,"seconds":0.5,"audio_path":job.audio_path,"target_duration":job.target_duration})));
        steps.push(step(
            format!("{prefix}-validate"),
            "artifact.validate",
            vec![render.clone()],
            json!({"path":{"$step":render,"pointer":"/path"}}),
        ));
    }
    Workflow::new(
        "Video batch".into(),
        Mode::FullAuto,
        steps.clone(),
        registry,
    )
    .map_err(PlanError::Invalid)?;
    let tools = steps
        .iter()
        .map(|s| s.tool.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok(CompiledPlan {
        summary: format!("Sản xuất và kiểm tra {} video", batch.jobs.len()),
        requirements: vec![Requirement {
            description: "Nội dung riêng → Grok hoặc tư liệu → edit, lời đọc, phụ đề → kiểm tra"
                .into(),
            tools,
        }],
        steps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_control::{Capability, Store};
    fn registry() -> Registry {
        let mut r = Registry::default();
        for name in [
            "batch.expand",
            "ai.script",
            "production.generate",
            "factory.render",
            "artifact.validate",
        ] {
            r.register(Capability {
                name: name.into(),
                description: name.into(),
                module: "test".into(),
                scope: "test".into(),
                read_only: name == "artifact.validate",
                replay_safe: matches!(
                    name,
                    "batch.expand" | "artifact.validate" | "factory.render"
                ),
                approval_required: false,
                input_schema: json!({"type":"object"}),
                output_schema: json!({"type":"object"}),
                timeout_seconds: 30,
            })
            .unwrap();
        }
        r
    }
    fn input(count: usize) -> Value {
        json!({"model":"test-model","jobs":(0..count).map(|i|json!({"brief":format!("Distinct video {i}"),"target_duration":10,"profile_id":"real-profile-from-fixture","anchor":"fixture.png","image_prompt":format!("Frame {i}"),"video_prompt":format!("Scene {i}")})).collect::<Vec<_>>()})
    }
    #[test]
    fn hundred_jobs_expand_transactionally_into_four_hundred_durable_steps() {
        let registry = registry();
        let request = input(100);
        let plan = compile(&request, &registry).unwrap();
        assert_eq!(plan.steps.len(), 400);
        let root =
            std::env::temp_dir().join(format!("control-batch-{:032x}", rand::random::<u128>()));
        let store = Store::open(&root).unwrap();
        let mut workflow = Workflow::new(
            "100 videos".into(),
            Mode::FullAuto,
            vec![step("batch".into(), "batch.expand", vec![], request)],
            &registry,
        )
        .unwrap();
        let scopes = std::collections::BTreeSet::from(["test".into()]);
        let ticket = store
            .claim(&mut workflow, &registry, &scopes)
            .unwrap()
            .unwrap();
        workflow = crate::control_plan::expand(&workflow, &ticket, plan, &registry).unwrap();
        store.save(&workflow).unwrap();
        assert_eq!(store.load(&workflow.id).unwrap().steps.len(), 401);
        let mut executions = std::collections::BTreeMap::<String, usize>::new();
        while let Some(ticket) = store.claim(&mut workflow, &registry, &scopes).unwrap() {
            *executions.entry(ticket.tool.clone()).or_default() += 1;
            if ticket.tool == "factory.render" {
                assert!(ticket.input["paths"][0].is_string());
                assert!(ticket.input["script"].is_string());
            }
            workflow.settle(&ticket,Ok(json!({"text":format!("Narration {}",ticket.step),"path":format!("{}.mp4",ticket.step)}))).unwrap();
            store.save(&workflow).unwrap();
        }
        assert!(workflow.finished());
        assert_eq!(executions.values().sum::<usize>(), 400);
        assert!(executions.values().all(|count| *count == 100));
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn rejects_empty_oversized_duplicate_and_incomplete_batches() {
        let r = registry();
        assert!(compile(&input(0), &r).is_err());
        assert!(compile(&input(101), &r).is_err());
        let mut request = input(2);
        request["jobs"][1] = request["jobs"][0].clone();
        assert!(compile(&request, &r).is_err());
        request["same_story"] = json!(true);
        assert!(compile(&request, &r).is_ok());
        request["jobs"][0]["video_prompt"] = json!("");
        assert!(compile(&request, &r).is_err());
    }
    #[test]
    fn existing_assets_skip_grok_and_preserve_supplied_script() {
        let request = json!({"model":"test","jobs":[{"brief":"Existing footage edit","paths":["user-attached.mp4"],"script":"User supplied narration","target_duration":10}]});
        let plan = compile(&request, &registry()).unwrap();
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].input["script"], "User supplied narration");
        assert_eq!(plan.steps[1].tool, "artifact.validate");
    }
    #[test]
    fn model_cannot_silently_reduce_requested_video_count() {
        assert!(compile_for_command(&input(2), "Tạo 100 video", &registry()).is_err());
        assert!(compile_for_command(&input(2), "Tạo 2 video, mỗi video 10s", &registry()).is_ok());
        assert!(compile_for_command(&input(2), "Tạo 2video", &registry()).is_ok());
    }
}
