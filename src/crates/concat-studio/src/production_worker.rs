// SPDX-License-Identifier: AGPL-3.0-or-later
//! Local, immutable reservations for central Grok commands. The central service
//! supplies opaque handles and correlations, never filesystem paths or prompts.
//! Command IDs, run IDs, fences and ACK delivery belong to the supervisor ledger;
//! this layer independently enforces owner, workflow, stage order and byte identity.
use crate::production::{
    ProductionClient, ProductionOutput, ProductionPlan, ProductionProgress, ProductionStage,
    ProductionStageOutput, hash, new_id, read_bounded, write_new,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

/// Authenticated identity captured from the local authenticated worker session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(missing_docs)]
pub struct ProductionOwner {
    pub tenant_id: String,
    pub user_id: String,
    pub device_id: String,
}
/// Exact immutable central graph revision authorized when the user reserves it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(missing_docs)]
pub struct WorkflowBinding {
    pub workflow_id: String,
    pub version: String,
}
/// No server-provided path, prompt, destination, profile or model is accepted.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(missing_docs)]
pub struct ProductionCommand {
    pub reservation_id: String,
    pub owner: ProductionOwner,
    pub workflow: WorkflowBinding,
    pub stage: ProductionStage,
    pub input_artifact_handle: String,
    pub expected_sha256: String,
}
/// Exact existing central node type with a single, strict operation selector.
#[derive(Clone, Debug, Serialize)]
#[allow(missing_docs)]
pub struct ProductionNode {
    pub id: String,
    pub node_type: String,
    pub operation: String,
    pub stage: ProductionStage,
    pub previous_node: Option<String>,
}
/// Safe ACK payload: opaque local handle and digest, never local paths/tokens.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[allow(missing_docs)]
pub struct WorkerStageReceipt {
    pub reservation_id: String,
    pub stage: ProductionStage,
    pub artifact_handle: String,
    pub sha256: String,
    pub mime_type: String,
    pub native_probe_required: bool,
}

/// A reservation cannot be deserialized from server data or recreated by ID.
/// A crash or duplicate stage leaves create-new receipts requiring reconciliation.
pub struct ProductionReservation {
    id: String,
    owner: ProductionOwner,
    workflow: WorkflowBinding,
    plan: ProductionPlan,
    root: PathBuf,
    source_handle: String,
    outputs: Vec<(WorkerStageReceipt, ProductionStageOutput)>,
    terminal_failure: bool,
}
const STAGES: [ProductionStage; 3] = [
    ProductionStage::ImageEdit,
    ProductionStage::Expand916,
    ProductionStage::VideoGenerate,
];

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && !value
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '/' | '\\'))
}
fn safe_directory(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("RESERVATION_ROOT_NOT_ABSOLUTE".into());
    }
    for ancestor in path.ancestors() {
        let meta = fs::symlink_metadata(ancestor).map_err(|_| "RESERVATION_ROOT_UNAVAILABLE")?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err("RESERVATION_ROOT_SYMLINK_OR_NOT_DIRECTORY".into());
        }
    }
    Ok(())
}

