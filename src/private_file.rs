//! Private, durable local files. Reservation never overwrites an existing secret;
//! failed writes clean up automatically through tempfile's ownership model.
use anyhow::{Context, Result};
use serde::Serialize;
use std::{
    fs::{File, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::Path,
};
use tempfile::{NamedTempFile, TempPath};

/// Reserve before consuming a one-time code or issuing a credential. Dropping
/// the returned file removes an unfinished output, including on early errors.
pub fn reserve(path: &Path) -> Result<NamedTempFile> {
    let path = std::path::absolute(path)?;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    Ok(NamedTempFile::from_parts(
        file,
        TempPath::try_from_path(path)?,
    ))
}

pub fn finish_json(mut file: NamedTempFile, value: &impl Serialize) -> Result<()> {
    serde_json::to_writer_pretty(&mut file, value)?;
    file.as_file().sync_all()?;
    file.keep()?;
    Ok(())
}

/// Replace a checkpoint on the same filesystem, then sync the directory entry.
pub fn replace(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("missing parent directory")?;
    let mut file = NamedTempFile::new_in(parent)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn failed_output_can_be_retried_without_overwriting_a_working_secret() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("connection.json");
        let pending = reserve(&path)?;
        assert!(reserve(&path).is_err());
        assert!(
            path.exists(),
            "a failed reservation must not remove the original"
        );
        drop(pending);
        assert!(!path.exists());
        finish_json(reserve(&path)?, &serde_json::json!({"token":"fixture"}))?;
        assert!(reserve(&path).is_err());
        assert_eq!(
            std::fs::read_to_string(&path)?,
            "{\n  \"token\": \"fixture\"\n}"
        );
        assert_eq!(path.metadata()?.permissions().mode() & 0o777, 0o600);
        Ok(())
    }

    #[test]
    fn checkpoint_replacement_leaves_no_partial_or_temporary_file() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("state.json");
        replace(&path, b"old")?;
        replace(&path, b"new")?;
        assert_eq!(std::fs::read(&path)?, b"new");
        assert_eq!(std::fs::read_dir(dir.path())?.count(), 1);
        assert_eq!(path.metadata()?.permissions().mode() & 0o777, 0o600);
        // A failed rename must preserve its destination and clean up staging.
        let directory = dir.path().join("not-a-file");
        std::fs::create_dir(&directory)?;
        assert!(replace(&directory, b"failed").is_err());
        assert!(directory.is_dir());
        assert_eq!(std::fs::read_dir(dir.path())?.count(), 2);
        Ok(())
    }
}
