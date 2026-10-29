// SPDX-License-Identifier: AGPL-3.0-or-later
//! Durable local-video index. Registration never moves, copies or deletes originals.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// An indexed video and its origin.
#[derive(Serialize, Deserialize)]
pub struct Entry {
    /// Canonical local file location.
    pub path: PathBuf,
    /// User-visible source (production, render or download).
    pub source: String,
    /// Canonical source URL when this is an online download (old indexes omit it).
    #[serde(default)]
    pub source_url: Option<String>,
}

/// Register once by canonical path. Independent records avoid lost concurrent appends.
pub fn record(root: &Path, path: &Path, source: &str) -> Result<(), String> {
    record_online(root, path, source, None)
}

/// Persist provenance only after the caller has verified a complete video.
pub fn record_online(
    root: &Path,
    path: &Path,
    source: &str,
    url: Option<&str>,
) -> Result<(), String> {
    if !path.is_file() {
        return Err("Video không còn trên máy".into());
    }
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let id = format!("{:x}", Sha256::digest(path.to_string_lossy().as_bytes()));
    let dest = root.join(format!("{id}.json"));
    if dest.is_file() {
        return Ok(());
    }
    let temp = root.join(format!("{id}-{}.tmp", rand::random::<u64>()));
    let bytes = serde_json::to_vec(&Entry {
        path,
        source: source.into(),
        source_url: url.map(str::to_owned),
    })
    .map_err(|e| e.to_string())?;
    std::fs::write(&temp, bytes).map_err(|e| e.to_string())?;
    if let Err(error) = std::fs::rename(&temp, &dest) {
        let _ = std::fs::remove_file(&temp);
        if !dest.is_file() {
            return Err(error.to_string());
        }
    }
    Ok(())
}

/// Delete an indexed video and its catalogue entry after the UI has confirmed.
/// Unindexed files are deliberately never removed through the library action.
pub fn remove(root: &Path, path: &Path) -> Result<(), String> {
    let canonical = path.canonicalize().map_err(|e| e.to_string())?;
    let entries = std::fs::read_dir(root).map_err(|e| e.to_string())?;
    let mut index_file = None;
    for file in entries.flatten() {
        if file.path().extension().is_none_or(|ext| ext != "json")
            || file.metadata().is_ok_and(|meta| meta.len() > 65536)
        {
            continue;
        }
        let Ok(bytes) = std::fs::read(file.path()) else {
            continue;
        };
        let Ok(entry) = serde_json::from_slice::<Entry>(&bytes) else {
            continue;
        };
        if entry.path.canonicalize().ok().as_ref() == Some(&canonical) {
            index_file = Some(file.path());
            break;
        }
    }
    let index_file =
        index_file.ok_or_else(|| "Video này không có trong Danh sách video.".to_owned())?;
    std::fs::remove_file(&canonical).map_err(|e| format!("Không xóa được video: {e}"))?;
    std::fs::remove_file(index_file)
        .map_err(|e| format!("Đã xóa video nhưng không xóa được mục lưu: {e}"))?;
    Ok(())
}

/// Stable identities ignore tracking parameters and TikTok account renames.
pub fn source_key(source: &str) -> Option<String> {
    let mut url = url::Url::parse(source).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    let host = url.host_str()?.to_lowercase();
    let parts: Vec<_> = url.path_segments()?.collect();
    if host == "tiktok.com" || host.ends_with(".tiktok.com") {
        let id = parts.windows(2).find(|p| p[0] == "video")?[1];
        return (!id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
            .then(|| format!("tiktok:{id}"));
    }
    if host == "youtu.be" || host == "youtube.com" || host.ends_with(".youtube.com") {
        let id = if host == "youtu.be" {
            parts.first().copied().map(str::to_owned)
        } else if matches!(parts.first(), Some(&"shorts" | &"embed")) {
            parts.get(1).map(|s| s.to_string())
        } else {
            url.query_pairs()
                .find(|(k, _)| k == "v")
                .map(|(_, v)| v.into_owned())
        }?;
        return (!id.is_empty()
            && id.len() <= 64
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b)))
        .then(|| format!("youtube:{id}"));
    }
    url.set_fragment(None);
    Some(url.into())
}

/// Select at most `count` new entries, preserving the source listing order.
pub fn select_new(existing: impl IntoIterator<Item = bool>, count: usize) -> Vec<bool> {
    let mut remaining = count;
    existing
        .into_iter()
        .map(|exists| {
            let selected = !exists && remaining > 0;
            if selected {
                remaining -= 1;
            }
            selected
        })
        .collect()
}

