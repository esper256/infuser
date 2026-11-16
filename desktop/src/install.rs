use anyhow::{Context, Result, ensure};
use infuser_native_protocol::Artifact;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};

pub fn hash(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn regular_file(root: &Path, relative: &str) -> Result<PathBuf> {
    let mut path = root.to_path_buf();
    for component in relative.split('/') {
        path.push(component);
        ensure!(
            !path.symlink_metadata()?.file_type().is_symlink(),
            "unexpected link in installed game"
        );
    }
    ensure!(path.is_file(), "installed game file is missing");
    Ok(path)
}
pub fn verify(root: &Path, artifact: &Artifact) -> Result<()> {
    ensure!(
        !root.symlink_metadata()?.file_type().is_symlink(),
        "unexpected linked game directory"
    );
    for (name, file) in &artifact.files {
        let path = regular_file(root, name)?;
        ensure!(
            path.metadata()?.len() == file.bytes,
            "installed game file size changed"
        );
        let mut input = fs::File::open(path)?;
        let mut hash = Sha256::new();
        let mut buffer = [0; 8192];
        loop {
            let count = match input.read(&mut buffer) {
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                result => result?,
            };
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        ensure!(
            hex(&hash.finalize()) == file.sha256,
            "installed game checksum changed"
        );
    }
    Ok(())
}
fn executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
pub fn install(root: &Path, artifact: &Artifact, bytes: &[u8]) -> Result<PathBuf> {
    ensure!(
        bytes.len() as u64 == artifact.bytes && hash(bytes) == artifact.sha256,
        "download failed verification; retry the update"
    );
    fs::create_dir_all(root)?;
    let stage = tempfile::Builder::new()
        .prefix(".incoming-")
        .tempdir_in(root)?;
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).context("invalid game archive")?;
    ensure!(
        zip.len() == artifact.files.len(),
        "archive inventory does not match"
    );
    let mut seen = std::collections::BTreeSet::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let name = entry.name()?.into_owned();
        ensure!(
            infuser_native_protocol::safe_path(&name) && seen.insert(name.to_ascii_lowercase()),
            "unsafe or duplicate archive path"
        );
        ensure!(
            entry.is_file() && !entry.is_symlink(),
            "archive links and special files are not allowed"
        );
        if let Some(mode) = entry.unix_mode() {
            ensure!(
                mode & 0o170000 == 0 || mode & 0o170000 == 0o100000,
                "archive contains a special file"
            );
        }
        let expected = artifact
            .files
            .get(&name)
            .context("undeclared archive file")?;
        ensure!(
            entry.size() == expected.bytes,
            "archive file size differs from manifest"
        );
        let path = stage.path().join(&name);
        fs::create_dir_all(path.parent().unwrap())?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let copied = std::io::copy(&mut (&mut entry).take(expected.bytes + 1), &mut output)?;
        ensure!(
            copied == expected.bytes,
            "archive expanded beyond declared size"
        );
        output.flush()?;
        output.sync_all()?;
    }
    verify(stage.path(), artifact)?;
    executable(&stage.path().join(&artifact.entrypoint))?;
    let target = root.join(&artifact.sha256);
    if target.exists() {
        if verify(&target, artifact).is_ok() {
            return Ok(target);
        }
        // Active games run from separate launch directories, so replacing a
        // damaged cache cannot rewrite a running executable or its save files.
        ensure!(
            !target.symlink_metadata()?.file_type().is_symlink(),
            "linked cache directory"
        );
        fs::remove_dir_all(&target)?;
    }
    fs::rename(stage.path(), &target)?; // Same filesystem: no partial installation.
    Ok(target)
}
pub struct Verified<'a> {
    cache: &'a Path,
    artifact: &'a Artifact,
}
pub fn verified<'a>(cache: &'a Path, artifact: &'a Artifact) -> Result<Verified<'a>> {
    verify(cache, artifact)?;
    Ok(Verified { cache, artifact })
}
#[cfg(test)]
pub fn prepare_run(
    cache: &Path,
    artifact: &Artifact,
    runs: &Path,
    namespace: &str,
) -> Result<crate::run_storage::Run> {
    prepare_verified(verified(cache, artifact)?, runs, namespace)
}
pub fn prepare_verified(
    verified: Verified<'_>,
    runs: &Path,
    namespace: &str,
) -> Result<crate::run_storage::Run> {
    let Verified { cache, artifact } = verified;
    ensure!(
        infuser_native_protocol::safe_path(namespace),
        "invalid save namespace"
    );
    fs::create_dir_all(runs)?;
    let run = tempfile::Builder::new().prefix("game-").tempdir_in(runs)?;
    for name in artifact.files.keys() {
        let target = run.path().join(name);
        fs::create_dir_all(target.parent().unwrap())?;
        let source = regular_file(cache, name)?;
        #[cfg(target_os = "linux")]
        {
            let input = fs::File::open(&source)?;
            let output = fs::File::create(&target)?;
            if rustix::fs::ioctl_ficlone(&output, &input).is_ok() {
                fs::set_permissions(&target, input.metadata()?.permissions())?;
                continue;
            }
        }
        fs::copy(source, target)?;
    }
    executable(&run.path().join(&artifact.entrypoint))?;
    // Godot reads this beside its executable before loading any autoload or
    // user:// data. This works on Windows without faking APPDATA environment.
    fs::write(
        run.path().join("override.cfg"),
        format!(
            "[application]\nconfig/use_custom_user_dir=true\nconfig/custom_user_dir_name=\"{namespace}\"\nconfig/project_settings_override=\"\"\n"
        ),
    )?;
    crate::run_storage::Run::new(run, &artifact.sha256)
}

