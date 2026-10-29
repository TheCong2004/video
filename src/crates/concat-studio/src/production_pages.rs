//! Nexora is the catalogue authority; Concat presets are optional overlays only.
use crate::{browser::BrowserProfile, pages::ContentPage};
use serde::Serialize;

/// A real Nexora profile/Page plus an explicitly linked, optional Concat preset.
#[derive(Clone, Debug, Serialize)]
pub struct ProductionPage {
    /// Actual profile returned by Nexora, never a generated substitute.
    pub profile: BrowserProfile,
    /// Existing per-channel preset; absent for profiles just created in Nexora.
    pub preset: Option<ContentPage>,
}
impl ProductionPage {
    /// Server workflow preference, if explicitly configured for this profile.
    pub fn workflow_id(&self) -> Option<&str> {
        self.preset
            .as_ref()?
            .default_workflow_id
            .as_deref()
            .filter(|id| !id.is_empty())
    }
}

/// Preserve Nexora IDs and order. Ambiguous preset links must not choose a random row.
pub fn catalogue(
    profiles: Vec<BrowserProfile>,
    presets: Vec<ContentPage>,
) -> Result<Vec<ProductionPage>, String> {
    let mut linked = std::collections::HashMap::new();
    for preset in presets.into_iter().filter(|p| !p.is_archived) {
        if let Some(id) = preset.browser_profile_id.clone() {
            if linked.insert(id, preset).is_some() {
                return Err(
                    "Nhiều preset cùng gắn một hồ sơ Nexora. Kiểm tra lại Kênh của bạn.".into(),
                );
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    profiles
        .into_iter()
        .map(|profile| {
            if !seen.insert(profile.id.clone()) {
                return Err("Nexora trả hồ sơ trùng ID.".into());
            }
            let preset = linked.remove(&profile.id);
            Ok(ProductionPage { profile, preset })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile(id: &str) -> BrowserProfile {
        BrowserProfile {
            id: id.into(),
            name: format!("Nexora {id}"),
            running: false,
            pid: None,
            generation: None,
        }
    }
    #[test]
    fn new_nexora_profile_is_visible_without_a_concat_page() {
        let pages = catalogue(vec![profile("new")], vec![]).unwrap();
        assert_eq!(pages[0].profile.id, "new");
        assert!(pages[0].preset.is_none());
        assert!(pages[0].workflow_id().is_none());
    }
    #[test]
    fn removed_nexora_profile_never_survives_as_a_stale_local_page() {
        assert!(catalogue(vec![], vec![]).unwrap().is_empty());
        assert!(catalogue(vec![profile("same"), profile("same")], vec![]).is_err());
    }
}
