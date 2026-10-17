// SPDX-License-Identifier: AGPL-3.0-or-later
//! Preserve old unscoped provider credentials without assigning them to an account.
use crate::workflows::WorkflowError;
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};

const ENTROPY: &str = "https://concat-legacy-provider-vault.invalid";
const LIMIT: u64 = 8 * 1024 * 1024;
fn fail<T>(_: T) -> WorkflowError {
    WorkflowError::Configuration
}

/// Encrypt the old JSON store; delete plaintext only after verified read-back.
/// Never import these unscoped credentials into whichever account happens to log in.
pub fn seal(data: &Path) -> Result<bool, WorkflowError> {
    let legacy = data.join("omniroute-providers.json");
    if !legacy.exists() {
        return Ok(false);
    }
    let mut plain = Vec::new();
    fs::File::open(&legacy)
        .map_err(fail)?
        .take(LIMIT + 1)
        .read_to_end(&mut plain)
        .map_err(fail)?;
    if plain.len() as u64 > LIMIT {
        return Err(WorkflowError::Configuration);
    }
    let rows: serde_json::Value = serde_json::from_slice(&plain).map_err(fail)?;
    if !rows.is_array() {
        return Err(WorkflowError::Configuration);
    }
    let target = data.join("omniroute-providers.dpapi");
    if target.exists() {
        let previous = fs::read(&target).map_err(fail)?;
        if crate::auth::unprotect(&previous, ENTROPY).map_err(fail)? != plain {
            // An older archive is never overwritten by a different legacy file.
            return Err(WorkflowError::Configuration);
        }
    } else {
        let sealed = crate::auth::protect(&plain, ENTROPY).map_err(fail)?;
        let temporary = data.join(format!("provider-vault-{}.tmp", rand::random::<u64>()));
        let result = (|| -> std::io::Result<()> {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(&sealed)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, &target)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result.map_err(fail)?;
        let persisted = fs::read(&target).map_err(fail)?;
        if crate::auth::unprotect(&persisted, ENTROPY).map_err(fail)? != plain {
            return Err(WorkflowError::Configuration);
        }
    }
    fs::remove_file(legacy).map_err(fail)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migration_preserves_credentials_without_plaintext() {
        let dir = std::env::temp_dir().join(format!("concat-vault-{}", rand::random::<u64>()));
        fs::create_dir_all(&dir).unwrap();
        let plain = br#"[{"key":"fixture-not-a-real-key"}]"#;
        fs::write(dir.join("omniroute-providers.json"), plain).unwrap();
        assert!(seal(&dir).unwrap());
        assert!(!dir.join("omniroute-providers.json").exists());
        let encrypted = fs::read(dir.join("omniroute-providers.dpapi")).unwrap();
        let needle = b"fixture-not-a-real-key";
        assert!(!encrypted.windows(needle.len()).any(|w| w == needle));
        assert_eq!(crate::auth::unprotect(&encrypted, ENTROPY).unwrap(), plain);
        assert!(!seal(&dir).unwrap());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn corrupt_or_conflicting_archives_do_not_destroy_plaintext() {
        let dir = std::env::temp_dir().join(format!("concat-vault-{}", rand::random::<u64>()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("omniroute-providers.json"), b"[]").unwrap();
        fs::write(dir.join("omniroute-providers.dpapi"), b"corrupt").unwrap();
        assert!(seal(&dir).is_err());
        assert!(dir.join("omniroute-providers.json").exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