#[cfg(test)]
mod tests {
    use super::*;
    use infuser_native_protocol::File;
    fn archive(path: &str, data: &[u8]) -> (Vec<u8>, Artifact) {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(data).unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        let artifact = Artifact {
            archive: "infuser-native/linux-x86_64.zip".into(),
            sha256: hash(&bytes),
            bytes: bytes.len() as u64,
            entrypoint: path.into(),
            files: [(
                path.into(),
                File {
                    sha256: hash(data),
                    bytes: data.len() as u64,
                },
            )]
            .into(),
        };
        (bytes, artifact)
    }
    #[test]
    fn sha256_matches_known_vectors() {
        assert_eq!(
            hash(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hash(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn verification_hashes_empty_and_multiple_buffer_files_and_rejects_tampering() -> Result<()> {
        let temp = tempfile::tempdir()?;
        for size in [0, 8192, 8193, 20000] {
            let data: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
            let (bytes, artifact) = archive("game", &data);
            let installed = install(temp.path(), &artifact, &bytes)?;
            verify(&installed, &artifact)?;
            if size > 0 {
                let mut damaged = data;
                damaged[size - 1] ^= 1;
                fs::write(installed.join("game"), damaged)?;
                assert!(verify(&installed, &artifact).is_err());
            }
        }
        Ok(())
    }

    #[test]
    fn updates_preserve_old_build_and_guest_save_namespace() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let (first, a) = archive("game", b"old");
        let old = install(temp.path(), &a, &first)?;
        let (new, b) = archive("game", b"new");
        let mut damaged = new.clone();
        damaged[0] ^= 1;
        assert!(install(temp.path(), &b, &damaged).is_err());
        verify(&old, &a)?;
        let installed = install(temp.path(), &b, &new)?;
        let owner = prepare_run(
            &installed,
            &b,
            &temp.path().join("runs"),
            "Infuser/site/player/game",
        )?;
        let guest = prepare_run(
            &installed,
            &b,
            &temp.path().join("runs"),
            "Infuser/site/guest-unique/game",
        )?;
        assert_ne!(
            fs::read(owner.path().join("override.cfg"))?,
            fs::read(guest.path().join("override.cfg"))?
        );
        assert_eq!(fs::read(old.join("game"))?, b"old");
        fs::write(owner.path().join("game"), b"modified by game")?;
        assert_eq!(fs::read(guest.path().join("game"))?, b"new");
        assert_eq!(fs::read(installed.join("game"))?, b"new");
        Ok(())
    }
    #[test]
    fn reject_traversal_and_inventory_mismatch() {
        let temp = tempfile::tempdir().unwrap();
        let (bytes, artifact) = archive("../outside", b"bad");
        assert!(install(temp.path(), &artifact, &bytes).is_err());
        let (bytes, mut artifact) = archive("game", b"bad");
        artifact.files.get_mut("game").unwrap().bytes = 2;
        assert!(install(temp.path(), &artifact, &bytes).is_err());
    }
}
