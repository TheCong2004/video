// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native local content-page catalog, preserving ArtCraft's page configuration
//! shape without opening or mutating its SQLite database.

use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_BYTES: u64 = 4 * 1024 * 1024;
const MAX_PAGES: usize = 500;
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// Page configuration used by production jobs and batch import.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[allow(missing_docs)]
pub struct ContentPage {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub output_root: String,
    pub target_platform: Option<String>,
    pub default_model_id: Option<String>,
    pub default_workflow_id: Option<String>,
    pub default_language: Option<String>,
    pub default_tone: Option<String>,
    pub default_aspect_ratio: Option<String>,
    pub browser_profile_id: Option<String>,
    pub worker_pool_id: Option<String>,
    pub default_image_prompt: Option<String>,
    pub default_expand_9_16_prompt: Option<String>,
    pub default_video_prompt: Option<String>,
    pub is_archived: bool,
    pub created_at: u64,
    pub updated_at: u64,
    pub revision: u64,
}

/// Create/update values. An existing ID requires the current revision.
#[derive(Clone, Debug, Default)]
#[allow(missing_docs)]
pub struct ContentPageDraft {
    pub id: Option<String>,
    pub revision: u64,
    pub name: String,
    pub slug: Option<String>,
    pub output_root: String,
    pub target_platform: Option<String>,
    pub default_model_id: Option<String>,
    pub default_workflow_id: Option<String>,
    pub default_language: Option<String>,
    pub default_tone: Option<String>,
    pub default_aspect_ratio: Option<String>,
    pub browser_profile_id: Option<String>,
    pub worker_pool_id: Option<String>,
    pub default_image_prompt: Option<String>,
    pub default_expand_9_16_prompt: Option<String>,
    pub default_video_prompt: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct PageLibrary {
    version: u32,
    pages: Vec<ContentPage>,
}

/// Persistent page catalog under Concat config, isolated from ArtCraft data.
pub struct ContentPageStore {
    root: PathBuf,
}

impl ContentPageStore {
    /// Use an explicit config root; useful for isolated tests and migration tools.
    pub fn new(config: &Path) -> Self {
        Self {
            root: config.join("studio"),
        }
    }

    fn lock(&self) -> Result<File, String> {
        fs::create_dir_all(&self.root).map_err(|_| "Cannot create Studio storage.")?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.root.join("pages.lock"))
            .map_err(|_| "Cannot open Studio page lock.")?;
        lock.try_lock()
            .map_err(|_| "Studio page catalog is busy. Try again.")?;
        Ok(lock)
    }

    fn read(&self) -> Result<PageLibrary, String> {
        let file = match File::open(self.root.join("pages.json")) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(PageLibrary {
                    version: 1,
                    pages: vec![],
                });
            }
            Err(_) => return Err("Cannot read Studio pages.".into()),
        };
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "Cannot read Studio pages.")?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("Studio page catalog is too large.".into());
        }
        let library: PageLibrary = serde_json::from_slice(&bytes)
            .map_err(|_| "Studio page catalog is damaged; it has not been overwritten.")?;
        if library.version != 1 || library.pages.len() > MAX_PAGES {
            return Err("Unsupported Studio page catalog format.".into());
        }
        let mut ids = std::collections::HashSet::new();
        for page in &library.pages {
            validate(&page.name, &page.slug, &page.output_root)?;
            if page.id.trim().is_empty()
                || page.id.len() > 128
                || page.revision == 0
                || !ids.insert(&page.id)
            {
                return Err("Invalid Studio page records.".into());
            }
        }
        Ok(library)
    }

    fn write(&self, library: &PageLibrary) -> Result<(), String> {
        let bytes =
            serde_json::to_vec_pretty(library).map_err(|_| "Cannot encode Studio pages.")?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("Studio page catalog is full.".into());
        }
        let temporary = self.root.join("pages.saving");
        let result = (|| {
            let mut file = File::create(&temporary).map_err(|_| "Cannot save Studio pages.")?;
            file.write_all(&bytes)
                .map_err(|_| "Cannot save Studio pages.")?;
            file.sync_all().map_err(|_| "Cannot flush Studio pages.")?;
            drop(file);
            fs::rename(&temporary, self.root.join("pages.json"))
                .map_err(|_| "Cannot replace Studio pages.")?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    /// List pages, excluding archived entries unless explicitly requested.
    pub fn list(&self, include_archived: bool) -> Result<Vec<ContentPage>, String> {
        let _lock = self.lock()?;
        let mut pages = self.read()?.pages;
        if !include_archived {
            pages.retain(|page| !page.is_archived);
        }
        pages.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(pages)
    }

    /// Create or update a page; stale revisions cannot overwrite newer edits.
    pub fn save(&self, draft: ContentPageDraft) -> Result<ContentPage, String> {
        let name = draft.name.trim();
        let output_root = draft.output_root.trim();
        let slug = draft
            .slug
            .as_deref()
            .map(str::trim)
            .filter(|slug| !slug.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| slugify(name));
        validate(name, &slug, output_root)?;
        let _lock = self.lock()?;
        let mut library = self.read()?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "System clock is invalid.")?;
        let mut page = if let Some(id) = &draft.id {
            let page = library
                .pages
                .iter()
                .find(|page| &page.id == id)
                .ok_or("Page no longer exists. Reload the catalog.")?;
            if page.revision != draft.revision {
                return Err("Page changed elsewhere. Reload before saving.".into());
            }
            page.clone()
        } else {
            if library.pages.len() >= MAX_PAGES {
                return Err("Studio page catalog is full.".into());
            }
            ContentPage {
                id: format!(
                    "{:x}-{:x}-{:x}",
                    now.as_nanos(),
                    std::process::id(),
                    NEXT_ID.fetch_add(1, Ordering::Relaxed)
                ),
                name: String::new(),
                slug: String::new(),
                output_root: String::new(),
                target_platform: None,
                default_model_id: None,
                default_workflow_id: None,
                default_language: None,
                default_tone: None,
                default_aspect_ratio: None,
                browser_profile_id: None,
                worker_pool_id: None,
                default_image_prompt: None,
                default_expand_9_16_prompt: None,
                default_video_prompt: None,
                is_archived: false,
                created_at: now.as_secs(),
                updated_at: 0,
                revision: 0,
            }
        };
        page.name = name.into();
        page.slug = slug;
        page.output_root = output_root.into();
        page.target_platform = clean(draft.target_platform);
        page.default_model_id = clean(draft.default_model_id);
        page.default_workflow_id = clean(draft.default_workflow_id);
        page.default_language = clean(draft.default_language);
        page.default_tone = clean(draft.default_tone);
        page.default_aspect_ratio = clean(draft.default_aspect_ratio);
        page.browser_profile_id = clean(draft.browser_profile_id);
        page.worker_pool_id = clean(draft.worker_pool_id);
        page.default_image_prompt = clean(draft.default_image_prompt);
        page.default_expand_9_16_prompt = clean(draft.default_expand_9_16_prompt);
        page.default_video_prompt = clean(draft.default_video_prompt);
        page.updated_at = now.as_secs();
        page.revision = page
            .revision
            .checked_add(1)
            .ok_or("Page revision overflow.")?;
        library.pages.retain(|old| old.id != page.id);
        library.pages.push(page.clone());
        self.write(&library)?;
        Ok(page)
    }

    /// Archive/unarchive exactly one page after checking its revision.
    pub fn archive(&self, id: &str, revision: u64, archived: bool) -> Result<(), String> {
        let _lock = self.lock()?;
        let mut library = self.read()?;
        let page = library
            .pages
            .iter_mut()
            .find(|page| page.id == id)
            .ok_or("Page no longer exists. Reload the catalog.")?;
        if page.revision != revision {
            return Err("Page changed elsewhere. Reload before archiving.".into());
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "System clock is invalid.")?;
        page.is_archived = archived;
        page.updated_at = now.as_secs();
        page.revision = page
            .revision
            .checked_add(1)
            .ok_or("Page revision overflow.")?;
        self.write(&library)
    }
}

