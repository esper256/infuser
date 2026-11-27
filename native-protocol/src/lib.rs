//! Preview native delivery contract, separate from the game's stable save API.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MANIFEST: &str = "infuser-native/v1.json";
pub const MAX_ARCHIVE: u64 = 128 * 1024 * 1024;
pub const MAX_UNPACKED: u64 = 1024 * 1024 * 1024;
pub const MAX_FILES: usize = 1024;
pub const PLATFORMS: [&str; 2] = ["linux-x86_64", "windows-x86_64"];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u8,
    pub engine: String,
    #[serde(default)]
    pub native_only: bool,
    pub platforms: BTreeMap<String, Artifact>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub archive: String,
    pub sha256: String,
    pub bytes: u64,
    pub entrypoint: String,
    pub files: BTreeMap<String, File>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    pub sha256: String,
    pub bytes: u64,
}
pub fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}
/// Use the same portable, case-insensitive rules on both OSes. No ZIP path is
/// trusted, even when the archive hash matches an authenticated manifest.
pub fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 240
        && path.split('/').all(|part| {
            if part.is_empty() || part == "." || part == ".." || part.ends_with(['.', ' ']) {
                return false;
            }
            if !part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._- ".contains(&b))
            {
                return false;
            }
            let stem = part.split('.').next().unwrap().to_ascii_uppercase();
            !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                && !(stem.len() == 4
                    && (stem.starts_with("COM") || stem.starts_with("LPT"))
                    && stem.as_bytes()[3].is_ascii_digit())
        })
}
impl Manifest {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1 && self.engine == "godot",
            "unsupported native delivery contract"
        );
        ensure!(
            !self.platforms.is_empty() && self.platforms.len() <= 2,
            "choose supported desktop platforms"
        );
        for (platform, artifact) in &self.platforms {
            ensure!(
                PLATFORMS.contains(&platform.as_str()),
                "unsupported native platform"
            );
            ensure!(
                artifact.archive == format!("infuser-native/{platform}.zip"),
                "unexpected native archive path"
            );
            ensure!(
                digest(&artifact.sha256) && artifact.bytes > 0 && artifact.bytes <= MAX_ARCHIVE,
                "invalid native archive digest or size"
            );
            ensure!(
                safe_path(&artifact.entrypoint) && !artifact.entrypoint.contains('/'),
                "native executable must be at the archive root"
            );
            ensure!(
                artifact.files.contains_key(&artifact.entrypoint),
                "native executable missing from inventory"
            );
            ensure!(
                !platform.starts_with("windows") || artifact.entrypoint.ends_with(".exe"),
                "Windows entrypoint must be an exe"
            );
            ensure!(
                !artifact.files.is_empty() && artifact.files.len() <= MAX_FILES,
                "native file count exceeded"
            );
            let mut folded = BTreeSet::new();
            let mut total = 0u64;
            for (path, file) in &artifact.files {
                ensure!(
                    safe_path(path) && digest(&file.sha256),
                    "invalid native file path or digest"
                );
                let name = path.to_ascii_lowercase();
                ensure!(
                    name != "override.cfg"
                        && name != "infuser-launch.json"
                        && name != ".infuser-run.json",
                    "reserved launcher file"
                );
                ensure!(folded.insert(name), "case-colliding native files");
                total = total
                    .checked_add(file.bytes)
                    .ok_or_else(|| anyhow::anyhow!("native size overflow"))?;
                ensure!(total <= MAX_UNPACKED, "native unpacked size exceeded");
            }
            for path in &folded {
                for (index, _) in path.match_indices('/') {
                    ensure!(
                        !folded.contains(&path[..index]),
                        "native file/directory collision"
                    );
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reject_paths_that_escape_or_alias_on_windows() {
        for path in [
            "../game",
            "/game",
            "C:/game",
            "a\\b",
            "a/../b",
            "NUL.txt",
            "COM1",
            "x.",
            "a//b",
            "game.exe:stream",
        ] {
            assert!(!super::safe_path(path), "{path}");
        }
        assert!(super::safe_path("data/game.pck"));
    }
}
