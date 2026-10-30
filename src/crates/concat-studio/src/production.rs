// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native extraction of ArtCraft's three Grok stages, using Nexora's existing
//! `floword-production` v1 worker/lease protocol, not an invented workflow API.
//! Source: ArtCraft `services/pipeline/grok_*_stage.rs` and
//! `floword_job_page_config.rs`. A successful byte receipt is NOT a decoder
//! verdict: the desktop caller must probe the final video before importing it.
use crate::{browser::BrowserClient, pages::ContentPage};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MAX_IMAGE: u64 = 16 * 1024 * 1024;
const MAX_VIDEO: u64 = 100 * 1024 * 1024;

/// Exact existing browser production stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProductionStage {
    /// Edit the selected anchor through the existing Grok browser worker.
    ImageEdit,
    /// Expand the verified previous image to vertical 9:16.
    Expand916,
    /// Generate video from the exact verified vertical artifact.
    VideoGenerate,
    /// Persist bytes for the native editor's decoder/import gate.
    LocalSave,
}
impl ProductionStage {
    /// ArtCraft's durable step identifier.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ImageEdit => "GENERATING_IMAGE",
            Self::Expand916 => "CONVERTING_9_16",
            Self::VideoGenerate => "GENERATING_VIDEO",
            Self::LocalSave => "SAVING_LOCAL",
        }
    }
    fn method(self) -> &'static str {
        match self {
            Self::ImageEdit => "grok.image.edit",
            Self::Expand916 => "grok.image.expand_9_16",
            Self::VideoGenerate => "grok.video.generate",
            Self::LocalSave => "",
        }
    }
}

/// Immutable job inputs: editing the Page later cannot redirect this job.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ProductionPlan {
    page: ContentPage,
    anchor: Option<PathBuf>,
    anchor_sha256: String,
    image_prompt: String,
    expand_prompt: String,
    video_prompt: String,
    job_id: String,
}
impl ProductionPlan {
    /// Freeze the verified anchor inside a newly reserved, locally owned directory.
    /// This is an internal reservation operation, never a remote path parameter.
    pub(crate) fn reserved_copy(&self, destination: &Path) -> Result<Self, String> {
        let bytes = self.source()?;
        validate_source_image(&bytes)?;
        write_new(destination, &bytes)?;
        let mut plan = self.clone();
        plan.anchor = Some(destination
            .canonicalize()
            .map_err(|_| "SOURCE_IMAGE_MISSING")?);
        Ok(plan)
    }
    /// Hash of the immutable selected source, usable as a command correlation.
    pub fn anchor_sha256(&self) -> &str {
        &self.anchor_sha256
    }
    /// Locally verified source; controllers must never fill this from a server path.
    pub(crate) fn anchor_path(&self) -> &Path {
        self.anchor.as_deref().unwrap_or_else(|| Path::new(""))
    }
    /// Validate and snapshot a selected Page and its source before any dispatch.
    pub fn new(
        page: ContentPage,
        anchor: PathBuf,
        image_prompt: &str,
        expand_prompt: &str,
        video_prompt: &str,
    ) -> Result<Self, String> {
        Self::with_source(page, Some(anchor), image_prompt, expand_prompt, video_prompt)
    }
    /// Generate the first image from its prompt without an uploaded reference.
    /// The existing Grok image worker accepts an omitted sourceArtifact.
    pub fn from_prompt(
        page: ContentPage,
        image_prompt: &str,
        expand_prompt: &str,
        video_prompt: &str,
    ) -> Result<Self, String> {
        Self::with_source(page, None, image_prompt, expand_prompt, video_prompt)
    }
    fn with_source(
        page: ContentPage,
        anchor: Option<PathBuf>,
        image_prompt: &str,
        expand_prompt: &str,
        video_prompt: &str,
    ) -> Result<Self, String> {
        if serde_json::to_vec(&page)
            .map_err(|_| "PAGE_CONFIGURATION_INVALID")?
            .len()
            > 256 * 1024
        {
            return Err("PAGE_CONFIGURATION_TOO_LARGE".into());
        }
        if page.is_archived
            || !identifier(&page.id)
            || page.name.trim().is_empty()
            || !Path::new(&page.output_root).is_absolute()
        {
            return Err("PAGE_CONFIGURATION_INVALID".into());
        }
        if !page.browser_profile_id.as_deref().is_some_and(identifier) {
            return Err("PAGE_NEXORA_PROFILE_REQUIRED".into());
        }
        if page
            .default_aspect_ratio
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty() && s != "9:16")
        {
            return Err("GROK_PIPELINE_REQUIRES_9_16".into());
        }
        let choose =
            |explicit: &str, default: Option<&str>, fallback: &str| -> Result<String, String> {
                let value = if !explicit.trim().is_empty() {
                    explicit
                } else {
                    default.filter(|s| !s.trim().is_empty()).unwrap_or(fallback)
                };
                if value.trim().is_empty() || value.len() > 32_768 || value.contains('\0') {
                    return Err("PRODUCTION_PROMPT_REQUIRED_OR_TOO_LARGE".into());
                }
                Ok(value.trim().into())
            };
        let image_prompt = choose(image_prompt, page.default_image_prompt.as_deref(), "")?;
        let expand_prompt = choose(
            expand_prompt,
            page.default_expand_9_16_prompt.as_deref(),
            "Expand image to 9:16 vertical ratio preserving subject and composition",
        )?;
        let video_prompt = choose(
            video_prompt,
            page.default_video_prompt.as_deref(),
            "Generate cinematic dynamic video animation preserving character and action",
        )?;
        let (anchor, anchor_sha256) = if let Some(anchor) = anchor {
            let anchor = anchor.canonicalize().map_err(|_| "SOURCE_IMAGE_MISSING")?;
            let bytes = read_bounded(&anchor, MAX_IMAGE)?;
            image_mime(&bytes)?;
            (Some(anchor), hash(&bytes))
        } else {
            (None, String::new())
        };
        Ok(Self {
            page,
            anchor,
            anchor_sha256,
            image_prompt,
            expand_prompt,
            video_prompt,
            job_id: new_id(),
        })
    }
    /// Selected Page snapshot.
    pub fn page(&self) -> &ContentPage {
        &self.page
    }
    /// Exact selected Nexora identity; no first-profile fallback.
    pub fn profile_id(&self) -> &str {
        self.page.browser_profile_id.as_deref().unwrap_or("")
    }
    /// Durable local job identity.
    pub fn job_id(&self) -> &str {
        &self.job_id
    }
    fn source(&self) -> Result<Vec<u8>, String> {
        let Some(anchor) = &self.anchor else { return Ok(Vec::new()); };
        let bytes = read_bounded(anchor, MAX_IMAGE)?;
        image_mime(&bytes)?;
        if hash(&bytes) != self.anchor_sha256 {
            return Err("SOURCE_IMAGE_CHANGED_RESELECT_REQUIRED".into());
        }
        Ok(bytes)
    }
}

/// Progress reflects completed network/artifact operations, never fake percentages.
#[derive(Clone, Debug)]
#[allow(missing_docs)]
pub struct ProductionProgress {
    pub stage: ProductionStage,
    pub state: String,
    pub message: String,
}

/// Verified local bytes. The native editor must additionally decode/probe before import.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[allow(missing_docs)]
pub struct ProductionOutput {
    pub job_id: String,
    pub path: PathBuf,
    pub sha256: String,
    pub mime_type: String,
    pub manifest_path: PathBuf,
    pub page: ContentPage,
}

/// A single confirmed provider stage, not a completed three-stage pipeline.
/// Video artifacts still require the desktop decoder/probe before importing.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[allow(missing_docs)]
pub struct ProductionStageOutput {
    pub stage: ProductionStage,
    pub path: PathBuf,
    pub sha256: String,
    pub mime_type: String,
    pub manifest_path: PathBuf,
}
impl ProductionStageOutput {
    /// Detect replacement before advancing a reserved workflow or importing.
    pub fn verify_bytes(&self) -> Result<(), String> {
        reject_symlink(&self.path)?;
        let bytes = read_bounded(
            &self.path,
            if self.stage == ProductionStage::VideoGenerate {
                MAX_VIDEO
            } else {
                MAX_IMAGE
            },
        )?;
        let mime = if self.stage == ProductionStage::VideoGenerate {
            video_mime(&bytes)?.0
        } else {
            validate_source_image(&bytes)?;
            image_mime(&bytes)?.0
        };
        if hash(&bytes) != self.sha256 || mime != self.mime_type {
            return Err("STAGE_ARTIFACT_HASH_OR_MIME_MISMATCH".into());
        }
        Ok(())
    }
}
impl ProductionOutput {
    /// Revalidate the artifact at the import boundary to reject replacement.
    pub fn verify_bytes(&self) -> Result<(), String> {
        let bytes = read_bounded(&self.path, MAX_VIDEO)?;
        video_mime(&bytes)?;
        if hash(&bytes) != self.sha256 {
            return Err("VIDEO_CHANGED_BEFORE_IMPORT".into());
        }
        Ok(())
    }
}