fn clean(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn validate(name: &str, slug: &str, output_root: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 200 || name.contains('\0') {
        return Err("Page name must contain 1–200 characters.".into());
    }
    if slug.is_empty() || slug.len() > 200 || slug.contains('\0') {
        return Err("Page slug must contain 1–200 characters.".into());
    }
    if output_root.is_empty() || output_root.len() > 4096 || output_root.contains('\0') {
        return Err("Output folder must contain 1–4096 characters.".into());
    }
    Ok(())
}

fn slugify(name: &str) -> String {
    let mut slug = String::new();
    for ch in name.chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() {
            slug.push(ch);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "concat-studio-pages-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn draft(name: &str) -> ContentPageDraft {
        ContentPageDraft {
            name: name.into(),
            output_root: "D:/media/exports".into(),
            target_platform: Some("tiktok".into()),
            ..Default::default()
        }
    }

    #[test]
    fn create_reopen_update_and_archive_round_trip_preserves_artcraft_fields() {
        let root = temp_root();
        let store = ContentPageStore::new(&root);
        let created = store.save(draft("  Kênh Demo  ")).expect("create page");
        assert_eq!(created.name, "Kênh Demo");
        assert_eq!(created.slug, "kênh-demo");
        assert_eq!(created.target_platform.as_deref(), Some("tiktok"));
        assert_eq!(store.list(false).expect("list").len(), 1);

        let reopened = ContentPageStore::new(&root);
        let loaded = reopened.list(false).expect("reopen list").remove(0);
        assert_eq!(loaded, created);
        let updated = reopened
            .save(ContentPageDraft {
                id: Some(loaded.id.clone()),
                revision: loaded.revision,
                default_workflow_id: Some("video-workflow".into()),
                ..draft("Kênh Demo")
            })
            .expect("update");
        assert_eq!(
            updated.default_workflow_id.as_deref(),
            Some("video-workflow")
        );
        assert!(
            reopened
                .save(ContentPageDraft {
                    id: Some(loaded.id.clone()),
                    revision: loaded.revision,
                    ..draft("Stale")
                })
                .is_err()
        );
        reopened
            .archive(&updated.id, updated.revision, true)
            .expect("archive");
        assert!(reopened.list(false).expect("active pages").is_empty());
        assert_eq!(reopened.list(true).expect("all pages").len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn corrupt_store_is_not_overwritten_and_invalid_page_is_rejected() {
        let root = temp_root();
        let store = ContentPageStore::new(&root);
        fs::create_dir_all(root.join("studio")).expect("storage dir");
        fs::write(root.join("studio/pages.json"), b"not-json").expect("corrupt fixture");
        assert!(store.save(draft("Page")).is_err());
        assert_eq!(
            fs::read(root.join("studio/pages.json")).unwrap(),
            b"not-json"
        );

        fs::remove_file(root.join("studio/pages.json")).expect("remove fixture");
        assert!(store.save(draft(" \0 ")).is_err());
        assert!(store.save(ContentPageDraft::default()).is_err());
        let _ = fs::remove_dir_all(root);
    }
}
