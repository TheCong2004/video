// SPDX-License-Identifier: AGPL-3.0-or-later

//! Local Studio prompt library, independent of Tauri, Python and Promax.
//! ArtCraft's prompt fields are preserved; its database is never opened here.
//! Writes are locked, revision checked, synced and atomically replaced.

use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 4 * 1024 * 1024;
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// One locally owned prompt, with the same content fields as ArtCraft.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[allow(missing_docs)]
pub struct PromptTemplate {
    pub id: String,
    pub name: String,
    pub image_prompt: String,
    pub expand_prompt: Option<String>,
    pub video_prompt: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub revision: u64,
}

/// Changes from an editor. None creates; Some requires the matching revision.
#[derive(Clone, Default)]
#[allow(missing_docs)]
pub struct PromptDraft {
    pub id: Option<String>,
    pub revision: u64,
    pub name: String,
    pub image_prompt: String,
    pub expand_prompt: Option<String>,
    pub video_prompt: String,
}

#[derive(Serialize, Deserialize)]
struct Library {
    version: u32,
    templates: Vec<PromptTemplate>,
}

/// Persistent library under Concat's config root, not the source checkout.
pub struct ScriptStore {
    root: PathBuf,
}

impl ScriptStore {
    /// Use an explicit app config root (also enables isolated fixture tests).
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
            .open(self.root.join("scripts.lock"))
            .map_err(|_| "Cannot open Studio storage lock.")?;
        lock.try_lock()
            .map_err(|_| "Studio storage is busy. Try again.")?;
        Ok(lock)
    }

    fn read(&self) -> Result<Library, String> {
        let file = match File::open(self.root.join("scripts.json")) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Library {
                    version: 1,
                    templates: vec![],
                });
            }
            Err(_) => return Err("Cannot read Studio scripts.".into()),
        };
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "Cannot read Studio scripts.")?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("Studio library is too large.".into());
        }
        let library: Library = serde_json::from_slice(&bytes)
            .map_err(|_| "Studio library is damaged; it has not been overwritten.")?;
        if library.version != 1 || library.templates.len() > 1000 {
            return Err("Unsupported Studio library format.".into());
        }
        let mut ids = std::collections::HashSet::new();
        for item in &library.templates {
            validate(
                &item.name,
                &item.image_prompt,
                item.expand_prompt.as_deref(),
                &item.video_prompt,
            )?;
            if item.id.is_empty()
                || item.id.len() > 128
                || item.revision == 0
                || !ids.insert(&item.id)
            {
                return Err("Invalid Studio library records.".into());
            }
        }
        Ok(library)
    }

    fn write(&self, library: &Library) -> Result<(), String> {
        let bytes =
            serde_json::to_vec_pretty(library).map_err(|_| "Cannot encode Studio scripts.")?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("Studio library is full.".into());
        }
        // The OS lock covers the whole read-modify-replace operation, including
        // other Concat instances. A failed write leaves the previous file intact.
        let temporary = self.root.join("scripts.saving");
        let result = (|| {
            let mut file = File::create(&temporary).map_err(|_| "Cannot save Studio scripts.")?;
            file.write_all(&bytes)
                .map_err(|_| "Cannot save Studio scripts.")?;
            file.sync_all()
                .map_err(|_| "Cannot flush Studio scripts.")?;
            drop(file);
            fs::rename(&temporary, self.root.join("scripts.json"))
                .map_err(|_| "Cannot replace Studio scripts.")?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    /// List persisted templates. No engine or network is started.
    pub fn list(&self) -> Result<Vec<PromptTemplate>, String> {
        let _lock = self.lock()?;
        let mut templates = self.read()?.templates;
        templates.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.name.cmp(&b.name)));
        Ok(templates)
    }

    /// Create or update one prompt. Stale editors cannot overwrite a newer save.
    pub fn save(&self, draft: PromptDraft) -> Result<PromptTemplate, String> {
        validate(
            &draft.name,
            &draft.image_prompt,
            draft.expand_prompt.as_deref(),
            &draft.video_prompt,
        )?;
        let _lock = self.lock()?;
        let mut library = self.read()?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "System clock is invalid.")?;
        let mut item = if let Some(id) = &draft.id {
            let item = library
                .templates
                .iter()
                .find(|item| &item.id == id)
                .ok_or("Script no longer exists. Reload the library.")?;
            if item.revision != draft.revision {
                return Err("Script changed elsewhere. Reload before saving.".into());
            }
            item.clone()
        } else {
            if library.templates.len() >= 1000 {
                return Err("Studio library is full.".into());
            }
            PromptTemplate {
                id: format!(
                    "{:x}-{:x}-{:x}",
                    now.as_nanos(),
                    std::process::id(),
                    NEXT_ID.fetch_add(1, Ordering::Relaxed)
                ),
                name: String::new(),
                image_prompt: String::new(),
                expand_prompt: None,
                video_prompt: String::new(),
                created_at: now.as_secs(),
                updated_at: 0,
                revision: 0,
            }
        };
        item.name = draft.name.trim().into();
        item.image_prompt = draft.image_prompt;
        item.expand_prompt = draft.expand_prompt.filter(|text| !text.is_empty());
        item.video_prompt = draft.video_prompt;
        item.updated_at = now.as_secs();
        item.revision = item
            .revision
            .checked_add(1)
            .ok_or("Script revision overflow.")?;
        library.templates.retain(|old| old.id != item.id);
        library.templates.push(item.clone());
        self.write(&library)?;
        Ok(item)
    }

    /// Delete exactly the selected local prompt after UI confirmation.
    pub fn delete(&self, id: &str, revision: u64) -> Result<(), String> {
        let _lock = self.lock()?;
        let mut library = self.read()?;
        let item = library
            .templates
            .iter()
            .find(|item| item.id == id)
            .ok_or("Script no longer exists. Reload the library.")?;
        if item.revision != revision {
            return Err("Script changed elsewhere. Reload before deleting.".into());
        }
        library.templates.retain(|item| item.id != id);
        self.write(&library)
    }
}