/// Local browser transport with explicit user-owned Nexora lifecycle.
pub struct ProductionClient {
    browser: BrowserClient,
    recovery_root: Option<PathBuf>,
}
impl ProductionClient {
    /// Execute exactly ONE reserved workflow stage through the existing lease,
    /// dispatch, heartbeat, cancellation and receipt protocol. `workspace` is a
    /// fresh local reservation child, never a path supplied by the central server.
    /// Existing directories are refused even if their previous outcome is unknown.
    pub fn run_stage(
        &self,
        plan: &ProductionPlan,
        stage: ProductionStage,
        source: &Path,
        source_sha256: &str,
        workspace: &Path,
        cancel: Arc<AtomicBool>,
        mut progress: impl FnMut(ProductionProgress),
    ) -> Result<ProductionStageOutput, String> {
        let prompt = match stage {
            ProductionStage::ImageEdit => &plan.image_prompt,
            ProductionStage::Expand916 => &plan.expand_prompt,
            ProductionStage::VideoGenerate => &plan.video_prompt,
            ProductionStage::LocalSave => return Err("CENTRAL_STAGE_NOT_ALLOWED".into()),
        };
        let recovery_root = self
            .recovery_root
            .as_ref()
            .ok_or("CENTRAL_PROFILE_RECOVERY_ROOT_REQUIRED")?;
        if !workspace.is_absolute() || !source.is_absolute() {
            return Err("STAGE_LOCAL_PATH_INVALID".into());
        }
        reject_symlink(source)?;
        reject_symlink(workspace.parent().ok_or("STAGE_LOCAL_PATH_INVALID")?)?;
        let bytes = read_bounded(source, MAX_IMAGE)?;
        validate_source_image(&bytes)?;
        if hash(&bytes) != source_sha256
            || (stage == ProductionStage::ImageEdit && source_sha256 != plan.anchor_sha256)
        {
            return Err("STAGE_SOURCE_HASH_MISMATCH".into());
        }
        if stage == ProductionStage::VideoGenerate {
            let (w, h) = dimensions(&bytes)?;
            if ((w as f64 / h as f64) - 9.0 / 16.0).abs() > 0.02 {
                return Err("VERTICAL_ASPECT_INVALID".into());
            }
        }
        cancelled(&cancel)?;
        self.preflight(plan)?;
        // The directory itself is the create-new stage fence. No retry path can
        // silently turn an interrupted attempt into another paid submission.
        fs::create_dir(workspace).map_err(|_| "STAGE_ALREADY_EXISTS_NO_RESUBMIT")?;
        let manifest = workspace.join("manifest.json");
        let mut journal = json!({"version":1,"plan":plan,"state":"PREPARED","current_stage":stage.as_str(),"source_sha256":source_sha256,"stages":[]});
        write_new(
            &manifest,
            &serde_json::to_vec(&journal).map_err(|_| "JOURNAL_INVALID")?,
        )?;
        let guard = RecoveryGuard::acquire(recovery_root, plan, &manifest)?;
        let result: Result<ProductionStageOutput, String> = (|| {
            cancelled(&cancel)?;
            let owned_source = workspace.join("source-image.bin");
            write_new(&owned_source, &bytes)?;
            journal["state"] = json!("RUNNING");
            persist(&manifest, &journal)?;
            progress(ProductionProgress {
                stage,
                state: "RUNNING".into(),
                message: stage.as_str().into(),
            });
            let (output_bytes, receipt) = self.stage(
                plan,
                stage,
                prompt,
                &owned_source,
                &bytes,
                &cancel,
                &manifest,
                &mut journal,
            )?;
            cancelled(&cancel)?;
            let (mime, ext) = if stage == ProductionStage::VideoGenerate {
                video_mime(&output_bytes)?
            } else {
                validate_source_image(&output_bytes)?;
                image_mime(&output_bytes)?
            };
            let path = workspace.join(format!("artifact.{ext}"));
            write_new(&path, &output_bytes)?;
            let output = ProductionStageOutput {
                stage,
                path,
                sha256: hash(&output_bytes),
                mime_type: mime.into(),
                manifest_path: manifest.clone(),
            };
            output.verify_bytes()?;
            journal["state"] = json!(if stage == ProductionStage::VideoGenerate {
                "AWAITING_NATIVE_MEDIA_PROBE"
            } else {
                "SUCCEEDED"
            });
            journal["stages"] = json!([{"stage":stage.as_str(),"receipt":compact_receipt(&receipt),"output":output}]);
            persist(&manifest, &journal)?;
            progress(ProductionProgress {
                stage,
                state: "SUCCEEDED".into(),
                message: "Single-stage artifact verified; no next stage dispatched".into(),
            });
            Ok(output)
        })();
        if let Err(error) = &result {
            journal["state"] = json!(
                if error.contains("UNKNOWN") || error.contains("UNCONFIRMED") {
                    "UNKNOWN_OUTCOME"
                } else if error == "CANCELLED" {
                    "CANCELLED"
                } else {
                    "FAILED"
                }
            );
            journal["error_code"] = json!(error);
            let _ = persist(&manifest, &journal);
        }
        let unknown = journal
            .get("pending_submission")
            .is_some_and(|v| !v.is_null())
            || result
                .as_ref()
                .err()
                .is_some_and(|e| e.contains("UNKNOWN") || e.contains("UNCONFIRMED"));
        if !unknown {
            guard.clear()?;
        }
        result
    }
    /// Reuse the configured local browser token; never cloud credentials.
    pub fn new(browser: BrowserClient) -> Self {
        Self {
            browser,
            recovery_root: None,
        }
    }
    /// Use Concat's explicit config directory for a cross-Page/profile recovery
    /// fence. Desktop integrations must set this; no ambient/global path is guessed.
    pub fn with_recovery_root(mut self, root: PathBuf) -> Self {
        self.recovery_root = Some(root);
        self
    }
    /// Read-only readiness and source checks. Never starts a browser/provider job.
    pub fn preflight(&self, plan: &ProductionPlan) -> Result<(), String> {
        plan.source()?;
        if let Some(root) = &self.recovery_root {
            central_recovery_check(root, plan.profile_id())?;
        }
        recovery_gate(plan)?;
        self.browser
            .health()
            .map_err(|_| "NEXORA_RUNTIME_UNAVAILABLE_OR_INCOMPATIBLE")?;
        let profiles = self
            .browser
            .profiles()
            .map_err(|_| "NEXORA_PROFILE_CATALOG_UNAVAILABLE")?;
        if !profiles.iter().any(|p| p.id == plan.profile_id()) {
            return Err("PAGE_NEXORA_PROFILE_NOT_FOUND".into());
        }
        Ok(())
    }
    /// Run exactly image edit -> vertical expand -> video, preserving receipts.
    /// Any unknown submission outcome is durable and never automatically retried.
    pub fn run(
        &self,
        plan: ProductionPlan,
        cancel: Arc<AtomicBool>,
        mut progress: impl FnMut(ProductionProgress),
    ) -> Result<ProductionOutput, String> {
        self.preflight(&plan)?;
        cancelled(&cancel)?;
        let root = PathBuf::from(&plan.page.output_root)
            .join("concat-production")
            .join(&plan.job_id);
        let manifest = root.join("manifest.json");
        let guard = self
            .recovery_root
            .as_ref()
            .map(|recovery_root| RecoveryGuard::acquire(recovery_root, &plan, &manifest))
            .transpose()?;
        if fs::create_dir_all(root.parent().ok_or("JOB_STORAGE_UNAVAILABLE")?).is_err() {
            if let Some(guard) = guard {
                guard.clear()?;
            }
            return Err("JOB_STORAGE_UNAVAILABLE".into());
        }
        // Existing job IDs are never resubmitted, including after an uncertain
        // dispatch/crash. Recovery requires reviewing retained receipts.
        if fs::create_dir(&root).is_err() {
            if let Some(guard) = guard {
                guard.clear()?;
            }
            return Err("JOB_ALREADY_EXISTS_REVIEW_RECEIPTS_NO_RESUBMIT".into());
        }
        let mut journal = json!({"version":1,"plan":plan,"state":"PREPARED","stages":[]});
        if let Err(error) = persist(&manifest, &journal) {
            if let Some(guard) = guard {
                guard.clear()?;
            }
            return Err(error);
        }
        let result: Result<ProductionOutput, String> = (|| {
            let mut bytes = plan.source()?;
            let mut source = root.join("source-image.bin");
            if plan.anchor.is_some() { write_new(&source, &bytes)?; }
            for (stage, prompt) in [
                (ProductionStage::ImageEdit, &plan.image_prompt),
                (ProductionStage::Expand916, &plan.expand_prompt),
                (ProductionStage::VideoGenerate, &plan.video_prompt),
            ] {
                cancelled(&cancel)?;
                progress(ProductionProgress {
                    stage,
                    state: "RUNNING".into(),
                    message: stage.as_str().into(),
                });
                journal["state"] = json!("RUNNING");
                journal["current_stage"] = json!(stage.as_str());
                persist(&manifest, &journal)?;
                let (next, receipt) = self.stage(
                    &plan,
                    stage,
                    prompt,
                    &source,
                    &bytes,
                    &cancel,
                    &manifest,
                    &mut journal,
                )?;
                bytes = next;
                let ext = if stage == ProductionStage::VideoGenerate {
                    video_mime(&bytes)?.1
                } else {
                    image_mime(&bytes)?.1
                };
                source = root.join(format!("{}.{ext}", stage.as_str()));
                write_new(&source, &bytes)?;
                let mime = if stage == ProductionStage::VideoGenerate {
                    video_mime(&bytes)?.0
                } else {
                    image_mime(&bytes)?.0
                };
                let row = json!({"stage":stage.as_str(),"receipt":compact_receipt(&receipt),"path":source,"sha256":hash(&bytes),"bytes":bytes.len(),"mime_type":mime});
                journal["stages"]
                    .as_array_mut()
                    .ok_or("JOURNAL_INVALID")?
                    .push(row);
                persist(&manifest, &journal)?;
                progress(ProductionProgress {
                    stage,
                    state: "SUCCEEDED".into(),
                    message: "Artifact saved and hash verified".into(),
                });
            }
            cancelled(&cancel)?;
            let mime = video_mime(&bytes)?.0.to_string();
            let output = ProductionOutput {
                job_id: plan.job_id.clone(),
                path: source,
                sha256: hash(&bytes),
                mime_type: mime,
                manifest_path: manifest.clone(),
                page: plan.page.clone(),
            };
            output.verify_bytes()?;
            journal["state"] = json!("AWAITING_NATIVE_MEDIA_PROBE");
            journal["output"] = serde_json::to_value(&output).map_err(|_| "JOURNAL_INVALID")?;
            persist(&manifest, &journal)?;
            progress(ProductionProgress {
                stage: ProductionStage::LocalSave,
                state: "SUCCEEDED".into(),
                message: "Saved video; native decoder verification required before import".into(),
            });
            Ok(output)
        })();
        if let Err(ref error) = result {
            journal["state"] = json!(if error == "CANCELLED" {
                "CANCELLED"
            } else if error.contains("UNKNOWN") || error.contains("UNCONFIRMED") {
                "UNKNOWN_OUTCOME"
            } else {
                "FAILED"
            });
            journal["error_code"] = json!(error);
            let _ = persist(&manifest, &journal);
        }
        if let Some(guard) = guard {
            let unknown =
                result.as_ref().err().is_some_and(|error| {
                    error.contains("UNKNOWN") || error.contains("UNCONFIRMED")
                }) || journal
                    .get("pending_submission")
                    .is_some_and(|value| !value.is_null());
            if !unknown {
                guard.clear()?;
            }
            // Unknown outcome or panic/crash: guard Drop only closes its file;
            // it never removes the durable profile fence.
        }
        result
    }

