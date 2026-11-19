//! Managed source recipes. Uploaded game files never become builder commands.
use crate::config::{Config, Game};
use anyhow::{Context, Result, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub mod worker;
pub const RECIPE_VERSION: u32 = 1;
pub const BUILD_MEMORY_MB: u64 = 768;
pub const BUILD_CPU_MILLIS: u64 = 500;
pub const SOURCE_BYTES: u64 = 64 * 1024 * 1024;
pub const SOURCE_FILE_BYTES: usize = 8 * 1024 * 1024;
pub const LOG_BYTES: usize = 32 * 1024;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// Operator-selected Linux runtime images, keyed by supported Node major.
    pub images: BTreeMap<String, String>,
    #[serde(default = "image_budget")]
    pub image_budget_mb: u64,
}
fn image_budget() -> u64 {
    2048
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.images.is_empty() && self.images.len() <= 4,
            "configure 1–4 Node runtime images"
        );
        for (major, image) in &self.images {
            ensure!(
                matches!(major.as_str(), "22" | "24"),
                "supported Node recipes are 22 and 24"
            );
            ensure!(
                image.contains("@sha256:") && pinned_image(image),
                "pin the Node base image by repository digest"
            );
        }
        ensure!(
            (256..=1048576).contains(&self.image_budget_mb),
            "invalid server image budget"
        );
        Ok(())
    }
}
pub fn pinned_image(image: &str) -> bool {
    let digest = image.rsplit_once("sha256:").map(|(_, d)| d).unwrap_or("");
    digest.len() == 64
        && digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && (image.starts_with("sha256:") || image.contains("@sha256:"))
        && image
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:/@-".contains(&b))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub runtime: String,
    pub node_major: u16,
    pub entry: String,
    #[serde(default = "health")]
    pub health_path: String,
    #[serde(default = "memory")]
    pub memory_mb: u64,
    #[serde(default = "cpu")]
    pub cpu_millis: u64,
    #[serde(default = "idle")]
    pub idle_seconds: u64,
    #[serde(default = "startup")]
    pub startup_seconds: u64,
    #[serde(default = "stop")]
    pub stop_seconds: u64,
}
fn health() -> String {
    "/healthz".into()
}
fn memory() -> u64 {
    256
}
fn cpu() -> u64 {
    500
}
fn idle() -> u64 {
    120
}
fn startup() -> u64 {
    90
}
fn stop() -> u64 {
    30
}
impl Recipe {
    pub fn validate(&self, config: &Config) -> Result<()> {
        let settings = config
            .controller
            .node_packaging
            .as_ref()
            .context("managed Node packaging is not enabled on this Infuser")?;
        ensure!(self.runtime == "node", "unsupported server recipe");
        ensure!(
            settings.images.contains_key(&self.node_major.to_string()),
            "requested Node major is not available"
        );
        ensure!(
            source_path(&self.entry)
                && [".js", ".mjs", ".cjs"]
                    .iter()
                    .any(|ext| self.entry.ends_with(ext)),
            "entry must be a relative JavaScript file"
        );
        ensure!(
            self.health_path.starts_with('/')
                && self.health_path.len() <= 200
                && self
                    .health_path
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"/_.-".contains(&b)),
            "health_path must be an absolute HTTP path without a query"
        );
        let mut validation = config.clone();
        validation.games.clear();
        validation.games.insert(
            "recipe-check".into(),
            self.game(format!("sha256:{}", "0".repeat(64))),
        );
        validation.validate()?;
        Ok(())
    }
    pub fn game(&self, image: String) -> Game {
        Game {
            image,
            container_port: 8080,
            memory_mb: self.memory_mb,
            cpu_millis: self.cpu_millis,
            protocols: vec!["tcp".into()],
            idle_seconds: self.idle_seconds,
            startup_seconds: self.startup_seconds,
            stop_seconds: self.stop_seconds,
            max_leases: 256,
            user: "1000:1000".into(),
            persistent: false,
            web: true,
            network: crate::config::NetworkPolicy::Internal,
        }
    }
}

