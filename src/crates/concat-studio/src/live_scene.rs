// SPDX-License-Identifier: AGPL-3.0-or-later
//! Prepared scene switching with versioned facts and a continuous external audio clock.
//! This controller selects already-authored clips. It does not synthesize body motion.
#![allow(missing_docs)]
use crate::{
    live_adapters::{AdapterError, SceneManifest},
    live_brain::{LiveTask, ProductRef},
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AvatarManifest {
    pub schema_version: u8,
    pub avatar_id: String,
    pub consent_ref: String,
    pub commercial_allowed: bool,
    pub rights_expires_at: String,
    pub motions: SceneManifest,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenePlan {
    pub scene_id: String,
    pub clip_id: String,
    pub path: PathBuf,
    pub asset_revision: u64,
    pub generation: u64,
    pub product_ref: Option<ProductRef>,
    pub show_price: bool,
}
pub struct SceneController {
    root: PathBuf,
    manifest: AvatarManifest,
    active: ScenePlan,
    pending: Option<ScenePlan>,
    last_pts_ns: u64,
    last_change_ms: u64,
}
impl SceneController {
    /// Validate consent, expiry, local paths and checksums before exposing any clip.
    pub fn load(root: &Path, manifest: AvatarManifest, now: &str) -> Result<Self, AdapterError> {
        let now =
            chrono::DateTime::parse_from_rfc3339(now).map_err(|_| AdapterError::InvalidInput)?;
        if manifest.schema_version != 1
            || manifest.avatar_id.trim().is_empty()
            || manifest.consent_ref.trim().is_empty()
            || !manifest.commercial_allowed
            || !chrono::DateTime::parse_from_rfc3339(&manifest.rights_expires_at)
                .is_ok_and(|expiry| expiry > now)
        {
            return Err(AdapterError::Unauthorized);
        }
        manifest.motions.validate_files(root)?;
        let root = root
            .canonicalize()
            .map_err(|_| AdapterError::InvalidConfig)?;
        let idle = manifest
            .motions
            .clips
            .iter()
            .find(|c| c.gesture == "idle" && c.loopable)
            .ok_or(AdapterError::InvalidInput)?;
        let active = ScenePlan {
            scene_id: "idle".into(),
            clip_id: idle.clip_id.clone(),
            path: root.join(&idle.path),
            asset_revision: manifest.motions.revision,
            generation: 0,
            product_ref: None,
            show_price: false,
        };
        Ok(Self {
            root,
            manifest,
            active,
            pending: None,
            last_pts_ns: 0,
            last_change_ms: 0,
        })
    }
    /// Stages a deterministic clip without changing the output/audio clock.
    pub fn prepare(
        &mut self,
        task: &LiveTask,
        manual_clip: Option<&str>,
    ) -> Result<ScenePlan, AdapterError> {
        let clip = if let Some(id) = manual_clip {
            self.manifest.motions.clips.iter().find(|c| c.clip_id == id)
        } else {
            self.manifest
                .motions
                .clips
                .iter()
                .find(|c| c.gesture == task.motion.gesture && c.emotion == task.motion.emotion)
                .or_else(|| {
                    self.manifest
                        .motions
                        .clips
                        .iter()
                        .find(|c| c.gesture == task.motion.gesture)
                })
        }
        .or_else(|| {
            self.manifest
                .motions
                .clips
                .iter()
                .find(|c| c.gesture == "idle" && c.loopable)
        })
        .ok_or(AdapterError::InvalidInput)?;
        let plan = ScenePlan {
            scene_id: task.scene.scene_id.clone(),
            clip_id: clip.clip_id.clone(),
            path: self.root.join(&clip.path),
            asset_revision: self.manifest.motions.revision,
            generation: task.generation_id,
            product_ref: task.product_ref.clone(),
            show_price: task.scene.show_price,
        };
        self.pending = Some(plan.clone());
        Ok(plan)
    }
    /// Commit a prepared frame boundary only when the generation and price revision still match.
    pub fn commit(
        &mut self,
        generation: u64,
        pts_ns: u64,
        now_ms: u64,
        current_product: Option<&ProductRef>,
        manual: bool,
    ) -> Result<&ScenePlan, AdapterError> {
        let plan = self.pending.take().ok_or(AdapterError::InvalidInput)?;
        if plan.generation != generation
            || pts_ns < self.last_pts_ns
            || (plan.show_price && plan.product_ref.as_ref() != current_product)
        {
            self.fallback(generation, pts_ns);
            return Err(AdapterError::Stale);
        }
        if !manual
            && self.active.scene_id != "idle"
            && now_ms.saturating_sub(self.last_change_ms) < 2000
        {
            return Ok(&self.active);
        }
        self.active = plan;
        self.last_pts_ns = pts_ns;
        self.last_change_ms = now_ms;
        Ok(&self.active)
    }
    /// Fall back to the prevalidated idle clip without resetting the audio PTS.
    pub fn fallback(&mut self, generation: u64, pts_ns: u64) {
        if let Some(idle) = self
            .manifest
            .motions
            .clips
            .iter()
            .find(|c| c.gesture == "idle" && c.loopable)
        {
            self.active = ScenePlan {
                scene_id: "idle".into(),
                clip_id: idle.clip_id.clone(),
                path: self.root.join(&idle.path),
                asset_revision: self.manifest.motions.revision,
                generation,
                product_ref: None,
                show_price: false,
            };
        }
        self.last_pts_ns = self.last_pts_ns.max(pts_ns);
        self.pending = None;
    }
    pub fn active(&self) -> &ScenePlan {
        &self.active
    }
    pub fn audio_pts_ns(&self) -> u64 {
        self.last_pts_ns
    }
    pub fn manifest(&self) -> &AvatarManifest {
        &self.manifest
    }
    pub fn rights_current(&self, now: &str) -> bool {
        chrono::DateTime::parse_from_rfc3339(now)
            .ok()
            .zip(chrono::DateTime::parse_from_rfc3339(&self.manifest.rights_expires_at).ok())
            .is_some_and(|(now, expiry)| expiry > now)
    }
}