    fn stage(
        &self,
        plan: &ProductionPlan,
        stage: ProductionStage,
        prompt: &str,
        source: &Path,
        bytes: &[u8],
        cancel: &Arc<AtomicBool>,
        manifest: &Path,
        journal: &mut Value,
    ) -> Result<(Vec<u8>, Value), String> {
        let attempt = "1";
        let lease=self.request("POST","/v1/workers/acquire",Some(&json!({"job_id":plan.job_id,"step_id":stage.as_str(),"attempt_id":attempt,"capability":stage.method(),"pool_id":plan.page.worker_pool_id,"profile_id":plan.profile_id(),"ttl_seconds":120})),60,1024*1024)?;
        let lease_id = field(&lease, "lease_id")?.to_string();
        let worker = field(&lease, "worker_id")?.to_string();
        if !identifier(&lease_id)
            || !(identifier(&worker) || worker == format!("playwright-profile:{}", plan.profile_id()))
            || lease["profile_id"] != plan.profile_id()
        {
            return Err("LEASE_CORRELATION_INVALID".into());
        }
        if cancel.load(Ordering::Acquire) {
            self.release(&lease_id)
                .map_err(|_| "CANCEL_BEFORE_DISPATCH_RELEASE_UNCONFIRMED")?;
            return Err("CANCELLED".into());
        }
        let request_id = new_id();
        let mut params = json!({"prompt":prompt,"timeoutMs":if stage==ProductionStage::VideoGenerate{300000}else{180000}});
        if stage != ProductionStage::ImageEdit || plan.anchor.is_some() {
            let mime = image_mime(bytes)?.0;
            params["sourceArtifact"] = json!({"artifactId":format!("art_{}",hash(bytes)),"path":source,"dataUrl":format!("data:{mime};base64,{}",STANDARD.encode(bytes)),"mimeType":mime});
        }
        if stage == ProductionStage::Expand916 {
            params["targetAspectRatio"] = json!("9:16");
        }
        let request = json!({"protocol":"floword-production","protocolVersion":1,"requestId":request_id,"jobId":plan.job_id,"stepId":stage.as_str(),"attemptId":attempt,"leaseId":lease_id,"profileId":plan.profile_id(),"pageId":plan.page.id,"method":stage.method(),"params":params,"createdAt":timestamp()});
        // Write submission intent BEFORE dispatch; a crash cannot silently rerun it.
        journal["pending_submission"] = json!({"requestId":request_id,"jobId":plan.job_id,"stepId":stage.as_str(),"attemptId":attempt,"leaseId":lease_id,"profileId":plan.profile_id(),"workerId":worker,"state":"UNKNOWN_UNTIL_RECEIPT"});
        if let Err(e) = persist(manifest, journal) {
            if self.release(&lease_id).is_err() {
                return Err("LEASE_RELEASE_UNCONFIRMED_UNKNOWN_OUTCOME".into());
            }
            return Err(e);
        }
        let done = Arc::new(AtomicBool::new(false));
        let lost = Arc::new(AtomicBool::new(false));
        let heartbeat = self.browser.clone();
        let hb_done = done.clone();
        let hb_lost = lost.clone();
        let hb_lease = lease_id.clone();
        let hb_job = plan.job_id.clone();
        let handle = std::thread::spawn(move || {
            let mut ticks = 0;
            while !hb_done.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(100));
                ticks += 1;
                if ticks >= 300 {
                    ticks = 0;
                    let value = heartbeat.request_bounded(
                        "POST",
                        &format!("/v1/workers/leases/{hb_lease}/heartbeat"),
                        Some(&json!({"job_id":hb_job,"attempt_id":"1","ttl_seconds":60})),
                        Duration::from_secs(5),
                        1024 * 1024,
                    );
                    if !value.is_ok_and(|v| v["status"] == "ACTIVE") {
                        hb_lost.store(true, Ordering::Release);
                        break;
                    }
                }
            }
        });
        let client = self.browser.clone();
        let dispatch_path = format!("/v1/workers/{worker}/dispatch");
        let request_copy = request.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let dispatch = std::thread::spawn(move || {
            let result = client.request_bounded(
                "POST",
                &dispatch_path,
                Some(&request_copy),
                Duration::from_secs(310),
                MAX_VIDEO * 2,
            );
            let _ = tx.send(result);
        });
        let mut confirmed_cancel = None;
        let result: Result<Value, String> = loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(value) => break value.map_err(|_| "DISPATCH_UNKNOWN_OUTCOME".to_string()),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    break Err("DISPATCH_UNKNOWN_OUTCOME".into());
                }
                Err(_) => {
                    if cancel.load(Ordering::Acquire) || lost.load(Ordering::Acquire) {
                        let ack=self.request("POST",&format!("/v1/workers/{worker}/jobs/{}/cancel",plan.job_id),Some(&json!({"stepId":stage.as_str(),"attemptId":attempt,"leaseId":lease_id,"profileId":plan.profile_id(),"targetRequestId":request_id})),10,1024*1024);
                        if let Some(value) =
                            ack.ok().filter(|value| cancel_correlated(&request, value))
                        {
                            confirmed_cancel = Some(value);
                            break Err(if cancel.load(Ordering::Acquire) {
                                "CANCELLED"
                            } else {
                                "LEASE_LOST"
                            }
                            .into());
                        }
                        break Err("CANCEL_UNCONFIRMED_UNKNOWN_OUTCOME".into());
                    }
                }
            }
        };
        // A blocked provider dispatch is bounded by HTTP timeout. Its detached
        // thread cannot publish artifacts or mutate journal after cancellation.
        if dispatch.is_finished() {
            let _ = dispatch.join();
        }
        let result = result.and_then(|receipt| {
            correlate(&request, &receipt)?;
            if receipt["ok"] != true {
                return Err("PROVIDER_STAGE_FAILED".into());
            }
            if lost.load(Ordering::Acquire) {
                return Err("LEASE_LOST".into());
            }
            let locator = field(&receipt["result"], "locator")?;
            let downloaded = self.materialize(plan.profile_id(), locator, stage)?;
            if stage != ProductionStage::VideoGenerate {
                image_mime(&downloaded)?;
                if hash(&downloaded) == hash(bytes) {
                    return Err("SOURCE_ARTIFACT_ECHO".into());
                }
            }
            if stage == ProductionStage::Expand916 {
                let (w, h) = dimensions(&downloaded)?;
                if w == 0 || h == 0 || ((w as f64 / h as f64) - (9.0 / 16.0)).abs() > 0.02 {
                    return Err("VERTICAL_ASPECT_INVALID".into());
                }
            }
            if stage == ProductionStage::VideoGenerate {
                video_mime(&downloaded)?;
            }
            let terminal = self.request(
                "POST",
                &format!("/v1/workers/leases/{lease_id}/heartbeat"),
                Some(&json!({"job_id":plan.job_id,"attempt_id":attempt,"ttl_seconds":60})),
                5,
                1024 * 1024,
            )?;
            if terminal["status"] != "ACTIVE" {
                return Err("TERMINAL_OWNERSHIP_LOST".into());
            }
            Ok((downloaded, receipt))
        });
        done.store(true, Ordering::Release);
        let _ = handle.join();
        // A post-submission provider failure without a reconciled terminal
        // receipt is also uncertain: the old Sidecar can time out after submit.
        let result = result.map_err(|error| {
            if error == "CANCELLED" || error == "LEASE_LOST" || error.contains("UNKNOWN") {
                error
            } else {
                format!("{error}_UNKNOWN_OUTCOME")
            }
        });
        // Transport/cancel ambiguity retains the lease for the runtime TTL
        // reaper. Never release a worker that may still be generating.
        if !result
            .as_ref()
            .err()
            .is_some_and(|e| e.contains("UNKNOWN") || e.contains("UNCONFIRMED"))
        {
            if self.release(&lease_id).is_err() {
                return Err("LEASE_RELEASE_UNCONFIRMED_UNKNOWN_OUTCOME".into());
            }
        }
        if result.is_ok() || confirmed_cancel.is_some() {
            if let Some(receipt) = confirmed_cancel {
                let mut compact = compact_receipt(&receipt);
                compact["result"] = json!({
                    "cancelled":true,
                    "requestId":receipt["result"]["requestId"],
                    "acknowledgment":{"ok":true}
                });
                journal["confirmed_cancel"] = compact;
            }
            journal
                .as_object_mut()
                .ok_or("JOURNAL_INVALID")?
                .remove("pending_submission");
        }
        result
    }
    fn release(&self, id: &str) -> Result<(), String> {
        self.request(
            "POST",
            &format!("/v1/workers/leases/{id}/release"),
            None,
            5,
            1024 * 1024,
        )
        .and_then(|value| {
            if value["lease_id"] == id && value["status"] == "RELEASED" {
                Ok(())
            } else {
                Err("LEASE_RELEASE_RECEIPT_INVALID".into())
            }
        })
    }
    fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
        seconds: u64,
        limit: u64,
    ) -> Result<Value, String> {
        self.browser
            .request_bounded(method, path, body, Duration::from_secs(seconds), limit)
            .map_err(|_| "NEXORA_REQUEST_FAILED".into())
    }
    fn materialize(
        &self,
        profile: &str,
        locator: &str,
        stage: ProductionStage,
    ) -> Result<Vec<u8>, String> {
        let limit = if stage == ProductionStage::VideoGenerate {
            MAX_VIDEO
        } else {
            MAX_IMAGE
        };
        if locator.starts_with("data:") {
            let (header, data) = locator.split_once(',').ok_or("ARTIFACT_DATA_URL_INVALID")?;
            if !header.ends_with(";base64") || data.len() as u64 > limit * 4 / 3 + 8 {
                return Err("ARTIFACT_TOO_LARGE_OR_INVALID".into());
            }
            let bytes = STANDARD
                .decode(data)
                .map_err(|_| "ARTIFACT_DATA_URL_INVALID")?;
            if bytes.is_empty() || bytes.len() as u64 > limit {
                return Err("ARTIFACT_EMPTY_OR_TOO_LARGE".into());
            }
            return Ok(bytes);
        }
        artifact_url(locator)?;
        if stage != ProductionStage::VideoGenerate {
            // Canonical authenticated browser-context image fetch from ArtCraft.
            let origin = std::env::var("FLOWORD_PLAYWRIGHT_RUNTIME_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:9223".into());
            let client =
                BrowserClient::with_token(&origin, std::env::var("FLOWORD_SIDECAR_TOKEN").ok())
                    .map_err(|_| "SIDECAR_CONFIGURATION_INVALID")?;
            let value = client
                .request_bounded(
                    "POST",
                    &format!("/v1/profiles/{profile}/artifacts/fetch"),
                    Some(&json!({"locator":locator})),
                    Duration::from_secs(40),
                    MAX_IMAGE * 2,
                )
                .map_err(|_| "ARTIFACT_BROWSER_FETCH_FAILED")?;
            if value["profileId"] != profile {
                return Err("ARTIFACT_PROFILE_MISMATCH".into());
            }
            let data = field(&value, "dataBase64")?;
            let bytes = STANDARD
                .decode(data)
                .map_err(|_| "ARTIFACT_INVALID_BASE64")?;
            if bytes.is_empty() || bytes.len() as u64 > limit {
                return Err("ARTIFACT_EMPTY_OR_TOO_LARGE".into());
            }
            return Ok(bytes);
        }
        // Existing Sidecar supports image fetch only. Public video locators use
        // the old direct path, now HTTPS host-checked and with redirects disabled.
        let response = ureq::AgentBuilder::new()
            .redirects(0)
            .timeout(Duration::from_secs(60))
            .build()
            .get(locator)
            .call()
            .map_err(|_| "VIDEO_DOWNLOAD_FAILED_AUTHENTICATED_RUNTIME_MAY_BE_REQUIRED")?;
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "VIDEO_DOWNLOAD_FAILED")?;
        if bytes.is_empty() || bytes.len() as u64 > limit {
            return Err("ARTIFACT_EMPTY_OR_TOO_LARGE".into());
        }
        Ok(bytes)
    }
}