pub fn source_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 240
        && path.split('/').all(|part| {
            !part.is_empty()
                && !part.starts_with('.')
                && !matches!(part, "node_modules" | "Dockerfile" | "npm-shrinkwrap.json")
                && !part.ends_with(".credentials.json")
                && !part.ends_with(".secret")
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_- .@()+".contains(&b))
        })
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Uploading,
    Queued,
    Packaging,
    Checking,
    Ready,
    Failed,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Job {
    pub recipe: Recipe,
    pub files: BTreeMap<String, u64>,
    pub state: Phase,
    pub attempt: u32,
    pub logs: String,
    pub error: Option<String>,
    pub artifact: Option<Artifact>,
    /// When server-image packaging started. Older rows leave this at zero.
    #[serde(default)]
    pub started: u64,
}
impl Job {
    pub fn new(recipe: Recipe) -> Self {
        Self {
            recipe,
            files: Default::default(),
            state: Phase::Uploading,
            attempt: 0,
            logs: String::new(),
            error: None,
            artifact: None,
            started: 0,
        }
    }
    pub fn active(&self) -> bool {
        matches!(
            self.state,
            Phase::Queued | Phase::Packaging | Phase::Checking
        )
    }
    pub fn log(&mut self, line: &str) {
        for c in line
            .chars()
            .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        {
            self.logs.push(c);
        }
        self.logs.push('\n');
        if self.logs.len() > LOG_BYTES {
            let mut start = self.logs.len() - LOG_BYTES;
            while !self.logs.is_char_boundary(start) {
                start += 1;
            }
            self.logs.drain(..start);
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub image: String,
    pub image_bytes: u64,
    pub base_image: String,
    pub lock_sha256: String,
    pub recipe_version: u32,
}

#[cfg(test)]
mod log_tests {
    use super::*;
    #[test]
    fn final_error_survives_verbose_unicode_output() {
        let recipe = serde_json::from_value(
            serde_json::json!({"runtime":"node","node_major":24,"entry":"server.js"}),
        )
        .unwrap();
        let mut job = Job::new(recipe);
        job.log(&"🚀".repeat(LOG_BYTES));
        job.log("readiness failed: missing shared module");
        assert!(job.logs.len() <= LOG_BYTES);
        assert!(
            job.logs
                .ends_with("readiness failed: missing shared module\n")
        );
    }
}

#[derive(Clone)]
pub struct Dependency {
    pub url: String,
    pub integrity: Vec<u8>,
}
fn dependency_specs(value: &Value) -> Result<()> {
    for field in ["dependencies", "devDependencies", "optionalDependencies"] {
        if let Some(deps) = value.get(field) {
            for spec in deps
                .as_object()
                .context("dependencies must be an object")?
                .values()
            {
                let spec = spec.as_str().context("dependency version must be text")?;
                ensure!(
                    !spec.contains(':') && !spec.contains('/') && !spec.contains('\\'),
                    "URL, Git, local and alias dependencies are not supported by this recipe"
                );
            }
        }
    }
    Ok(())
}
/// Deliberately narrow first recipe: no project-controlled npm configuration,
/// links, arbitrary registries, Git sources, or lifecycle-script compilation.
pub fn dependencies(package: &[u8], lock: &[u8]) -> Result<Vec<Dependency>> {
    ensure!(
        package.len() <= 1024 * 1024 && lock.len() <= 4 * 1024 * 1024,
        "package metadata is too large"
    );
    let package: Value = serde_json::from_slice(package)?;
    ensure!(
        package.is_object() && package.get("workspaces").is_none(),
        "workspaces need a separate server export in this recipe"
    );
    dependency_specs(&package)?;
    let lock: Value = serde_json::from_slice(lock)?;
    ensure!(
        matches!(lock["lockfileVersion"].as_u64(), Some(2 | 3)),
        "provide npm package-lock.json version 2 or 3"
    );
    let packages = lock["packages"]
        .as_object()
        .context("lockfile packages missing")?;
    ensure!(
        packages.len() <= 512 && packages.contains_key(""),
        "invalid lockfile package count/root"
    );
    let mut result = BTreeMap::new();
    for (path, value) in packages {
        dependency_specs(value)?;
        if path.is_empty() {
            continue;
        }
        ensure!(
            path.starts_with("node_modules/")
                && !path
                    .split('/')
                    .any(|p| p.is_empty() || p == "." || p == "..")
                && !path.contains('\\'),
            "invalid lockfile package path"
        );
        ensure!(
            value.get("link").is_none() && value.get("inBundle").is_none_or(|v| v == false),
            "linked and bundled dependencies are not supported"
        );
        let raw = value["resolved"]
            .as_str()
            .context("every dependency needs a registry tarball URL")?;
        let url = reqwest::Url::parse(raw)?;
        ensure!(
            url.scheme() == "https"
                && url.host_str() == Some("registry.npmjs.org")
                && url.port().is_none()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
                && url.path().ends_with(".tgz"),
            "dependencies must resolve to HTTPS tarballs at registry.npmjs.org"
        );
        let integrity = value["integrity"]
            .as_str()
            .and_then(|v| v.strip_prefix("sha512-"))
            .context("every dependency needs a SHA-512 integrity hash")?;
        let bytes = STANDARD
            .decode(integrity)
            .context("invalid dependency integrity")?;
        ensure!(
            bytes.len() == 64 && STANDARD.encode(&bytes) == integrity,
            "invalid dependency SHA-512 hash"
        );
        if let Some(prior) = result.insert(raw.to_owned(), bytes.clone()) {
            ensure!(prior == bytes, "conflicting dependency integrity");
        }
    }
    Ok(result
        .into_iter()
        .map(|(url, integrity)| Dependency { url, integrity })
        .collect())
}
