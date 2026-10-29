// SPDX-License-Identifier: AGPL-3.0-or-later
//! Kedi Entitlements & FeatureGuard
//! Enforces licensing, RBAC and feature quotas at the execution boundary of Kedi premium features.
//! Non-premium local editor operations (trim, split, local playback) are never blocked.
//! Defined per KEDI_CONCAT_CHROMEX_CODEX_PLAN.md (Gate G1: I01, I05, I06, I08)

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Canonical feature identifiers
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KediFeature {
    /// Basic local video editing (always allowed)
    BasicEdit,
    /// Kedi Automated Factory 6-step video generation
    FactoryAutoRender,
    /// Edge/Local TTS Vietnamese voice generation
    AiVoiceTts,
    /// AI Automatic Subtitle alignment
    AiSubtitles,
    /// Kedi Interactive Live Stream avatar & FSM
    LiveCommerceAvatar,
    /// Cloud rendering & GPU worker export
    CloudGpuRender,
}

impl KediFeature {
    /// Returns true if this feature is a base capability that cannot be revoked
    pub fn is_free_tier(&self) -> bool {
        matches!(self, KediFeature::BasicEdit)
    }

    /// String identifier for audit logs and API wire protocol
    pub fn as_str(&self) -> &'static str {
        match self {
            KediFeature::BasicEdit => "concat.basic_edit",
            KediFeature::FactoryAutoRender => "kedi.factory.auto_render",
            KediFeature::AiVoiceTts => "kedi.ai.voice_tts",
            KediFeature::AiSubtitles => "kedi.ai.subtitles",
            KediFeature::LiveCommerceAvatar => "kedi.live.avatar",
            KediFeature::CloudGpuRender => "kedi.cloud.gpu_render",
        }
    }
}

/// Structured entitlement verification error
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntitlementError {
    /// User is signed out or has no active session
    Unauthenticated,
    /// Feature is not included in the user's active tier
    FeatureNotGranted(String),
    /// Entitlement validity window has passed
    LicenseExpired { expires_at_unix: u64, now_unix: u64 },
    /// Device has been revoked by admin or user remotely
    DeviceRevoked,
    /// AI token/credit budget exhausted
    QuotaExhausted { used: u64, limit: u64 },
    /// Tenant mismatch: trying to execute action on another tenant's project
    TenantMismatch { expected: String, actual: String },
}

impl std::fmt::Display for EntitlementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unauthenticated => write!(f, "User is unauthenticated"),
            Self::FeatureNotGranted(feat) => write!(f, "Feature not granted: {feat}"),
            Self::LicenseExpired { expires_at_unix, now_unix } => {
                write!(f, "License expired at {expires_at_unix} (current {now_unix})")
            }
            Self::DeviceRevoked => write!(f, "Device has been revoked remotely"),
            Self::QuotaExhausted { used, limit } => write!(f, "AI quota exhausted ({used}/{limit})"),
            Self::TenantMismatch { expected, actual } => {
                write!(f, "Tenant mismatch: expected {expected}, got {actual}")
            }
        }
    }
}

impl std::error::Error for EntitlementError {}

/// In-memory grant record bound to a tenant and device
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KediGrant {
    pub user_id: String,
    pub tenant_id: String,
    pub device_id: String,
    pub granted_features: HashSet<KediFeature>,
    pub expires_at_unix: u64,
    pub ai_credits_limit: u64,
    pub ai_credits_used: u64,
    pub is_device_active: bool,
}

impl KediGrant {
    /// Creates a mock/default grant for local testing
    pub fn new_active(
        user_id: impl Into<String>,
        tenant_id: impl Into<String>,
        device_id: impl Into<String>,
        expires_at_unix: u64,
    ) -> Self {
        let mut features = HashSet::new();
        features.insert(KediFeature::BasicEdit);
        features.insert(KediFeature::FactoryAutoRender);
        features.insert(KediFeature::AiVoiceTts);
        features.insert(KediFeature::AiSubtitles);

        Self {
            user_id: user_id.into(),
            tenant_id: tenant_id.into(),
            device_id: device_id.into(),
            granted_features: features,
            expires_at_unix,
            ai_credits_limit: 100_000,
            ai_credits_used: 0,
            is_device_active: true,
        }
    }
}

/// Execution-boundary feature guard (Gate G1)
pub struct FeatureGuard;

