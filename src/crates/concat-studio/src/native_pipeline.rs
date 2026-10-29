//! Reserved seven-stage production executor. Promax alone advances stages.
use crate::{
    browser::BrowserClient,
    production::{ProductionClient, ProductionPlan, ProductionStage},
    production_worker::{ProductionOwner, ProductionReservation, WorkflowBinding},
    promax_worker::{ProductionHandler, WorkerIdentity},
    storyboard::Storyboard,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

pub const WORKFLOW: &str = "concat.native_production.v1";
const GRAPH: &str = include_str!("../../../../docs/contracts/native-production-v1.workflow.json");
pub fn definition() -> Value {
    serde_json::from_str(GRAPH).unwrap_or(Value::Null)
}
pub fn digest(graph: &Value) -> Result<String, String> {
    fn integers(v: &Value) -> bool {
        match v {
            Value::Number(n) => {
                n.as_i64()
                    .is_some_and(|n| n.unsigned_abs() <= 9_007_199_254_740_991)
                    || n.as_u64().is_some_and(|n| n <= 9_007_199_254_740_991)
            }
            Value::Array(a) => a.iter().all(integers),
            Value::Object(o) => o.values().all(integers),
            _ => true,
        }
    }
    if !integers(graph) {
        return Err("WORKFLOW_GRAPH_INVALID".into());
    }
    fn canonical(v: &Value) -> Result<String, String> {
        Ok(match v {
            Value::Array(items) => format!(
                "[{}]",
                items
                    .iter()
                    .map(canonical)
                    .collect::<Result<Vec<_>, _>>()?
                    .join(",")
            ),
            Value::Object(items) => {
                let mut keys = items.keys().collect::<Vec<_>>();
                keys.sort();
                format!(
                    "{{{}}}",
                    keys.into_iter()
                        .map(|k| Ok(format!(
                            "{}:{}",
                            serde_json::to_string(k).map_err(|_| "WORKFLOW_GRAPH_INVALID")?,
                            canonical(&items[k])?
                        )))
                        .collect::<Result<Vec<_>, String>>()?
                        .join(",")
                )
            }
            _ => serde_json::to_string(v).map_err(|_| "WORKFLOW_GRAPH_INVALID")?,
        })
    }
    Ok(format!(
        "{:x}",
        Sha256::digest(canonical(graph)?.as_bytes())
    ))
}

/// Native dependencies are injected, not reimplemented or assumed available.
pub trait Services: Send + Sync + 'static {
    fn sources(&self) -> Result<(), String>;
    fn script(&self) -> Result<Storyboard, String>;
    fn voice(&self, board: &mut Storyboard, root: &Path, cancel: &AtomicBool)
    -> Result<(), String>;
    fn render(
        &self,
        board: &Storyboard,
        root: &Path,
        cancel: &AtomicBool,
    ) -> Result<PathBuf, String>;
    fn verify_video(&self, path: &Path) -> Result<(), String>;
    fn progress(&self, stage: usize, scene: usize, total: usize);
    fn completed(&self, board: Storyboard, video: PathBuf);
    fn failed(&self, message: &str);
}
struct State {
    next: usize,
    run: Option<String>,
    board: Option<Storyboard>,
    reservations: Vec<ProductionReservation>,
    terminal: bool,
}
/// One explicit user intent, one immutable graph/profile/source, one worker.
pub struct Pipeline {
    id: String,
    identity: WorkerIdentity,
    graph: Value,
    graph_digest: String,
    root: PathBuf,
    page: crate::pages::ContentPage,
    anchor: PathBuf,
    image: String,
    expand: String,
    video: String,
    client: ProductionClient,
    services: Arc<dyn Services>,
    state: Mutex<State>,
}
impl Pipeline {
    #[allow(clippy::too_many_arguments)]
    pub fn reserve(
        identity: WorkerIdentity,
        graph: Value,
        root: &Path,
        recovery_root: &Path,
        page: crate::pages::ContentPage,
        anchor: PathBuf,
        image: String,
        expand: String,
        video: String,
        browser: BrowserClient,
        services: Arc<dyn Services>,
    ) -> Result<Self, String> {
        // This bundled graph is independently registered; never accept a server
        // graph edited to introduce arbitrary node types or additional stages.
        if graph != definition() {
            return Err("NATIVE_WORKFLOW_REVISION_CHANGED".into());
        }
        let id = format!("pipeline-{:032x}", rand::random::<u128>());
        std::fs::create_dir_all(root).map_err(|_| "PIPELINE_ROOT_UNAVAILABLE")?;
        let root = root.join(&id);
        std::fs::create_dir(&root).map_err(|_| "PIPELINE_ROOT_UNAVAILABLE")?;
        let anchor_copy = root.join("anchor.bin");
        let original = std::fs::File::open(&anchor).map_err(|_| "SOURCE_IMAGE_MISSING")?;
        use std::io::Read;
        let mut bytes = vec![];
        original
            .take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "SOURCE_IMAGE_MISSING")?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err("SOURCE_IMAGE_TOO_LARGE".into());
        }
        std::fs::write(&anchor_copy, bytes).map_err(|_| "SOURCE_COPY_FAILED")?;
        let plan = ProductionPlan::new(page.clone(), anchor_copy.clone(), &image, &expand, &video)?;
        let client = ProductionClient::new(browser).with_recovery_root(recovery_root.to_owned());
        client.preflight(&plan)?;
        let graph_digest = digest(&graph)?;
        Ok(Self {
            id,
            identity,
            graph,
            graph_digest,
            root,
            page,
            anchor: anchor_copy,
            image,
            expand,
            video,
            client,
            services,
            state: Mutex::new(State {
                next: 0,
                run: None,
                board: None,
                reservations: vec![],
                terminal: false,
            }),
        })
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn target(&self) -> Value {
        json!({"clientId":self.identity.client_id,"deviceId":self.identity.device_id})
    }
    pub fn finished(&self) -> bool {
        self.state
            .try_lock()
            .map(|s| s.terminal || s.next == 7)
            .unwrap_or(false)
    }
    fn check(&self, command: &Value, state: &State) -> Result<(), String> {
        let p = &command["inputPayload"];
        let node = &self.graph["nodes"][state.next.min(6)];
        if state.terminal
            || state.next >= 7
            || command["workflowId"] != WORKFLOW
            || command["stepId"] != node["nodeId"]
            || command["stageId"] != node["nodeType"]
            || p["_workflowDigest"] != self.graph_digest
            || p["_workflowVersion"] != self.graph["version"]
            || p["production_settings"]["nativeReservationId"] != self.id
            || p["production_settings"]["workerTarget"] != self.target()
            || p["production_settings"]["browserProfileId"]
                != self.page.browser_profile_id.as_deref().unwrap_or("")
            || p["_tenantId"] != self.identity.tenant_id
            || p["_ownerId"] != self.identity.user_id
            || state
                .run
                .as_ref()
                .is_some_and(|run| command["runId"] != *run)
            || node["config"]["operation"] != p.get("operation").cloned().unwrap_or(Value::Null)
        {
            return Err("NATIVE_RESERVATION_MISMATCH".into());
        }
        Ok(())
    }
    fn step(&self, state: &mut State, cancel: Arc<AtomicBool>) -> Result<Value, String> {
        self.services.progress(state.next, 0, 1);
        match state.next {
            0 => self.services.sources()?,
            1 => {
                let board = self.services.script()?;
                board.validate()?;
                if board.shots.is_empty() || board.shots.len() > 30 {
                    return Err("SCRIPT_SCENE_COUNT_INVALID".into());
                }
                let owner = ProductionOwner {
                    tenant_id: self.identity.tenant_id.clone(),
                    user_id: self.identity.user_id.clone(),
                    device_id: self.identity.device_id.clone(),
                };
                let binding = WorkflowBinding {
                    workflow_id: WORKFLOW.into(),
                    version: "1.0.0".into(),
                };
                for shot in &board.shots {
                    let prompt = format!(
                        "{}\n{}\n{}",
                        self.image,
                        shot.narration,
                        shot.visual_prompt()
                    );
                    let video = format!("{}\n{}", self.video, shot.visual_prompt());
                    let plan = ProductionPlan::new(
                        self.page.clone(),
                        self.anchor.clone(),
                        &prompt,
                        &self.expand,
                        &video,
                    )?;
                    state.reservations.push(ProductionReservation::reserve(
                        &self.root,
                        owner.clone(),
                        binding.clone(),
                        plan,
                    )?);
                }
                state.board = Some(board);
            }
            2..=4 => {
                let stage = [
                    ProductionStage::ImageEdit,
                    ProductionStage::Expand916,
                    ProductionStage::VideoGenerate,
                ][state.next - 2];
                let total = state.reservations.len();
                for (i, reservation) in state.reservations.iter_mut().enumerate() {
                    if cancel.load(Ordering::Acquire) {
                        return Err("CANCELLED".into());
                    }
                    self.services.progress(state.next, i, total);
                    let command = reservation.next_command(stage)?;
                    reservation.execute(&self.client, &command, cancel.clone(), |_| {})?;
                    if stage == ProductionStage::VideoGenerate {
                        let artifact = reservation.final_artifact_for_probe()?;
                        self.services.verify_video(&artifact.path)?;
                        if let Some(shot) = state.board.as_mut().and_then(|b| b.shots.get_mut(i)) {
                            shot.generated = Some(artifact.path.clone());
                            shot.preview = None;
                        }
                    }
                }
            }
            5 => self.services.voice(
                state.board.as_mut().ok_or("SCRIPT_MISSING")?,
                &self.root,
                &cancel,
            )?,
            6 => {
                let board = state.board.as_ref().ok_or("SCRIPT_MISSING")?;
                let video = self.services.render(board, &self.root, &cancel)?;
                self.services.verify_video(&video)?;
                use std::io::Read;
                let mut file = std::fs::File::open(&video).map_err(|_| "FINAL_VIDEO_MISSING")?;
                let mut hash = Sha256::new();
                let mut bytes = 0u64;
                let mut chunk = [0u8; 65536];
                loop {
                    let read = file
                        .read(&mut chunk)
                        .map_err(|_| "FINAL_VIDEO_READ_FAILED")?;
                    if read == 0 {
                        break;
                    }
                    hash.update(&chunk[..read]);
                    bytes += read as u64;
                }
                let sha = format!("{:x}", hash.finalize());
                let mut board = board.clone();
                board.output = Some(video.clone());
                self.services.completed(board, video);
                return Ok(
                    json!({"output":{"resourceId":self.id,"storageScope":"local","sha256":sha,"mimeType":"video/mp4","bytes":bytes}}),
                );
            }
            _ => return Err("NATIVE_STAGE_INVALID".into()),
        }
        if let Some(board) = &state.board {
            std::fs::write(
                self.root.join("storyboard.json"),
                serde_json::to_vec_pretty(board).map_err(|_| "STATE_SAVE_FAILED")?,
            )
            .map_err(|_| "STATE_SAVE_FAILED")?;
        }
        Ok(
            json!({"output":{"resourceId":format!("{}-stage-{}",self.id,state.next+1),"storageScope":"local"}}),
        )
    }
}
impl ProductionHandler for Pipeline {
    fn capabilities(&self) -> Vec<String> {
        [
            "research",
            "story_script",
            "grok_image_edit",
            "grok_content_pipeline",
            "voice",
            "media_timeline",
        ]
        .iter()
        .map(|s| format!("artcraft.workflow.stage.{s}"))
        .collect()
    }
    fn validate(&self, command: &Value) -> Result<(), String> {
        let state = self.state.lock().map_err(|_| "PIPELINE_UNAVAILABLE")?;
        self.check(command, &state)
    }
    fn execute(&self, command: Value, cancel: Arc<AtomicBool>) -> Result<Value, String> {
        let mut state = self.state.lock().map_err(|_| "PIPELINE_UNAVAILABLE")?;
        self.check(&command, &state)?;
        let run = command["runId"].as_str().ok_or("RUN_ID_REQUIRED")?;
        state.run = Some(run.into());
        if cancel.load(Ordering::Acquire) {
            state.terminal = true;
            return Ok(json!({"status":"cancelled","confirmed":true}));
        }
        match self.step(&mut state, cancel) {
            Ok(receipt) => {
                state.next += 1;
                Ok(receipt)
            }
            Err(e) => {
                state.terminal = true;
                self.services.failed(&e);
                if e.contains("UNKNOWN") || e.contains("UNCONFIRMED") {
                    Err(e)
                } else if e == "CANCELLED" {
                    Ok(json!({"status":"cancelled","confirmed":true}))
                } else {
                    Ok(json!({"status":"failed","confirmed":true}))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn graph_is_seven_serial_non_retrying_real_stages() {
        let g = definition();
        assert_eq!(g["workflowId"], WORKFLOW);
        let nodes = g["nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 7);
        assert_eq!(g["edges"].as_array().unwrap().len(), 6);
        for n in nodes {
            assert_eq!(n["retryable"], false);
            assert_eq!(n["maxAttempts"], 1);
        }
        assert_eq!(nodes[3]["config"]["operation"], "expand_9_16");
        assert_eq!(nodes[4]["config"]["operation"], "video_generate");
        assert_eq!(digest(&g).unwrap().len(), 64);
        assert_eq!(
            digest(&g).unwrap(),
            "ec6a85ff214a72ed5832e7d4efcb2c512854629dc55a622f7dd41974a1687ebf",
            "must match Promax productionWorkflowDigest of the same contract"
        );
    }
    #[test]
    fn graph_digest_rejects_non_interoperable_numbers() {
        assert!(digest(&json!({"x":1.5})).is_err());
        assert!(digest(&json!({"x":9007199254740992u64})).is_err());
    }
}
