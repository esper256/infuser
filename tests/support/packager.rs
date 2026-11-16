use anyhow::Result;
use async_trait::async_trait;
use infuser::server_package::{
    Artifact, Phase, Recipe,
    worker::{Engine, Progress},
};
use std::{
    path::Path,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
#[derive(Default)]
pub struct Packager {
    pub fail_next: AtomicBool,
    pub builds: AtomicUsize,
    pub cleanups: AtomicUsize,
}
#[async_trait]
impl Engine for Packager {
    async fn build(
        &self,
        _: &str,
        key: &str,
        _: &Recipe,
        source: &Path,
        progress: Progress,
    ) -> Result<Artifact> {
        self.builds.fetch_add(1, Ordering::SeqCst);
        anyhow::ensure!(source.join("server.js").exists(), "missing uploaded entry");
        progress(Phase::Checking, "fixture readiness check")?;
        anyhow::ensure!(
            !self.fail_next.swap(false, Ordering::SeqCst),
            "fixture readiness failed"
        );
        Ok(Artifact {
            image: format!("sha256:{}", infuser::platform::hash(key)),
            image_bytes: 1024,
            base_image: format!("node@sha256:{}", "a".repeat(64)),
            lock_sha256: infuser::platform::hash("fixture"),
            recipe_version: 1,
        })
    }
    async fn cleanup(&self, _: &str, _: &str, _: bool) -> Result<()> {
        self.cleanups.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