/// Only live files suppress a download. Missing/deleted files can be downloaded again.
pub fn downloaded_sources(root: &Path) -> std::collections::HashSet<String> {
    let mut keys = std::collections::HashSet::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return keys;
    };
    for file in entries.flatten() {
        if file.path().extension().is_none_or(|s| s != "json")
            || file.metadata().is_ok_and(|m| m.len() > 65536)
        {
            continue;
        }
        let Ok(bytes) = std::fs::read(file.path()) else {
            continue;
        };
        let Ok(entry) = serde_json::from_slice::<Entry>(&bytes) else {
            continue;
        };
        if !entry.path.is_file() {
            continue;
        }
        if let Some(key) = entry.source_url.as_deref().and_then(source_key) {
            keys.insert(key);
        }
        // Previous downloader used [platform-id] in filenames but did not store URLs.
        if entry.source == "Tải trực tuyến" {
            if let Some(name) = entry.path.file_stem().and_then(|s| s.to_str()) {
                if let Some((_, id)) = name
                    .rsplit_once(" [")
                    .and_then(|(prefix, id)| id.strip_suffix(']').map(|id| (prefix, id)))
                {
                    if id.len() >= 15 && id.bytes().all(|b| b.is_ascii_digit()) {
                        keys.insert(format!("tiktok:{id}"));
                    } else if id.len() == 11
                        && id
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
                    {
                        keys.insert(format!("youtube:{id}"));
                    }
                }
            }
        }
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_skips_existing_and_selects_exact_requested_count() {
        let flags = select_new((0..150).map(|i| i < 20), 100);
        assert_eq!(flags.iter().filter(|v| **v).count(), 100);
        assert!(flags[..20].iter().all(|v| !v));
        assert!(flags[20..120].iter().all(|v| *v));
        assert!(flags[120..].iter().all(|v| !v));
        assert_eq!(
            select_new([true, false, false], 10),
            vec![false, true, true]
        );
        assert_eq!(
            select_new([false, true, false], 0),
            vec![false, false, false]
        );
    }
    #[test]
    fn source_identity_and_file_presence_prevent_duplicate_downloads() {
        assert_eq!(
            source_key("https://www.tiktok.com/@old/video/123?tracking=x"),
            source_key("https://tiktok.com/@new/video/123")
        );
        assert_eq!(
            source_key("https://youtu.be/abcd1234567?si=x"),
            source_key("https://www.youtube.com/watch?v=abcd1234567&list=other")
        );
        assert_eq!(
            source_key("https://www.youtube.com/shorts/abcd1234567"),
            Some("youtube:abcd1234567".into())
        );
        assert!(source_key("https://tiktok.com/@profile").is_none());
        assert!(source_key("file:///secret").is_none());
        let root =
            std::env::temp_dir().join(format!("concat-dedup-{:032x}", rand::random::<u128>()));
        std::fs::create_dir_all(&root).unwrap();
        let video = root.join("video.mp4");
        std::fs::write(&video, b"complete-fixture").unwrap();
        let catalog = root.join("catalog");
        record_online(
            &catalog,
            &video,
            "Tải trực tuyến",
            Some("https://tiktok.com/@old/video/123"),
        )
        .unwrap();
        assert!(downloaded_sources(&catalog).contains("tiktok:123"));
        std::fs::remove_file(&video).unwrap();
        assert!(downloaded_sources(&catalog).is_empty());
        let legacy = root.join("Demo [abcd1234567].mkv");
        std::fs::write(&legacy, b"complete-fixture").unwrap();
        record(&catalog, &legacy, "Tải trực tuyến").unwrap();
        assert!(downloaded_sources(&catalog).contains("youtube:abcd1234567"));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn registration_is_durable_idempotent_and_preserves_original() {
        let root =
            std::env::temp_dir().join(format!("concat-library-{:032x}", rand::random::<u128>()));
        std::fs::create_dir_all(&root).unwrap();
        let video = root.join("video.mp4");
        std::fs::write(&video, b"fixture-original").unwrap();
        let catalog = root.join("catalog");
        record(&catalog, &video, "Test").unwrap();
        record(&catalog, &video, "Test").unwrap();
        let paths: Vec<_> = std::fs::read_dir(&catalog).unwrap().flatten().collect();
        assert_eq!(paths.len(), 1);
        let saved: Entry =
            serde_json::from_slice(&std::fs::read(paths[0].path()).unwrap()).unwrap();
        assert_eq!(saved.path, video.canonicalize().unwrap());
        assert_eq!(saved.source, "Test");
        assert_eq!(std::fs::read(&video).unwrap(), b"fixture-original");
        assert!(record(&catalog, &root.join("missing.mp4"), "Test").is_err());
        // Exact random test directory, never a user output folder.
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn remove_deletes_only_a_registered_video_and_its_catalogue_entry() {
        let root = std::env::temp_dir().join(format!(
            "concat-library-remove-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let video = root.join("video.mp4");
        std::fs::write(&video, b"fixture-original").unwrap();
        let catalog = root.join("catalog");
        record(&catalog, &video, "Test").unwrap();
        assert!(remove(&catalog, &video).is_ok());
        assert!(!video.exists());
        assert_eq!(std::fs::read_dir(&catalog).unwrap().count(), 0);
        let unregistered = root.join("unregistered.mp4");
        std::fs::write(&unregistered, b"keep me").unwrap();
        assert!(remove(&catalog, &unregistered).is_err());
        assert!(unregistered.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_registration_does_not_drop_records() {
        let root =
            std::env::temp_dir().join(format!("concat-library-{:032x}", rand::random::<u128>()));
        std::fs::create_dir_all(&root).unwrap();
        let mut handles = Vec::new();
        for i in 0..8 {
            let root = root.clone();
            handles.push(std::thread::spawn(move || {
                let path = root.join(format!("{i}.mp4"));
                std::fs::write(&path, b"fixture").unwrap();
                record(&root.join("catalog"), &path, "Test").unwrap();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(std::fs::read_dir(root.join("catalog")).unwrap().count(), 8);
        std::fs::remove_dir_all(root).unwrap();
    }
}
