//! Native delivery and device handoff are separate preview contracts. Browser
//! game builds and their SDK declarations retain their existing meaning.
use anyhow::{Result, ensure};
use infuser_native_protocol::{MANIFEST, Manifest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Read, path::Path};

const MAX_MANIFEST_BYTES: usize = 256 * 1024;

#[derive(Default, Serialize, Deserialize)]
pub struct Data {
    pub pairs: BTreeMap<String, Pair>,
    pub devices: BTreeMap<String, Device>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub launch_devices: BTreeMap<String, String>,
    pub requests: BTreeMap<String, Play>,
}
#[derive(Serialize, Deserialize)]
pub struct Pair {
    pub challenge: String,
    pub expires: u64,
    pub email: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Device {
    pub email: String,
    pub created: u64,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Play {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visit: Option<String>,
    pub email: String,
    pub app: String,
    pub build: String,
    pub platform: String,
    pub guest: bool,
    pub expires: u64,
}
pub fn read(root: &Path) -> Result<Option<Manifest>> {
    let path = root.join(MANIFEST);
    if !path.exists() {
        return Ok(None);
    }
    ensure!(
        path.metadata()?.len() <= MAX_MANIFEST_BYTES as u64,
        "native manifest is too large"
    );
    let value: Manifest = serde_json::from_slice(&std::fs::read(path)?)?;
    value.validate()?;
    Ok(Some(value))
}
pub fn sha256(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut bytes = [0; 65536];
    loop {
        let len = file.read(&mut bytes)?;
        if len == 0 {
            break;
        }
        hash.update(&bytes[..len]);
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}
pub fn validate(root: &Path, files: &BTreeMap<String, u64>) -> Result<()> {
    if let Some(manifest) = read(root)? {
        for artifact in manifest.platforms.values() {
            ensure!(
                files.get(&artifact.archive) == Some(&artifact.bytes),
                "native archive is absent or wrong size"
            );
            ensure!(
                sha256(&root.join(&artifact.archive))? == artifact.sha256,
                "native archive checksum mismatch"
            );
        }
    }
    Ok(())
}

pub fn pack(
    exports: &[(String, std::path::PathBuf)],
    native_only: bool,
) -> Result<tempfile::TempDir> {
    use infuser_native_protocol::{Artifact, File, MAX_ARCHIVE, MAX_UNPACKED};
    let dir = tempfile::tempdir()?;
    std::fs::create_dir(dir.path().join("infuser-native"))?;
    let mut manifest = Manifest {
        version: 1,
        engine: "godot".into(),
        native_only,
        platforms: BTreeMap::new(),
    };
    for (platform, root) in exports {
        let mut files = Vec::new();
        crate::publisher::walk(root, root, &mut files)?;
        ensure!(
            files.len() <= infuser_native_protocol::MAX_FILES,
            "too many native export files"
        );
        files.sort_by(|a, b| a.0.cmp(&b.0));
        let archive = format!("infuser-native/{platform}.zip");
        let path = dir.path().join(&archive);
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path)?);
        let mut inventory = BTreeMap::new();
        let mut entrypoints = Vec::new();
        let mut total = 0u64;
        for (name, source) in files {
            ensure!(
                infuser_native_protocol::safe_path(&name),
                "native export filename is not portable: {name}"
            );
            let size = source.metadata()?.len();
            total = total
                .checked_add(size)
                .ok_or_else(|| anyhow::anyhow!("native size overflow"))?;
            ensure!(
                total <= MAX_UNPACKED,
                "native export exceeds {} MiB",
                MAX_UNPACKED / crate::publish_limits::MIB
            );
            let mut input = std::fs::File::open(&source)?;
            if !name.contains('/') {
                let mut magic = [0u8; 4];
                let len = input.read(&mut magic)?;
                if len == 4
                    && ((platform == "windows-x86_64"
                        && magic[..2] == *b"MZ"
                        && name.ends_with(".exe"))
                        || (platform == "linux-x86_64"
                            && magic == *b"\x7fELF"
                            && !name.ends_with(".so")))
                {
                    entrypoints.push(name.clone());
                }
            }
            zip.start_file(
                &name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated)
                    .unix_permissions(0o644),
            )?;
            std::io::copy(&mut std::fs::File::open(&source)?, &mut zip)?;
            inventory.insert(
                name,
                File {
                    sha256: sha256(&source)?,
                    bytes: size,
                },
            );
        }
        zip.finish()?.sync_all()?;
        ensure!(
            entrypoints.len() == 1,
            "export exactly one native executable at the root of the {platform} directory"
        );
        ensure!(
            path.metadata()?.len() <= MAX_ARCHIVE,
            "compressed native export exceeds {} MiB",
            MAX_ARCHIVE / crate::publish_limits::MIB
        );
        manifest.platforms.insert(
            platform.clone(),
            Artifact {
                archive,
                sha256: sha256(&path)?,
                bytes: path.metadata()?.len(),
                entrypoint: entrypoints.remove(0),
                files: inventory,
            },
        );
    }
    manifest.validate()?;
    let bytes = serde_json::to_vec_pretty(&manifest)?;
    ensure!(
        bytes.len() <= MAX_MANIFEST_BYTES,
        "native file inventory too large"
    );
    std::fs::write(dir.path().join(MANIFEST), bytes)?;
    Ok(dir)
}