impl FeatureGuard {
    /// Checks whether an action can execute.
    /// Returns Ok(()) if authorized, or Err(EntitlementError).
    pub fn check(
        grant: Option<&KediGrant>,
        feature: KediFeature,
        target_tenant_id: &str,
        now_unix: u64,
    ) -> Result<(), EntitlementError> {
        // 1. Basic editing is ALWAYS permitted, even if unauthenticated or revoked
        if feature.is_free_tier() {
            return Ok(());
        }

        // 2. Premium features require an authenticated grant
        let grant = grant.ok_or(EntitlementError::Unauthenticated)?;

        // 3. Verify tenant isolation (A user in Tenant A cannot run jobs in Tenant B)
        if !target_tenant_id.is_empty() && grant.tenant_id != target_tenant_id {
            return Err(EntitlementError::TenantMismatch {
                expected: grant.tenant_id.clone(),
                actual: target_tenant_id.to_string(),
            });
        }

        // 4. Device revocation check
        if !grant.is_device_active {
            return Err(EntitlementError::DeviceRevoked);
        }

        // 5. Expiry check
        if now_unix > grant.expires_at_unix {
            return Err(EntitlementError::LicenseExpired {
                expires_at_unix: grant.expires_at_unix,
                now_unix,
            });
        }

        // 6. Feature inclusion check
        if !grant.granted_features.contains(&feature) {
            return Err(EntitlementError::FeatureNotGranted(feature.as_str().to_string()));
        }

        // 7. Quota check
        if grant.ai_credits_used >= grant.ai_credits_limit {
            return Err(EntitlementError::QuotaExhausted {
                used: grant.ai_credits_used,
                limit: grant.ai_credits_limit,
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_edit_is_never_blocked() {
        // Even with None grant (signed out)
        assert!(FeatureGuard::check(None, KediFeature::BasicEdit, "tenant_1", 1000).is_ok());

        // Even with revoked device
        let mut grant = KediGrant::new_active("u1", "t1", "d1", 5000);
        grant.is_device_active = false;
        assert!(FeatureGuard::check(Some(&grant), KediFeature::BasicEdit, "t1", 1000).is_ok());

        // Even with expired license
        grant.expires_at_unix = 500;
        assert!(FeatureGuard::check(Some(&grant), KediFeature::BasicEdit, "t1", 1000).is_ok());
    }

    #[test]
    fn premium_features_require_active_grant() {
        assert_eq!(
            FeatureGuard::check(None, KediFeature::FactoryAutoRender, "t1", 1000),
            Err(EntitlementError::Unauthenticated)
        );

        let grant = KediGrant::new_active("u1", "t1", "d1", 5000);
        assert!(FeatureGuard::check(Some(&grant), KediFeature::FactoryAutoRender, "t1", 1000).is_ok());
    }

    #[test]
    fn tenant_isolation_is_strictly_enforced() {
        let grant = KediGrant::new_active("u1", "tenant_alpha", "d1", 5000);
        // Matching tenant succeeds
        assert!(FeatureGuard::check(Some(&grant), KediFeature::FactoryAutoRender, "tenant_alpha", 1000).is_ok());

        // Mismatched tenant fails
        assert_eq!(
            FeatureGuard::check(Some(&grant), KediFeature::FactoryAutoRender, "tenant_beta", 1000),
            Err(EntitlementError::TenantMismatch {
                expected: "tenant_alpha".to_string(),
                actual: "tenant_beta".to_string(),
            })
        );
    }

    #[test]
    fn revoked_device_immediately_blocks_new_premium_actions() {
        let mut grant = KediGrant::new_active("u1", "t1", "d1", 5000);
        grant.is_device_active = false;

        assert_eq!(
            FeatureGuard::check(Some(&grant), KediFeature::AiSubtitles, "t1", 1000),
            Err(EntitlementError::DeviceRevoked)
        );
    }

    #[test]
    fn expired_license_blocks_execution() {
        let grant = KediGrant::new_active("u1", "t1", "d1", 1000);
        assert_eq!(
            FeatureGuard::check(Some(&grant), KediFeature::AiSubtitles, "t1", 2000),
            Err(EntitlementError::LicenseExpired {
                expires_at_unix: 1000,
                now_unix: 2000,
            })
        );
    }
}