fn validate(name: &str, image: &str, expand: Option<&str>, video: &str) -> Result<(), String> {
    if name.trim().is_empty() || name.len() > 256 || name.chars().any(char::is_control) {
        return Err("Enter a script name (maximum 256 bytes, no control characters).".into());
    }
    if [image, expand.unwrap_or(""), video]
        .iter()
        .any(|text| text.len() > 32768 || text.contains('\0'))
    {
        return Err("Each prompt must be at most 32 KiB and contain no NUL characters.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                    "concat-studio-scripts-{}-{}-{}",
                    std::process::id(),
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_nanos(),
                    NEXT_ID.fetch_add(1, Ordering::Relaxed)
                )))
        }
        fn store(&self) -> ScriptStore {
            ScriptStore::new(&self.0)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn draft() -> PromptDraft {
        PromptDraft {
            name: "Xin chào".into(),
            image_prompt: "Ảnh".into(),
            video_prompt: "Video".into(),
            ..Default::default()
        }
    }

    #[test]
    fn studio_scripts_crud_persists_and_preserves_identity() {
        let fixture = Fixture::new();
        let store = fixture.store();
        assert!(store.list().unwrap().is_empty());
        let first = store.save(draft()).unwrap();
        let changed = store
            .save(PromptDraft {
                id: Some(first.id.clone()),
                revision: first.revision,
                expand_prompt: Some("Mở rộng".into()),
                ..draft()
            })
            .unwrap();
        assert_eq!(changed.id, first.id);
        assert_eq!(changed.revision, 2);
        assert_eq!(fixture.store().list().unwrap(), vec![changed.clone()]);
        store.delete(&changed.id, changed.revision).unwrap();
        assert!(fixture.store().list().unwrap().is_empty());
    }

    #[test]
    fn studio_scripts_stale_save_and_delete_do_not_mutate() {
        let fixture = Fixture::new();
        let store = fixture.store();
        let first = store.save(draft()).unwrap();
        let changed = store
            .save(PromptDraft {
                id: Some(first.id.clone()),
                revision: 1,
                ..draft()
            })
            .unwrap();
        assert!(
            store
                .save(PromptDraft {
                    id: Some(first.id.clone()),
                    revision: 1,
                    ..draft()
                })
                .is_err()
        );
        assert!(store.delete(&first.id, 1).is_err());
        assert!(store.delete("../scripts.json", 1).is_err());
        assert_eq!(store.list().unwrap(), vec![changed]);
    }

    #[test]
    fn studio_scripts_invalid_input_and_corruption_are_not_overwritten() {
        let fixture = Fixture::new();
        let store = fixture.store();
        assert!(
            store
                .save(PromptDraft {
                    name: " ".into(),
                    ..draft()
                })
                .is_err()
        );
        assert!(
            store
                .save(PromptDraft {
                    video_prompt: "x".repeat(32769),
                    ..draft()
                })
                .is_err()
        );
        let _ = store.list().unwrap();
        let path = store.root.join("scripts.json");
        fs::write(&path, b"broken").unwrap();
        assert!(store.list().is_err());
        assert!(store.save(draft()).is_err());
        assert_eq!(fs::read(path).unwrap(), b"broken");
    }

    #[test]
    fn studio_scripts_lock_prevents_concurrent_writer() {
        let fixture = Fixture::new();
        let store = fixture.store();
        let lock = store.lock().unwrap();
        assert!(fixture.store().save(draft()).is_err());
        drop(lock);
        assert!(fixture.store().save(draft()).is_ok());
    }

    #[test]
    fn studio_scripts_rejects_oversize_unknown_schema_and_duplicate_ids() {
        let fixture = Fixture::new();
        let store = fixture.store();
        let item = store.save(draft()).unwrap();
        let path = store.root.join("scripts.json");
        for bytes in [
            serde_json::to_vec(&Library {
                version: 2,
                templates: vec![item.clone()],
            })
            .unwrap(),
            serde_json::to_vec(&Library {
                version: 1,
                templates: vec![item.clone(), item],
            })
            .unwrap(),
            vec![b' '; MAX_BYTES as usize + 1],
        ] {
            fs::write(&path, &bytes).unwrap();
            assert!(store.list().is_err());
            assert!(store.save(draft()).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
}
