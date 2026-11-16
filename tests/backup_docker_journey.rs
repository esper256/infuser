//! Root-only, disposable Docker volume/image snapshot. Never uses real platform data.
use anyhow::{Context, Result, ensure};
use infuser::{backup, config::Config, controller::Controller, docker::Docker, platform};
use serde_json::Value;
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    process::Command,
    sync::Arc,
};

fn docker(args: &[&str]) -> Result<Vec<u8>> {
    let result = Command::new("docker").args(args).output()?;
    ensure!(
        result.status.success(),
        "Docker fixture command failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(result.stdout)
}
struct Volume(String);
impl Drop for Volume {
    fn drop(&mut self) {
        let _ = docker(&["volume", "rm", &self.0]);
    }
}
struct Container(String);
impl Drop for Container {
    fn drop(&mut self) {
        let _ = docker(&["rm", "-f", &self.0]);
    }
}

#[test]
#[ignore = "requires root, Docker and INFUSER_TEST_IMAGE; creates only uniquely named disposable resources"]
fn staging_preserves_world_ownership_and_required_image_contents() -> Result<()> {
    let image = std::env::var("INFUSER_TEST_IMAGE")
        .context("set INFUSER_TEST_IMAGE to the demo image ID")?;
    let root = tempfile::tempdir()?;
    let state = root.path().join("state");
    let config: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={state:?}\ndisk_reserve_mb=64\n[games.demo]\nimage={image:?}\ncontainer_port=7000\npersistent=true"
    ))?;
    let controller = Controller::open(
        config.clone(),
        Arc::new(Docker {
            settings: config.controller.clone(),
        }),
    )?;
    let owner = controller.snapshot().owner;
    drop(controller);
    drop(platform::Database::open(&state)?);
    let volume = Volume(format!("infuser-{owner}-demo-data"));
    docker(&[
        "volume",
        "create",
        "--label",
        &format!("org.infuser.controller={owner}"),
        "--label",
        "org.infuser.game=demo",
        &volume.0,
    ])?;
    let raw: Vec<Value> = serde_json::from_slice(&docker(&["volume", "inspect", &volume.0])?)?;
    let mount = std::path::Path::new(raw[0]["Mountpoint"].as_str().unwrap());
    let world = mount.join("world.save");
    fs::write(&world, "persistent fixture")?;
    fs::set_permissions(&world, fs::Permissions::from_mode(0o600))?;
    std::os::unix::fs::chown(&world, Some(1000), Some(1000))?;
    fs::create_dir(mount.join("empty-directory"))?;
    std::os::unix::fs::symlink("world.save", mount.join("latest"))?;
    let config_dir = root.path().join("config");
    fs::create_dir(&config_dir)?;
    fs::write(config_dir.join("fixture.toml"), "fixture = true\n")?;
    let work = root.path().join("work");
    fs::create_dir(&work)?;
    let settings = backup::Settings {
        controller_config: config_dir.join("fixture.toml"),
        config_dir,
        work_dir: work.clone(),
        repository_file: root.path().join("repo"),
        password_file: root.path().join("password"),
        rclone_config: None,
        restic: "restic".into(),
        service: "infuser.service".into(),
        extra_paths: vec![],
    };
    let snapshot = work.join("snapshot");
    fs::create_dir(&snapshot)?;
    let running = Container(
        String::from_utf8(docker(&[
            "run",
            "-d",
            "--network",
            "none",
            "--memory",
            "32m",
            "--label",
            &format!("org.infuser.controller={owner}"),
            "--entrypoint",
            "/bin/sh",
            &image,
            "-c",
            "sleep 120",
        ])?)?
        .trim()
        .into(),
    );
    assert!(
        backup::stage(&config, &settings, &snapshot)
            .unwrap_err()
            .to_string()
            .contains("still running"),
        "a surviving owned container makes a snapshot unsafe"
    );
    drop(running);
    backup::stage(&config, &settings, &snapshot)?;
    let manifest = backup::verify(&snapshot)?;
    let entry = manifest
        .volumes
        .get(&volume.0)
        .context("owned world volume missing from snapshot")?;
    assert_eq!(entry.game, "demo");
    let world = snapshot.join(&entry.path).join("world.save");
    assert_eq!(fs::read_to_string(&world)?, "persistent fixture");
    assert_eq!(world.metadata()?.uid(), 1000);
    assert_eq!(world.metadata()?.permissions().mode() & 0o777, 0o600);
    assert_eq!(
        fs::read_link(snapshot.join(&entry.path).join("latest"))?,
        std::path::PathBuf::from("world.save")
    );
    assert_eq!(manifest.images, vec![image]);
    ensure!(
        snapshot.join("images.tar").metadata()?.len() > 1024,
        "required image was not archived"
    );
    fs::remove_dir(snapshot.join(&entry.path).join("empty-directory"))?;
    assert!(
        backup::verify(&snapshot).is_err(),
        "missing empty directories must fail verification"
    );
    Ok(())
}
