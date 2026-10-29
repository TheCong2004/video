// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native role selection for explicitly supplied motion clips. Reuses SLive's
//! idle/custom-motion idea; no pose extraction, generation or autonomous file access.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// User-assigned source clips. Slot 0=reply, 1=greeting, 2=explanation.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct MotionProfile {
    /// Opt-in for role selection, not permission for uploads.
    pub enabled: bool,
    /// Local user-selected MP4 files; existing selected avatar remains idle.
    pub clips: [Option<PathBuf>; 3],
}
impl MotionProfile {
    /// Select deterministically from reply text; never claims semantic gesture synthesis.
    pub fn source<'a>(&'a self, text: &str, idle: &'a Path) -> (&'a Path, usize) {
        if !self.enabled {
            return (idle, 3);
        }
        let text = text
            .trim()
            .chars()
            .take(240)
            .collect::<String>()
            .to_lowercase();
        let role = if text.starts_with("chào") || text.starts_with("xin chào") {
            1
        } else if ["sản phẩm", "giới thiệu", "giá ", "sử dụng"]
            .iter()
            .any(|w| text.contains(w))
        {
            2
        } else {
            0
        };
        self.clips[role]
            .as_deref()
            .map(|p| (p, role))
            .or_else(|| self.clips[0].as_deref().map(|p| (p, 0)))
            .unwrap_or((idle, 3))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile() -> MotionProfile {
        MotionProfile {
            enabled: true,
            clips: [
                Some("reply.mp4".into()),
                Some("hello.mp4".into()),
                Some("explain.mp4".into()),
            ],
        }
    }
    #[test]
    fn uses_only_explicit_roles_with_deterministic_fallback() {
        let mut p = profile();
        let idle = Path::new("idle.mp4");
        assert_eq!(
            p.source("Chào anh Công nhé!", idle),
            (Path::new("hello.mp4"), 1)
        );
        assert_eq!(
            p.source("Sản phẩm này có giá 50 nghìn", idle),
            (Path::new("explain.mp4"), 2)
        );
        assert_eq!(
            p.source("Hôm nay trời đẹp", idle),
            (Path::new("reply.mp4"), 0)
        );
        p.clips[1] = None;
        assert_eq!(p.source("Xin chào", idle).0, Path::new("reply.mp4"));
        p.clips[0] = None;
        assert_eq!(p.source("Xin chào", idle).0, idle);
        p.enabled = false;
        assert_eq!(p.source("Sản phẩm", idle).0, idle);
    }
    #[test]
    fn motion_profile_roundtrip_contains_no_voice_tokens() {
        let p = profile();
        let data = serde_json::to_vec(&p).unwrap();
        let restored: MotionProfile = serde_json::from_slice(&data).unwrap();
        assert_eq!(restored.clips, p.clips);
    }
}