impl ProductionReservation {
    /// Reserve locally after user intent and authenticated identity are known.
    /// `root` must already exist and be selected by the desktop controller.
    pub fn reserve(
        root: &Path,
        owner: ProductionOwner,
        workflow: WorkflowBinding,
        plan: ProductionPlan,
    ) -> Result<Self, String> {
        if !valid_id(&owner.tenant_id)
            || !valid_id(&owner.user_id)
            || !valid_id(&owner.device_id)
            || !valid_id(&workflow.workflow_id)
            || !valid_id(&workflow.version)
        {
            return Err("RESERVATION_OWNER_OR_WORKFLOW_INVALID".into());
        }
        safe_directory(root)?;
        let id = new_id();
        let directory = root.join(&id);
        fs::create_dir(&directory).map_err(|_| "RESERVATION_CREATE_NEW_FAILED")?;
        // This intent precedes any source copy so a crash is not interpreted as
        // an unreserved directory that may be automatically recycled.
        let source_handle = format!("artifact-{}", new_id());
        write_new(
            &directory.join("reservation.json"),
            &serde_json::to_vec(&json!({
                "version":1,"reservation_id":id,"owner":owner,"workflow":workflow,
                "plan":plan,"source_handle":source_handle,"source_sha256":plan.anchor_sha256(),
                "allowed_stages":STAGES,"state":"PREPARING_LOCAL_SOURCE"
            }))
            .map_err(|_| "RESERVATION_SERIALIZE_FAILED")?,
        )?;
        let plan = plan.reserved_copy(&directory.join("anchor-image.bin"))?;
        let reservation = Self {
            id,
            owner,
            workflow,
            plan,
            root: directory,
            source_handle,
            outputs: vec![],
            terminal_failure: false,
        };
        reservation.checkpoint("RESERVED")?;
        Ok(reservation)
    }
    /// Opaque reservation identity. Does not authorize another owner/session.
    pub fn id(&self) -> &str {
        &self.id
    }
    /// Bound authenticated owner (contains no credential).
    pub fn owner(&self) -> &ProductionOwner {
        &self.owner
    }
    /// Expected central graph revision.
    pub fn workflow(&self) -> &WorkflowBinding {
        &self.workflow
    }
    /// Immutable local snapshot. Never serialize its filesystem paths to central ACKs.
    pub fn plan(&self) -> &ProductionPlan {
        &self.plan
    }
    /// Opaque input for the first stage, not a readable filesystem location.
    pub fn source_handle(&self) -> &str {
        &self.source_handle
    }
    /// Immutable input digest to include in the first central command.
    pub fn source_sha256(&self) -> &str {
        self.plan.anchor_sha256()
    }
    /// Exactly three single-operation nodes, never a whole-chain node dispatch.
    pub fn graph(&self) -> Vec<ProductionNode> {
        STAGES
            .iter()
            .enumerate()
            .map(|(index, stage)| ProductionNode {
                id: format!("grok-stage-{}", index + 1),
                node_type: if *stage == ProductionStage::ImageEdit {
                    "stage.grok_image_edit"
                } else {
                    "stage.grok_content_pipeline"
                }
                .into(),
                operation: match stage {
                    ProductionStage::ImageEdit => "image_edit",
                    ProductionStage::Expand916 => "expand_9_16",
                    _ => "video_generate",
                }
                .into(),
                stage: *stage,
                previous_node: if index == 0 {
                    None
                } else {
                    Some(format!("grok-stage-{index}"))
                },
            })
            .collect()
    }
    /// Central registration inputs with opaque references only. These operation
    /// names are the Concat allowlist, not a claim of an old generic chain API.
    pub fn worker_inputs(&self) -> serde_json::Value {
        json!({"reservation_id":self.id,"owner":self.owner,"workflow":self.workflow,
            "source_artifact_handle":self.source_handle,"source_sha256":self.plan.anchor_sha256(),
            "page_id":self.plan.page().id,"profile_id":self.plan.profile_id(),"graph":self.graph()})
    }
    /// Build a command using only the locally reserved artifact chain. The
    /// caller still verifies the authenticated graph/node/operation first.
    pub fn next_command(&self, stage: ProductionStage) -> Result<ProductionCommand, String> {
        let (handle, digest) = match self.outputs.last() {
            Some((receipt, _)) => (&receipt.artifact_handle, receipt.sha256.as_str()),
            None => (&self.source_handle, self.plan.anchor_sha256()),
        };
        let command = ProductionCommand {
            reservation_id: self.id.clone(),
            owner: self.owner.clone(),
            workflow: self.workflow.clone(),
            stage,
            input_artifact_handle: handle.clone(),
            expected_sha256: digest.into(),
        };
        self.validate_command(&command)?;
        Ok(command)
    }
    fn checkpoint(&self, state: &str) -> Result<(), String> {
        // Immutable append-only receipt. We intentionally expose no recovery
        // constructor: restart must reconcile the supervisor's unknown result.
        let bytes = serde_json::to_vec(&json!({"version":1,"reservation_id":self.id,"owner":self.owner,"workflow":self.workflow,"source_handle":self.source_handle,"source_sha256":self.plan.anchor_sha256(),"state":state,"completed":self.outputs.iter().map(|(receipt, artifact)| json!({"receipt":receipt,"local_artifact":artifact})).collect::<Vec<_>>()})).map_err(|_| "RESERVATION_SERIALIZE_FAILED")?;
        write_new(
            &self
                .root
                .join(format!("reservation-receipt-{}.json", new_id())),
            &bytes,
        )
    }
    /// Read-only gate to use BEFORE accepting/ACKing a central command. This does
    /// not replace the supervisor's authenticated command/run/fence ledger.
    pub fn validate_command(&self, command: &ProductionCommand) -> Result<(), String> {
        self.command_source(command).map(|_| ())
    }
    fn command_source(&self, command: &ProductionCommand) -> Result<(PathBuf, String), String> {
        if command.reservation_id != self.id {
            return Err("RESERVATION_NOT_OWNED".into());
        }
        if command.owner != self.owner {
            return Err("RESERVATION_OWNER_MISMATCH".into());
        }
        if command.workflow != self.workflow {
            return Err("RESERVATION_WORKFLOW_REVISION_MISMATCH".into());
        }
        if self.terminal_failure {
            return Err("RESERVATION_RECONCILIATION_REQUIRED_NO_RESUBMIT".into());
        }
        if STAGES.get(self.outputs.len()) != Some(&command.stage) {
            return Err("RESERVATION_STAGE_OUT_OF_ORDER_OR_DUPLICATE".into());
        }
        safe_directory(&self.root)?;
        for name in [
            format!("stage-{}", self.outputs.len() + 1),
            format!("stage-{}-intent.json", self.outputs.len() + 1),
        ] {
            match fs::symlink_metadata(self.root.join(name)) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err("RESERVATION_STAGE_EXISTS_NO_RESUBMIT".into()),
            }
        }
        let (handle, path, digest) = match self.outputs.last() {
            None => (
                &self.source_handle,
                self.plan.anchor_path(),
                self.plan.anchor_sha256(),
            ),
            Some((receipt, artifact)) => {
                artifact.verify_bytes()?;
                (
                    &receipt.artifact_handle,
                    artifact.path.as_path(),
                    artifact.sha256.as_str(),
                )
            }
        };
        if command.input_artifact_handle != *handle || command.expected_sha256 != digest {
            return Err("RESERVATION_INPUT_HANDLE_OR_HASH_MISMATCH".into());
        }
        // Re-read the exact own source/output, not a command-provided location.
        let bytes = read_bounded(path, 16 * 1024 * 1024)?;
        if hash(&bytes) != digest {
            return Err("RESERVATION_LOCAL_ARTIFACT_CHANGED".into());
        }
        Ok((path.to_path_buf(), digest.into()))
    }
    /// Execute one accepted step. The supervisor remains responsible for current
    /// run/command IDs, lease fences and an authenticated central ACK.
    pub fn execute(
        &mut self,
        client: &ProductionClient,
        command: &ProductionCommand,
        cancel: Arc<AtomicBool>,
        progress: impl FnMut(ProductionProgress),
    ) -> Result<WorkerStageReceipt, String> {
        let (source, digest) = self.command_source(command)?;
        let stage_root = self.root.join(format!("stage-{}", self.outputs.len() + 1));
        if stage_root.exists() {
            return Err("RESERVATION_STAGE_EXISTS_NO_RESUBMIT".into());
        }
        // A distinct create-new intent is durable BEFORE network activity. Even
        // definitely preflight failures require new explicit reservation intent.
        write_new(
            &self
                .root
                .join(format!("stage-{}-intent.json", self.outputs.len() + 1)),
            &serde_json::to_vec(command).map_err(|_| "RESERVATION_SERIALIZE_FAILED")?,
        )?;
        self.terminal_failure = true;
        let artifact = match client.run_stage(
            &self.plan,
            command.stage,
            &source,
            &digest,
            &stage_root,
            cancel,
            progress,
        ) {
            Ok(artifact) => artifact,
            Err(error) => {
                let _ = self.checkpoint("FAILED_OR_UNKNOWN_RECONCILE");
                return Err(error);
            }
        };
        artifact.verify_bytes()?;
        let receipt = WorkerStageReceipt {
            reservation_id: self.id.clone(),
            stage: command.stage,
            artifact_handle: format!("artifact-{}", new_id()),
            sha256: artifact.sha256.clone(),
            mime_type: artifact.mime_type.clone(),
            native_probe_required: command.stage == ProductionStage::VideoGenerate,
        };
        self.outputs.push((receipt.clone(), artifact));
        // If checkpoint fails the reservation remains fail-closed, even though a
        // provider has succeeded. It must not dispatch or ACK a second attempt.
        self.checkpoint("STAGE_SUCCEEDED")?;
        self.terminal_failure = false;
        Ok(receipt)
    }
    /// Final local descriptor for native probing only; no central path exposure.
    pub fn final_artifact_for_probe(&self) -> Result<&ProductionStageOutput, String> {
        if self.terminal_failure || self.outputs.len() != STAGES.len() {
            return Err("RESERVATION_FINAL_ARTIFACT_NOT_READY".into());
        }
        let artifact = &self
            .outputs
            .last()
            .ok_or("RESERVATION_FINAL_ARTIFACT_NOT_READY")?
            .1;
        artifact.verify_bytes()?;
        Ok(artifact)
    }
    /// Existing native import descriptor, exposed only after all three stages.
    /// This byte-level gate is NOT the required desktop decoder/probe verdict.
    pub fn final_output_for_probe(&self) -> Result<ProductionOutput, String> {
        let artifact = self.final_artifact_for_probe()?;
        let output = ProductionOutput {
            job_id: self.plan.job_id().into(),
            path: artifact.path.clone(),
            sha256: artifact.sha256.clone(),
            mime_type: artifact.mime_type.clone(),
            manifest_path: artifact.manifest_path.clone(),
            page: self.plan.page().clone(),
        };
        output.verify_bytes()?;
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pages::ContentPage;
    fn reserved() -> (PathBuf, ProductionReservation) {
        let root = std::env::temp_dir().join(format!("concat-reservation-test-{}", new_id()));
        fs::create_dir(&root).unwrap();
        let mut png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
        png.extend_from_slice(&1080u32.to_be_bytes());
        png.extend_from_slice(&1920u32.to_be_bytes());
        fs::write(root.join("source.png"), png).unwrap();
        let page:ContentPage = serde_json::from_value(json!({"id":"page-a","name":"Page","slug":"page-a","output_root":root,"browser_profile_id":"profile-a","is_archived":false,"created_at":1,"updated_at":1,"revision":1})).unwrap();
        let plan = ProductionPlan::new(page, root.join("source.png"), "edit", "vertical", "video")
            .unwrap();
        let owner = ProductionOwner {
            tenant_id: "tenant-a".into(),
            user_id: "user-a".into(),
            device_id: "device-a".into(),
        };
        let workflow = WorkflowBinding {
            workflow_id: "workflow-a".into(),
            version: "1.0.0".into(),
        };
        let reservation = ProductionReservation::reserve(&root, owner, workflow, plan).unwrap();
        (root, reservation)
    }
    fn command(r: &ProductionReservation) -> ProductionCommand {
        ProductionCommand {
            reservation_id: r.id().into(),
            owner: r.owner().clone(),
            workflow: r.workflow().clone(),
            stage: ProductionStage::ImageEdit,
            input_artifact_handle: r.source_handle().into(),
            expected_sha256: r.source_sha256().into(),
        }
    }
    #[test]
    fn owner_workflow_stage_handle_and_digest_are_independent_gates() {
        let (root, r) = reserved();
        let c = command(&r);
        assert!(r.validate_command(&c).is_ok());
        let mut bad = c.clone();
        bad.owner.user_id = "other".into();
        assert!(r.validate_command(&bad).is_err());
        let mut bad = c.clone();
        bad.reservation_id = "other".into();
        assert!(r.validate_command(&bad).is_err());
        let mut bad = c.clone();
        bad.workflow.version = "2.0.0".into();
        assert!(r.validate_command(&bad).is_err());
        let mut bad = c.clone();
        bad.stage = ProductionStage::Expand916;
        assert!(r.validate_command(&bad).is_err());
        let mut bad = c.clone();
        bad.expected_sha256 = "0".repeat(64);
        assert!(r.validate_command(&bad).is_err());
        let mut bad = c.clone();
        bad.input_artifact_handle = "C:\\arbitrary.png".into();
        assert!(r.validate_command(&bad).is_err());
        // Only remove the unique test-created root, never production data.
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn copied_source_is_immutable_and_graph_is_exactly_three_operations() {
        let (root, r) = reserved();
        fs::write(root.join("source.png"), b"replaced original").unwrap();
        assert!(r.validate_command(&command(&r)).is_ok());
        let graph = r.graph();
        assert_eq!(graph.len(), 3);
        assert_eq!(graph[0].node_type, "stage.grok_image_edit");
        assert_eq!(graph[1].operation, "expand_9_16");
        assert_eq!(graph[2].operation, "video_generate");
        fs::write(r.plan.anchor_path(), b"changed reserved source").unwrap();
        assert!(r.validate_command(&command(&r)).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn an_unfinished_stage_intent_cannot_be_resubmitted() {
        let (root, mut r) = reserved();
        let c = command(&r);
        write_new(
            &r.root.join("stage-1-intent.json"),
            b"durable interrupted intent",
        )
        .unwrap();
        let client = ProductionClient::new(
            crate::browser::BrowserClient::new("http://127.0.0.1:1").unwrap(),
        );
        // Fails on the local create-new intent, before any runtime request.
        assert!(
            r.execute(&client, &c, Arc::new(AtomicBool::new(false)), |_| {})
                .is_err()
        );
        assert!(!r.root.join("stage-1").exists());
        assert!(r.final_artifact_for_probe().is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