fn field<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "PRODUCTION_RECEIPT_INVALID".into())
}
fn correlate(request: &Value, receipt: &Value) -> Result<(), String> {
    for key in [
        "protocol",
        "protocolVersion",
        "requestId",
        "jobId",
        "stepId",
        "attemptId",
        "leaseId",
        "profileId",
    ] {
        if request.get(key).is_none() || request.get(key) != receipt.get(key) {
            return Err("PRODUCTION_RECEIPT_CORRELATION_MISMATCH".into());
        }
    }
    Ok(())
}
// Provider receipts may contain a 100 MiB data URL or a signed CDN URL. Keep
// only bounded correlation/evidence in durable journals; actual bytes already
// live in the hash-verified artifact file. Never duplicate payloads or secrets.
fn compact_receipt(receipt: &Value) -> Value {
    let mut compact = json!({});
    for key in [
        "protocol",
        "requestId",
        "jobId",
        "stepId",
        "attemptId",
        "leaseId",
        "profileId",
    ] {
        if let Some(value) = receipt[key].as_str().filter(|s| s.len() <= 256) {
            compact[key] = json!(value);
        }
    }
    if let Some(version) = receipt["protocolVersion"].as_u64() {
        compact["protocolVersion"] = json!(version);
    }
    if let Some(ok) = receipt["ok"].as_bool() {
        compact["ok"] = json!(ok);
    }
    let result = &receipt["result"];
    let mut evidence = json!({});
    for key in ["mediaType", "mimeType"] {
        if let Some(value) = result[key].as_str().filter(|s| s.len() <= 128) {
            evidence[key] = json!(value);
        }
    }
    for key in ["width", "height", "durationSec"] {
        if result[key].is_number() {
            evidence[key] = result[key].clone();
        }
    }
    if let Some(locator) = result["locator"].as_str() {
        evidence["locatorSha256"] = json!(hash(locator.as_bytes()));
        evidence["transport"] = json!(if locator.starts_with("data:") {
            "inline"
        } else {
            "https"
        });
    }
    compact["result"] = evidence;
    compact
}
fn cancel_correlated(request: &Value, receipt: &Value) -> bool {
    [
        "protocol",
        "protocolVersion",
        "jobId",
        "stepId",
        "attemptId",
        "leaseId",
        "profileId",
    ]
    .iter()
    .all(|key| request.get(key).is_some() && request.get(key) == receipt.get(key))
        && receipt["ok"] == true
        && receipt["requestId"].as_str().is_some_and(|s| !s.is_empty())
        && receipt["result"]["cancelled"] == true
        && receipt["result"]["requestId"] == request["requestId"]
        && receipt["result"]["acknowledgment"]["ok"] == true
}
fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
}
fn cancelled(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        Err("CANCELLED".into())
    } else {
        Ok(())
    }
}
pub(crate) fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub(crate) fn new_id() -> String {
    let mut bytes: [u8; 16] = rand::random();
    bytes[6] = (bytes[6] & 15) | 64;
    bytes[8] = (bytes[8] & 63) | 128;
    let s: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &s[..8],
        &s[8..12],
        &s[12..16],
        &s[16..20],
        &s[20..]
    )
}
fn timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    chrono::DateTime::from_timestamp(seconds as i64, 0)
        .map(|v| v.to_rfc3339())
        .unwrap_or_default()
}
pub(crate) fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let file = fs::File::open(path).map_err(|_| "ARTIFACT_FILE_UNAVAILABLE")?;
    if !file
        .metadata()
        .map_err(|_| "ARTIFACT_FILE_UNAVAILABLE")?
        .is_file()
    {
        return Err("ARTIFACT_NOT_REGULAR_FILE".into());
    }
    let mut bytes = vec![];
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "ARTIFACT_READ_FAILED")?;
    if bytes.is_empty() || bytes.len() as u64 > limit {
        return Err("ARTIFACT_EMPTY_OR_TOO_LARGE".into());
    }
    Ok(bytes)
}
pub(crate) fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|_| "ARTIFACT_WRITE_FAILED")?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "ARTIFACT_WRITE_FAILED")?;
    if hash(&read_bounded(path, MAX_VIDEO)?) != hash(bytes) {
        return Err("ARTIFACT_PERSISTED_HASH_MISMATCH".into());
    }
    Ok(())
}
fn persist(path: &Path, value: &Value) -> Result<(), String> {
    // Write-ahead versioned receipts avoid overwriting a previous durable record.
    let bytes = serde_json::to_vec_pretty(value).map_err(|_| "JOURNAL_INVALID")?;
    let part = path.with_file_name(format!("receipt-{}.json", new_id()));
    write_new(&part, &bytes)?;
    // manifest.json is a convenience copy; immutable receipt-* files remain authoritative.
    fs::write(path, &bytes).map_err(|_| "JOURNAL_WRITE_FAILED".into())
}
struct RecoveryGuard {
    file: fs::File,
    path: PathBuf,
}
impl RecoveryGuard {
    fn acquire(root: &Path, plan: &ProductionPlan, manifest: &Path) -> Result<Self, String> {
        central_recovery_check(root, plan.profile_id())?;
        fs::create_dir_all(root).map_err(|_| "PRODUCTION_PROFILE_GUARD_STORAGE_FAILED")?;
        let path = central_guard_path(root, plan.profile_id())?;
        // create_new is the cross-process atomic fence; OS locking also prevents
        // a concurrent cooperating process from modifying the active receipt.
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| "PRODUCTION_PROFILE_BUSY_OR_UNCERTAIN_REVIEW_RECEIPTS")?;
        file.try_lock()
            .map_err(|_| "PRODUCTION_PROFILE_GUARD_LOCK_FAILED")?;
        let bytes = serde_json::to_vec(&json!({"version":1,"profile_id":plan.profile_id(),"job_id":plan.job_id(),"manifest_path":manifest,"state":"PENDING_UNTIL_CONFIRMED_TERMINAL"}))
            .map_err(|_| "PRODUCTION_PROFILE_GUARD_WRITE_FAILED")?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "PRODUCTION_PROFILE_GUARD_WRITE_FAILED")?;
        Ok(Self { file, path })
    }
    fn clear(self) -> Result<(), String> {
        // Only called for a confirmed terminal or definitely pre-dispatch exit.
        // No Drop cleanup: crash/unknown outcome must survive process restart.
        drop(self.file);
        fs::remove_file(self.path).map_err(|_| "PRODUCTION_PROFILE_GUARD_CLEAR_UNCONFIRMED".into())
    }
}
fn central_guard_path(root: &Path, profile: &str) -> Result<PathBuf, String> {
    if !root.is_absolute() || !identifier(profile) {
        return Err("PRODUCTION_PROFILE_GUARD_CONFIGURATION_INVALID".into());
    }
    if fs::symlink_metadata(root).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err("PRODUCTION_PROFILE_GUARD_SYMLINK_FORBIDDEN".into());
    }
    Ok(root.join(format!("profile-{}.json", hash(profile.as_bytes()))))
}
fn central_recovery_check(root: &Path, profile: &str) -> Result<(), String> {
    let path = central_guard_path(root, profile)?;
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("PRODUCTION_PROFILE_GUARD_READ_FAILED".into()),
        Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() => {
            return Err("PRODUCTION_PROFILE_GUARD_REVIEW_REQUIRED".into());
        }
        Ok(_) => {}
    }
    let bytes =
        read_bounded(&path, 64 * 1024).map_err(|_| "PRODUCTION_PROFILE_GUARD_REVIEW_REQUIRED")?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|_| "PRODUCTION_PROFILE_GUARD_REVIEW_REQUIRED")?;
    if value["version"] != 1
        || value["profile_id"] != profile
        || value["job_id"].as_str().is_none()
        || value["manifest_path"].as_str().is_none()
    {
        return Err("PRODUCTION_PROFILE_GUARD_REVIEW_REQUIRED".into());
    }
    Err("PREVIOUS_SUBMISSION_UNKNOWN_OR_RUNNING_RECONCILE_PROFILE_RECEIPTS".into())
}
fn recovery_gate(plan: &ProductionPlan) -> Result<(), String> {
    let root = PathBuf::from(&plan.page.output_root).join("concat-production");
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("PRODUCTION_RECOVERY_SCAN_FAILED".into()),
    };
    for (index, entry) in entries.enumerate() {
        if index >= 1000 {
            return Err("PRODUCTION_RECOVERY_SCAN_LIMIT_REVIEW_REQUIRED".into());
        }
        let entry = entry.map_err(|_| "PRODUCTION_RECOVERY_SCAN_FAILED")?;
        let metadata = entry
            .file_type()
            .map_err(|_| "PRODUCTION_RECOVERY_SCAN_FAILED")?;
        if metadata.is_symlink() {
            return Err("PRODUCTION_RECOVERY_SYMLINK_REVIEW_REQUIRED".into());
        }
        if !metadata.is_dir() {
            continue;
        }
        let path = entry.path().join("manifest.json");
        // A directory created before a crash but without its manifest is not
        // silently assumed safe: review its immutable receipt files first.
        let bytes = read_bounded(&path, 4 * 1024 * 1024)
            .map_err(|_| "PRODUCTION_RECOVERY_MANIFEST_REVIEW_REQUIRED")?;
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| "PRODUCTION_RECOVERY_MANIFEST_REVIEW_REQUIRED")?;
        let profile = value
            .pointer("/plan/page/browser_profile_id")
            .and_then(Value::as_str)
            .ok_or("PRODUCTION_RECOVERY_MANIFEST_REVIEW_REQUIRED")?;
        if profile == plan.profile_id()
            && (value["state"] == "UNKNOWN_OUTCOME"
                || value
                    .get("pending_submission")
                    .is_some_and(|v| !v.is_null()))
        {
            return Err("PREVIOUS_SUBMISSION_UNKNOWN_RECONCILE_RECEIPTS_BEFORE_RUN".into());
        }
    }
    Ok(())
}
fn artifact_url(locator: &str) -> Result<(), String> {
    let url = url::Url::parse(locator).map_err(|_| "ARTIFACT_LOCATOR_FORBIDDEN")?;
    let host = url.host_str().unwrap_or("");
    let allowed = ["grok.com", "x.ai", "twimg.com", "twitter.com"]
        .iter()
        .any(|base| host == *base || host.ends_with(&format!(".{base}")));
    if url.scheme() != "https"
        || !allowed
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
    {
        return Err("ARTIFACT_LOCATOR_FORBIDDEN".into());
    }
    Ok(())
}
fn image_mime(bytes: &[u8]) -> Result<(&'static str, &'static str), String> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Ok(("image/png", "png"))
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Ok(("image/jpeg", "jpg"))
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Ok(("image/webp", "webp"))
    } else {
        Err("SOURCE_ARTIFACT_INVALID_MIME".into())
    }
}
fn reject_symlink(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors() {
        let meta = fs::symlink_metadata(ancestor).map_err(|_| "LOCAL_ARTIFACT_PATH_UNAVAILABLE")?;
        if meta.file_type().is_symlink() {
            return Err("LOCAL_ARTIFACT_SYMLINK_FORBIDDEN".into());
        }
    }
    Ok(())
}
fn validate_source_image(bytes: &[u8]) -> Result<(), String> {
    image_mime(bytes)?;
    let (w, h) = dimensions(bytes)?;
    if w == 0 || h == 0 || w > 32768 || h > 32768 || u64::from(w) * u64::from(h) > 100_000_000 {
        return Err("SOURCE_IMAGE_DIMENSIONS_INVALID".into());
    }
    Ok(())
}
fn video_mime(bytes: &[u8]) -> Result<(&'static str, &'static str), String> {
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        Ok(("video/mp4", "mp4"))
    } else if bytes.starts_with(b"\x1a\x45\xdf\xa3") {
        Ok(("video/webm", "webm"))
    } else {
        Err("VIDEO_ARTIFACT_INVALID_MIME".into())
    }
}
fn dimensions(bytes: &[u8]) -> Result<(u32, u32), String> {
    if bytes.len() >= 24 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") && &bytes[12..16] == b"IHDR" {
        return Ok((
            u32::from_be_bytes(bytes[16..20].try_into().unwrap()),
            u32::from_be_bytes(bytes[20..24].try_into().unwrap()),
        ));
    }
    if bytes.starts_with(b"\xff\xd8") {
        let mut i = 2;
        while i + 9 < bytes.len() {
            if bytes[i] != 255 {
                i += 1;
                continue;
            }
            let marker = bytes[i + 1];
            if matches!(marker,0xc0..=0xc3|0xc5..=0xc7|0xc9..=0xcb|0xcd..=0xcf) {
                return Ok((
                    u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32,
                    u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32,
                ));
            }
            if matches!(marker, 0xd8 | 0xd9 | 0x01 | 0xd0..=0xd7) {
                i += 2;
                continue;
            }
            let n = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
            if n < 2 {
                break;
            }
            i += 2 + n;
        }
    }
    // ArtCraft's canonical WebP dimension formats: lossy, lossless, extended.
    if bytes.len() >= 30 && &bytes[..4] == b"RIFF" && &bytes[8..16] == b"WEBPVP8 " {
        let width = u32::from(u16::from_le_bytes([bytes[26], bytes[27]]) & 0x3fff);
        let height = u32::from(u16::from_le_bytes([bytes[28], bytes[29]]) & 0x3fff);
        if width > 0 && height > 0 {
            return Ok((width, height));
        }
    }
    if bytes.len() >= 25 && &bytes[..4] == b"RIFF" && &bytes[8..16] == b"WEBPVP8L" {
        let b1 = u32::from(bytes[21]);
        let b2 = u32::from(bytes[22]);
        let b3 = u32::from(bytes[23]);
        let b4 = u32::from(bytes[24]);
        return Ok((
            1 + (((b2 & 0x3f) << 8) | b1),
            1 + (((b4 & 0x0f) << 10) | (b3 << 2) | ((b2 & 0xc0) >> 6)),
        ));
    }
    if bytes.len() >= 30 && &bytes[..4] == b"RIFF" && &bytes[8..16] == b"WEBPVP8X" {
        let read = |offset: usize| {
            1 + u32::from(bytes[offset])
                + (u32::from(bytes[offset + 1]) << 8)
                + (u32::from(bytes[offset + 2]) << 16)
        };
        return Ok((read(24), read(27)));
    }
    Err("IMAGE_DIMENSIONS_UNSUPPORTED_OR_INVALID".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::Mutex;
    struct Fixture {
        root: PathBuf,
        origin: String,
        requests: Arc<Mutex<Vec<(String, Value)>>>,
        done: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }
    impl Fixture {
        fn new(wrong_correlation: bool, cancel_after_acquire: Option<Arc<AtomicBool>>) -> Self {
            Self::mode(if wrong_correlation { 1 } else { 0 }, cancel_after_acquire)
        }
        fn mode(mode: u8, cancel_after_acquire: Option<Arc<AtomicBool>>) -> Self {
            let root =
                std::env::temp_dir().join(format!("concat-production-http-test-{}", new_id()));
            fs::create_dir(&root).unwrap();
            fs::write(root.join("anchor.png"), b"\x89PNG\r\n\x1a\nanchor").unwrap();
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let origin = format!("http://{}", listener.local_addr().unwrap());
            let requests = Arc::new(Mutex::new(vec![]));
            let log = requests.clone();
            let done = Arc::new(AtomicBool::new(false));
            let stop = done.clone();
            let thread = std::thread::spawn(move || {
                while !stop.load(Ordering::Acquire) {
                    let (mut stream, _) = match listener.accept() {
                        Ok(v) => v,
                        Err(_) => {
                            std::thread::sleep(Duration::from_millis(5));
                            continue;
                        }
                    };
                    // Accepted Windows sockets may inherit nonblocking mode;
                    // WouldBlock is not simulated transport loss in these fixtures.
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(3)))
                        .unwrap();
                    let mut bytes = Vec::new();
                    let mut buf = [0u8; 4096];
                    let split = loop {
                        let n = stream.read(&mut buf).unwrap_or(0);
                        if n == 0 {
                            break None;
                        }
                        bytes.extend_from_slice(&buf[..n]);
                        if let Some(i) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                            break Some(i + 4);
                        }
                        if bytes.len() > MAX_IMAGE as usize * 2 {
                            break None;
                        }
                    };
                    let Some(split) = split else {
                        continue;
                    };
                    let header = String::from_utf8_lossy(&bytes[..split]).to_string();
                    let length = header
                        .lines()
                        .find_map(|s| {
                            s.split_once(':')
                                .filter(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                                .and_then(|(_, v)| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    while bytes.len() < split + length {
                        let n = stream.read(&mut buf).unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        bytes.extend_from_slice(&buf[..n]);
                    }
                    let path = header
                        .lines()
                        .next()
                        .unwrap()
                        .split_whitespace()
                        .nth(1)
                        .unwrap()
                        .to_string();
                    let body =
                        serde_json::from_slice::<Value>(&bytes[split..]).unwrap_or(Value::Null);
                    log.lock().unwrap().push((path.clone(), body.clone()));
                    if mode == 2 && path.ends_with("/dispatch") {
                        // A provider can accept the request then drop transport.
                        // The client must quarantine, not submit a second time.
                        drop(stream);
                        continue;
                    }
                    let response = if path == "/v1/runtime/health" {
                        json!({"protocol":"floword-production","protocolVersion":1,"runtime":"nexorabrowser","status":"READY"})
                    } else if path == "/v1/local/browser/profiles" {
                        json!({"profiles":[{"id":"profile-a","name":"Profile A","is_running":true,"process_id":1,"last_launch":1}],"total":1})
                    } else if path == "/v1/workers/acquire" {
                        if let Some(flag) = &cancel_after_acquire {
                            flag.store(true, Ordering::Release);
                        }
                        json!({"lease_id":"lease-a","worker_id":if mode==4{"playwright-profile:profile-a"}else{"worker-a"},"profile_id":"profile-a","expires_at":"2099-01-01T00:00:00Z"})
                    } else if path.ends_with("/dispatch") {
                        let mut reply = body.clone();
                        reply.as_object_mut().unwrap().remove("params");
                        reply["ok"] = json!(true);
                        if mode == 1 {
                            reply["requestId"] = json!("different-request");
                        }
                        let mut png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
                        png.extend_from_slice(&1080u32.to_be_bytes());
                        png.extend_from_slice(&1920u32.to_be_bytes());
                        let (mime, data) = if body["method"] == "grok.video.generate" {
                            ("video/mp4", b"\x00\x00\x00\x18ftypmp42fixture".to_vec())
                        } else {
                            if body["method"] == "grok.image.edit" {
                                png.extend_from_slice(b"edited-image-fixture");
                            }
                            ("image/png", png)
                        };
                        reply["result"] = json!({"mediaType":if mime.starts_with("video"){"video"}else{"image"},"source":"data","locator":format!("data:{mime};base64,{}",STANDARD.encode(data)),"mimeType":mime});
                        reply
                    } else if path.ends_with("/heartbeat") {
                        json!({"lease_id":"lease-a","status":"ACTIVE","expires_at":"2099-01-01T00:00:00Z"})
                    } else if path.ends_with("/release") {
                        json!({"lease_id":"lease-a","status":if mode==3 {"ACTIVE"} else {"RELEASED"}})
                    } else {
                        json!({"ok":false})
                    };
                    let body = serde_json::to_vec(&response).unwrap();
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(header.as_bytes());
                    let _ = stream.write_all(&body);
                }
            });
            Self {
                root,
                origin,
                requests,
                done,
                thread: Some(thread),
            }
        }
        fn plan(&self) -> ProductionPlan {
            ProductionPlan::new(page(&self.root), self.root.join("anchor.png"), "", "", "").unwrap()
        }
        fn client(&self) -> ProductionClient {
            ProductionClient::new(BrowserClient::new(&self.origin).unwrap())
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.done.store(true, Ordering::Release);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            } // Only the unique, test-created fixture directory is removed.
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    #[test]
    fn prompt_only_image_feeds_verified_image_into_expand_and_video() {
        let f = Fixture::new(false, None);
        let plan = ProductionPlan::from_prompt(
            page(&f.root), "A red bag on a wooden table", "Expand to 9:16", "Slow orbit around the bag",
        ).unwrap();
        let output = f.client().run(plan, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        output.verify_bytes().unwrap();
        let requests = f.requests.lock().unwrap();
        let dispatches: Vec<_> = requests.iter().filter(|(p, _)| p.ends_with("/dispatch")).collect();
        assert_eq!(dispatches.len(), 3);
        assert_eq!(dispatches[0].1["method"], "grok.image.edit");
        assert_eq!(dispatches[0].1["params"]["prompt"], "A red bag on a wooden table");
        assert!(dispatches[0].1["params"].get("sourceArtifact").is_none());
        let manifest: Value = serde_json::from_slice(&fs::read(&output.manifest_path).unwrap()).unwrap();
        for i in 1..3 {
            let previous = fs::read(manifest["stages"][i-1]["path"].as_str().unwrap()).unwrap();
            assert_eq!(dispatches[i].1["params"]["sourceArtifact"]["dataUrl"], format!("data:image/png;base64,{}", STANDARD.encode(previous)));
        }
        assert!(!output.manifest_path.parent().unwrap().join("source-image.bin").exists());
    }
    #[test]
    fn canonical_playwright_worker_id_runs_the_real_http_stage_contract() {
        let f=Fixture::mode(4,None);
        let output=f.client().run(f.plan(),Arc::new(AtomicBool::new(false)), |_|{}).unwrap();
        output.verify_bytes().unwrap();
        let requests=f.requests.lock().unwrap();
        assert_eq!(requests.iter().filter(|(p,_)|p=="/v1/workers/playwright-profile:profile-a/dispatch").count(),3);
    }
    #[test]
    fn actual_http_three_stage_contract_and_byte_receipt() {
        let f = Fixture::new(false, None);
        let mut stages = vec![];
        let output = f
            .client()
            .run(f.plan(), Arc::new(AtomicBool::new(false)), |p| {
                if p.state == "SUCCEEDED" {
                    stages.push(p.stage);
                }
            })
            .unwrap();
        output.verify_bytes().unwrap();
        assert_eq!(
            stages,
            vec![
                ProductionStage::ImageEdit,
                ProductionStage::Expand916,
                ProductionStage::VideoGenerate,
                ProductionStage::LocalSave
            ]
        );
        let requests = f.requests.lock().unwrap();
        let dispatches: Vec<_> = requests
            .iter()
            .filter(|(path, _)| path.ends_with("/dispatch"))
            .collect();
        assert_eq!(dispatches.len(), 3);
        for (index, (_, body)) in dispatches.iter().enumerate() {
            assert_eq!(body["protocol"], "floword-production");
            assert_eq!(body["protocolVersion"], 1);
            assert_eq!(body["profileId"], "profile-a");
            assert_eq!(body["pageId"], "page-a");
            assert_eq!(body["leaseId"], "lease-a");
            assert!(
                body["params"]["sourceArtifact"]["dataUrl"]
                    .as_str()
                    .unwrap()
                    .starts_with("data:image/png;base64,")
            );
            if index == 1 {
                assert_eq!(body["params"]["targetAspectRatio"], "9:16");
            }
        }
        assert_eq!(
            requests
                .iter()
                .filter(|(p, _)| p.ends_with("/release"))
                .count(),
            3
        );
        let manifest: Value =
            serde_json::from_slice(&fs::read(&output.manifest_path).unwrap()).unwrap();
        assert_eq!(manifest["state"], "AWAITING_NATIVE_MEDIA_PROBE");
    }
    #[test]
    fn native_seven_stage_executor_consumes_central_commands_and_local_artifacts_once() {
        use crate::{
            native_pipeline::{self, Pipeline, Services},
            promax_worker::{ProductionHandler, WorkerIdentity},
            storyboard::{Shot, Storyboard},
        };
        struct LocalFixtureServices;
        impl Services for LocalFixtureServices {
            fn sources(&self) -> Result<(), String> {
                Ok(())
            }
            fn script(&self) -> Result<Storyboard, String> {
                Ok(Storyboard {
                    shots: vec![Shot::new("fixture narration", 1.0)],
                    ..Storyboard::default()
                })
            }
            fn voice(&self, _: &mut Storyboard, _: &Path, _: &AtomicBool) -> Result<(), String> {
                Ok(())
            }
            fn render(
                &self,
                board: &Storyboard,
                root: &Path,
                _: &AtomicBool,
            ) -> Result<PathBuf, String> {
                assert!(board.shots[0].generated.as_ref().unwrap().is_file());
                let p = root.join("fixture-not-a-decoded-video.mp4");
                fs::write(&p, b"\0\0\0\x18ftypmp42fixture").unwrap();
                Ok(p)
            }
            fn verify_video(&self, path: &Path) -> Result<(), String> {
                assert!(path.is_file());
                Ok(())
            }
            fn progress(&self, _: usize, _: usize, _: usize) {}
            fn completed(&self, board: Storyboard, video: PathBuf) {
                assert!(board.output.is_some());
                assert!(video.is_file());
            }
            fn failed(&self, e: &str) {
                panic!("coordinator fixture failed: {e}")
            }
        }
        // Providers and decoder are explicit fixtures here. Real MP4 decoding
        // and assembly are covered by storyboard_render's FFmpeg integration.
        let f = Fixture::new(false, None);
        let _ = strict_fixture_plan(&f);
        let identity = WorkerIdentity {
            client_id: "concat-local".into(),
            device_id: "device-a".into(),
            tenant_id: "tenant-a".into(),
            user_id: "user-a".into(),
        };
        let graph = native_pipeline::definition();
        let pipeline = Pipeline::reserve(
            identity.clone(),
            graph.clone(),
            &f.root,
            &f.root.join("global-profile-guards"),
            page(&f.root),
            f.root.join("anchor.png"),
            "image".into(),
            "expand".into(),
            "video".into(),
            BrowserClient::new(&f.origin).unwrap(),
            Arc::new(LocalFixtureServices),
        )
        .unwrap();
        let nodes = graph["nodes"].as_array().unwrap();
        for (i, node) in nodes.iter().enumerate() {
            let command = json!({"commandId":format!("command-{i}"),"runId":"run-a","workflowId":native_pipeline::WORKFLOW,"stepId":node["nodeId"],"stageId":node["nodeType"],"inputPayload":{"nodeType":node["nodeType"],"_tenantId":identity.tenant_id,"_ownerId":identity.user_id,"_workflowVersion":graph["version"],"_workflowDigest":native_pipeline::digest(&graph).unwrap(),"operation":node["config"]["operation"],"production_settings":{"nativeReservationId":pipeline.id(),"workerTarget":pipeline.target(),"browserProfileId":"profile-a"}}});
            let mut wrong = command.clone();
            wrong["inputPayload"]["_workflowDigest"] = json!("changed");
            assert!(pipeline.validate(&wrong).is_err());
            let mut wrong = command.clone();
            wrong["inputPayload"]["production_settings"]["browserProfileId"] =
                json!("another-profile");
            assert!(pipeline.validate(&wrong).is_err());
            pipeline.validate(&command).unwrap();
            let receipt = pipeline
                .execute(command.clone(), Arc::new(AtomicBool::new(false)))
                .unwrap();
            crate::promax_worker_ledger::validate_receipt(&receipt).unwrap();
            assert!(receipt["output"]["resourceId"].is_string());
            assert!(
                pipeline.validate(&command).is_err(),
                "duplicate stage must never execute twice"
            );
        }
        assert!(pipeline.finished());
        assert_eq!(
            f.requests
                .lock()
                .unwrap()
                .iter()
                .filter(|(p, _)| p.ends_with("/dispatch"))
                .count(),
            3
        );
    }
    fn strict_fixture_plan(f: &Fixture) -> ProductionPlan {
        let mut png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
        png.extend_from_slice(&1080u32.to_be_bytes());
        png.extend_from_slice(&1920u32.to_be_bytes());
        png.extend_from_slice(b"reserved-original-fixture");
        fs::write(f.root.join("anchor.png"), png).unwrap();
        f.plan()
    }
    #[test]
    fn single_stage_actual_http_dispatches_once_and_duplicate_never_resubmits() {
        let f = Fixture::new(false, None);
        let plan = strict_fixture_plan(&f);
        let client = f.client().with_recovery_root(f.root.join("profile-guards"));
        let workspace = f.root.join("one-stage");
        let output = client
            .run_stage(
                &plan,
                ProductionStage::ImageEdit,
                plan.anchor_path(),
                plan.anchor_sha256(),
                &workspace,
                Arc::new(AtomicBool::new(false)),
                |_| {},
            )
            .unwrap();
        output.verify_bytes().unwrap();
        assert_eq!(output.stage, ProductionStage::ImageEdit);
        let requests = f.requests.lock().unwrap();
        let dispatches: Vec<_> = requests
            .iter()
            .filter(|(path, _)| path.ends_with("/dispatch"))
            .collect();
        assert_eq!(dispatches.len(), 1);
        assert_eq!(dispatches[0].1["method"], "grok.image.edit");
        drop(requests);
        assert!(
            client
                .run_stage(
                    &plan,
                    ProductionStage::ImageEdit,
                    plan.anchor_path(),
                    plan.anchor_sha256(),
                    &workspace,
                    Arc::new(AtomicBool::new(false)),
                    |_| {}
                )
                .is_err()
        );
        assert_eq!(
            f.requests
                .lock()
                .unwrap()
                .iter()
                .filter(|(path, _)| path.ends_with("/dispatch"))
                .count(),
            1
        );
        let manifest: Value =
            serde_json::from_slice(&fs::read(output.manifest_path).unwrap()).unwrap();
        assert_eq!(manifest["stages"].as_array().unwrap().len(), 1);
    }
    #[test]
    fn single_stage_rejects_source_hash_and_unknown_keeps_profile_fence() {
        let f = Fixture::mode(2, None);
        let plan = strict_fixture_plan(&f);
        let recovery = f.root.join("profile-guards");
        let client = f.client().with_recovery_root(recovery.clone());
        assert!(
            client
                .run_stage(
                    &plan,
                    ProductionStage::ImageEdit,
                    plan.anchor_path(),
                    &"0".repeat(64),
                    &f.root.join("wrong-hash"),
                    Arc::new(AtomicBool::new(false)),
                    |_| {}
                )
                .is_err()
        );
        assert!(f.requests.lock().unwrap().is_empty());
        assert!(
            client
                .run_stage(
                    &plan,
                    ProductionStage::ImageEdit,
                    plan.anchor_path(),
                    plan.anchor_sha256(),
                    &f.root.join("unknown"),
                    Arc::new(AtomicBool::new(false)),
                    |_| {}
                )
                .unwrap_err()
                .contains("UNKNOWN")
        );
        assert!(central_recovery_check(&recovery, plan.profile_id()).is_err());
        assert!(
            client
                .run_stage(
                    &plan,
                    ProductionStage::ImageEdit,
                    plan.anchor_path(),
                    plan.anchor_sha256(),
                    &f.root.join("retry-elsewhere"),
                    Arc::new(AtomicBool::new(false)),
                    |_| {}
                )
                .is_err()
        );
        assert_eq!(
            f.requests
                .lock()
                .unwrap()
                .iter()
                .filter(|(path, _)| path.ends_with("/dispatch"))
                .count(),
            1
        );
    }
    #[test]
    fn reserved_central_commands_are_ordered_and_each_dispatch_is_single_stage() {
        use crate::production_worker::{
            ProductionCommand, ProductionOwner, ProductionReservation, WorkflowBinding,
        };
        let f = Fixture::new(false, None);
        let plan = strict_fixture_plan(&f);
        let mut reservation = ProductionReservation::reserve(
            &f.root,
            ProductionOwner {
                tenant_id: "tenant-a".into(),
                user_id: "user-a".into(),
                device_id: "device-a".into(),
            },
            WorkflowBinding {
                workflow_id: "workflow-a".into(),
                version: "1.0.0".into(),
            },
            plan,
        )
        .unwrap();
        let client = f.client().with_recovery_root(f.root.join("profile-guards"));
        let mut command = ProductionCommand {
            reservation_id: reservation.id().into(),
            owner: reservation.owner().clone(),
            workflow: reservation.workflow().clone(),
            stage: ProductionStage::ImageEdit,
            input_artifact_handle: reservation.source_handle().into(),
            expected_sha256: reservation.source_sha256().into(),
        };
        for (index, stage) in [
            ProductionStage::ImageEdit,
            ProductionStage::Expand916,
            ProductionStage::VideoGenerate,
        ]
        .into_iter()
        .enumerate()
        {
            command.stage = stage;
            let receipt = reservation
                .execute(&client, &command, Arc::new(AtomicBool::new(false)), |_| {})
                .unwrap();
            assert_eq!(
                f.requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|(path, _)| path.ends_with("/dispatch"))
                    .count(),
                index + 1
            );
            assert!(
                reservation
                    .execute(&client, &command, Arc::new(AtomicBool::new(false)), |_| {})
                    .is_err()
            );
            command.input_artifact_handle = receipt.artifact_handle;
            command.expected_sha256 = receipt.sha256;
        }
        assert_eq!(
            reservation.final_artifact_for_probe().unwrap().mime_type,
            "video/mp4"
        );
    }
    #[test]
    fn actual_http_correlation_failure_retains_lease_and_blocks_restart() {
        let f = Fixture::new(true, None);
        let error = f
            .client()
            .run(f.plan(), Arc::new(AtomicBool::new(false)), |_| {})
            .unwrap_err();
        assert!(error.contains("CORRELATION_MISMATCH_UNKNOWN_OUTCOME"));
        assert!(
            !f.requests
                .lock()
                .unwrap()
                .iter()
                .any(|(p, _)| p.ends_with("/release"))
        );
        assert_eq!(
            f.client().preflight(&f.plan()).unwrap_err(),
            "PREVIOUS_SUBMISSION_UNKNOWN_RECONCILE_RECEIPTS_BEFORE_RUN"
        );
    }
    #[test]
    fn cancel_during_acquire_never_dispatches() {
        let cancel = Arc::new(AtomicBool::new(false));
        let f = Fixture::new(false, Some(cancel.clone()));
        assert_eq!(
            f.client().run(f.plan(), cancel, |_| {}).unwrap_err(),
            "CANCELLED"
        );
        let requests = f.requests.lock().unwrap();
        assert!(!requests.iter().any(|(p, _)| p.ends_with("/dispatch")));
        assert!(requests.iter().any(|(p, _)| p.ends_with("/release")));
    }
    #[test]
    fn corrupt_manifest_fails_closed_on_restart() {
        let f = Fixture::new(false, None);
        let path = f.root.join("concat-production").join("old-job");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("manifest.json"), b"broken").unwrap();
        assert_eq!(
            f.client().preflight(&f.plan()).unwrap_err(),
            "PRODUCTION_RECOVERY_MANIFEST_REVIEW_REQUIRED"
        );
        assert!(f.requests.lock().unwrap().is_empty());
    }
    #[test]
    fn actual_http_transport_loss_never_retries_or_fabricates_output() {
        let f = Fixture::mode(2, None);
        let error = f
            .client()
            .run(f.plan(), Arc::new(AtomicBool::new(false)), |_| {})
            .unwrap_err();
        assert_eq!(error, "DISPATCH_UNKNOWN_OUTCOME");
        let requests = f.requests.lock().unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|(p, _)| p.ends_with("/dispatch"))
                .count(),
            1
        );
        assert!(!requests.iter().any(|(p, _)| p.ends_with("/release")));
        drop(requests);
        let directory = fs::read_dir(f.root.join("concat-production"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let manifest: Value =
            serde_json::from_slice(&fs::read(directory.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest["state"], "UNKNOWN_OUTCOME");
        assert!(manifest.get("output").is_none());
    }
    #[test]
    fn central_unknown_fence_survives_other_page_and_changed_output_root() {
        let f = Fixture::mode(2, None);
        let recovery = f.root.join("config-recovery");
        let client = f.client().with_recovery_root(recovery.clone());
        assert_eq!(
            client
                .run(f.plan(), Arc::new(AtomicBool::new(false)), |_| {})
                .unwrap_err(),
            "DISPATCH_UNKNOWN_OUTCOME"
        );
        let mut changed = page(&f.root.join("different-output-root"));
        changed.id = "page-other".into();
        changed.name = "Another Page same Nexora profile".into();
        let plan = ProductionPlan::new(changed, f.root.join("anchor.png"), "", "", "").unwrap();
        // A fresh client models app restart; no in-memory state is reused.
        let restarted = f.client().with_recovery_root(recovery);
        assert_eq!(
            restarted.preflight(&plan).unwrap_err(),
            "PREVIOUS_SUBMISSION_UNKNOWN_OR_RUNNING_RECONCILE_PROFILE_RECEIPTS"
        );
        assert_eq!(
            f.requests
                .lock()
                .unwrap()
                .iter()
                .filter(|(path, _)| path.ends_with("/dispatch"))
                .count(),
            1
        );
    }
    #[test]
    fn central_profile_guard_is_atomic_across_two_clients() {
        let f = Fixture::new(false, None);
        let recovery = f.root.join("config-recovery");
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let mut threads = vec![];
        for _ in 0..2 {
            let barrier = barrier.clone();
            let root = recovery.clone();
            let plan = f.plan();
            threads.push(std::thread::spawn(move || {
                barrier.wait();
                let result = RecoveryGuard::acquire(
                    &root,
                    &plan,
                    &PathBuf::from(&plan.page.output_root).join("manifest.json"),
                );
                barrier.wait(); // Winner remains locked until both attempts finish.
                result.is_ok()
            }));
        }
        let wins = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .filter(|won| *won)
            .count();
        assert_eq!(wins, 1);
        assert_eq!(
            central_recovery_check(&recovery, "profile-a").unwrap_err(),
            "PREVIOUS_SUBMISSION_UNKNOWN_OR_RUNNING_RECONCILE_PROFILE_RECEIPTS"
        );
    }
    #[test]
    fn confirmed_success_clears_central_profile_guard() {
        let f = Fixture::new(false, None);
        let recovery = f.root.join("config-recovery");
        let client = f.client().with_recovery_root(recovery.clone());
        client
            .run(f.plan(), Arc::new(AtomicBool::new(false)), |_| {})
            .unwrap();
        central_recovery_check(&recovery, "profile-a").unwrap();
    }
    #[test]
    fn ambiguous_release_retains_central_guard_and_never_returns_video() {
        let f = Fixture::mode(3, None);
        let recovery = f.root.join("config-recovery");
        let client = f.client().with_recovery_root(recovery.clone());
        assert_eq!(
            client
                .run(f.plan(), Arc::new(AtomicBool::new(false)), |_| {})
                .unwrap_err(),
            "LEASE_RELEASE_UNCONFIRMED_UNKNOWN_OUTCOME"
        );
        assert_eq!(
            central_recovery_check(&recovery, "profile-a").unwrap_err(),
            "PREVIOUS_SUBMISSION_UNKNOWN_OR_RUNNING_RECONCILE_PROFILE_RECEIPTS"
        );
        let directory = fs::read_dir(f.root.join("concat-production"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let manifest: Value =
            serde_json::from_slice(&fs::read(directory.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest["state"], "UNKNOWN_OUTCOME");
        assert!(manifest.get("output").is_none());
    }
    fn page(root: &Path) -> ContentPage {
        serde_json::from_value(json!({
            "id":"page-a","name":"Page A","slug":"page-a","output_root":root,
            "browser_profile_id":"profile-a","default_image_prompt":"Page image preset",
            "default_expand_9_16_prompt":"Page expand preset","default_video_prompt":"Page video preset",
            "target_platform":"tiktok","default_model_id":null,"default_workflow_id":null,
            "default_language":"vi","default_tone":null,"default_aspect_ratio":"9:16",
            "worker_pool_id":null,"is_archived":false,"created_at":1,"updated_at":1,"revision":1
        })).unwrap()
    }
    #[test]
    fn page_snapshot_and_source_are_immutable() {
        let root = std::env::temp_dir().join(format!("concat-production-test-{}", new_id()));
        fs::create_dir(&root).unwrap();
        let anchor = root.join("anchor.png");
        fs::write(&anchor, b"\x89PNG\r\n\x1a\nfixture").unwrap();
        let mut selected = page(&root);
        let plan = ProductionPlan::new(selected.clone(), anchor.clone(), "", "", "").unwrap();
        selected.browser_profile_id = Some("profile-b".into());
        selected.output_root = root.join("different-output").to_string_lossy().into_owned();
        assert_eq!(plan.profile_id(), "profile-a");
        assert_ne!(plan.page().output_root, selected.output_root);
        assert_eq!(plan.image_prompt, "Page image preset");
        assert_eq!(plan.expand_prompt, "Page expand preset");
        assert_eq!(plan.video_prompt, "Page video preset");
        fs::write(&anchor, b"\x89PNG\r\n\x1a\nmodified").unwrap();
        assert_eq!(
            plan.source().unwrap_err(),
            "SOURCE_IMAGE_CHANGED_RESELECT_REQUIRED"
        );
        fs::remove_file(&anchor).unwrap();
        fs::remove_dir(&root).unwrap();
    }
    #[test]
    fn missing_source_and_wrong_aspect_fail_before_network() {
        let root = std::env::temp_dir();
        let mut selected = page(&root);
        selected.default_aspect_ratio = Some("16:9".into());
        assert_eq!(
            ProductionPlan::new(selected, root.join("not-used"), "", "", "").unwrap_err(),
            "GROK_PIPELINE_REQUIRES_9_16"
        );
        assert!(ProductionPlan::new(page(&root), root.join(new_id()), "", "", "").is_err());
    }
    #[test]
    fn receipt_must_match_all_correlations() {
        let request = json!({"protocol":"floword-production","protocolVersion":1,"requestId":"a","jobId":"b","stepId":"c","attemptId":"1","leaseId":"l","profileId":"p"});
        assert!(correlate(&request, &request).is_ok());
        for key in [
            "protocol",
            "protocolVersion",
            "requestId",
            "jobId",
            "stepId",
            "attemptId",
            "leaseId",
            "profileId",
        ] {
            let mut bad = request.clone();
            bad[key] = Value::Null;
            assert!(correlate(&request, &bad).is_err(), "{key}");
        }
        let mut cancel = request.clone();
        cancel["requestId"] = json!("CANCEL_fixture");
        cancel["ok"] = json!(true);
        cancel["result"] = json!({"cancelled":true,"requestId":"a","acknowledgment":{"ok":true}});
        assert!(cancel_correlated(&request, &cancel));
        cancel["result"]["requestId"] = json!("another-request");
        assert!(!cancel_correlated(&request, &cancel));
    }
    #[test]
    fn artifact_origin_policy_has_no_arbitrary_downloads() {
        for bad in [
            "http://grok.com/a",
            "https://evil.test/a",
            "https://grok.com.evil.test/a",
            "https://127.0.0.1/a",
            "https://u:p@grok.com/a",
            "https://grok.com:444/a",
        ] {
            assert!(artifact_url(bad).is_err());
        }
        assert!(artifact_url("https://assets.grok.com/a").is_ok());
        assert!(artifact_url("https://imagine-public.x.ai/a").is_ok());
    }
    #[test]
    fn stages_preserve_existing_contract_names() {
        assert_eq!(ProductionStage::ImageEdit.method(), "grok.image.edit");
        assert_eq!(
            ProductionStage::Expand916.method(),
            "grok.image.expand_9_16"
        );
        assert_eq!(
            ProductionStage::VideoGenerate.method(),
            "grok.video.generate"
        );
        assert_eq!(ProductionStage::Expand916.as_str(), "CONVERTING_9_16");
    }
    #[test]
    fn invalid_bytes_never_become_video() {
        assert!(video_mime(b"<html>error</html>").is_err());
        assert!(image_mime(b"notanimage").is_err());
        assert!(dimensions(b"truncated").is_err());
        let mut lossy = vec![0u8; 30];
        lossy[..4].copy_from_slice(b"RIFF");
        lossy[8..16].copy_from_slice(b"WEBPVP8 ");
        lossy[26..28].copy_from_slice(&1080u16.to_le_bytes());
        lossy[28..30].copy_from_slice(&1920u16.to_le_bytes());
        assert_eq!(dimensions(&lossy).unwrap(), (1080, 1920));
        let mut lossless = vec![0u8; 25];
        lossless[..4].copy_from_slice(b"RIFF");
        lossless[8..16].copy_from_slice(b"WEBPVP8L");
        let (w, h) = (1079u32, 1919u32);
        lossless[21] = (w & 255) as u8;
        lossless[22] = (((w >> 8) & 63) | ((h & 3) << 6)) as u8;
        lossless[23] = ((h >> 2) & 255) as u8;
        lossless[24] = ((h >> 10) & 15) as u8;
        assert_eq!(dimensions(&lossless).unwrap(), (1080, 1920));
    }
    #[test]
    fn durable_receipt_never_duplicates_large_media_or_signed_urls() {
        let mut receipt = json!({
            "protocol":"floword-production","protocolVersion":1,
            "requestId":"request-a","jobId":"job-a","stepId":"GENERATING_VIDEO",
            "attemptId":"1","leaseId":"lease-a","profileId":"profile-a","ok":true,
            "result":{"mimeType":"video/mp4","mediaType":"video","width":1080,"height":1920,
                "durationSec":5,"locator":format!("data:video/mp4;base64,{}", "A".repeat(5 * 1024 * 1024)),
                "dataBase64":"must-not-persist","providerSecret":"must-not-persist"}
        });
        let compact = compact_receipt(&receipt);
        let bytes = serde_json::to_vec(&compact).unwrap();
        assert!(bytes.len() < 2048);
        assert_eq!(compact["result"]["transport"], "inline");
        assert_eq!(compact["result"]["width"], 1080);
        assert!(compact["result"].get("locator").is_none());
        assert!(compact["result"].get("dataBase64").is_none());
        assert!(compact["result"].get("providerSecret").is_none());
        assert_eq!(
            compact["result"]["locatorSha256"].as_str().unwrap().len(),
            64
        );
        receipt["result"]["locator"] =
            json!("https://assets.grok.com/video?token=private-signed-value");
        let serialized = serde_json::to_string(&compact_receipt(&receipt)).unwrap();
        assert!(!serialized.contains("private-signed-value"));
        assert!(!serialized.contains("https://"));
    }
}
