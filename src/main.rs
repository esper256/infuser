use anyhow::{Context, Result, bail, ensure};
use infuser::{
    api, caddy,
    config::Config,
    controller::{Controller, now},
    docker::Docker,
    store::{State, secret},
};
use std::{path::Path, sync::Arc, time::Duration};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "infuser=info".into()),
        )
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|s| s == "turn-fingerprint") {
        println!("{}", infuser::turn::FINGERPRINT);
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "turn") {
        return infuser::turn::run(Path::new(
            args.get(1).context("configuration path required")?,
        ))
        .await;
    }
    if args.first().is_some_and(|s| s == "doctor") {
        let settings = infuser::host_health::Settings::load(Path::new(
            args.get(1).context("operations config required")?,
        ))?;
        let report = infuser::host_health::collect(&settings).await?;
        if !args.iter().any(|v| v == "--quiet") {
            println!("{}", serde_json::to_string(&report)?);
        }
        return Ok(());
    }
    if args.first().is_some_and(|s| {
        matches!(
            s.as_str(),
            "backup" | "backup-init" | "backup-resume" | "backup-verify"
        )
    }) {
        let path = Path::new(
            args.get(1)
                .context("backup configuration or snapshot path required")?,
        );
        if args[0] == "backup-verify" {
            infuser::backup::verify(path)?;
            println!("Restored snapshot verified.");
        } else {
            let settings = if args[0] == "backup-resume" {
                infuser::backup::Settings::load_for_resume(path)?
            } else {
                infuser::backup::Settings::load(path)?
            };
            match args[0].as_str() {
                "backup-init" => settings.initialize()?,
                "backup-resume" => infuser::backup::resume(&settings)?,
                _ => println!("{}", infuser::backup::run(settings).await?),
            }
        }
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "capture-export") {
        ensure!(
            args.len() == 3,
            "usage: infuser capture-export CONFIG NEW_OUTPUT_DIRECTORY"
        );
        let config = Config::load(Path::new(&args[1]))?;
        infuser::publish_capture::export(&config.controller, Path::new(&args[2]))?;
        println!("Private publication fixtures exported. No requests were replayed.");
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "connect") {
        return infuser::publisher::connect(&args).await;
    }
    if args.first().is_some_and(|s| s == "preview-game") {
        let config = Config::load(Path::new(
            args.get(1).context("configuration path required")?,
        ))?;
        let app = args.get(2).context("preview app ID required")?;
        let email = args.get(3).context("existing admin email required")?;
        let output = args.get(4).context("private output path required")?;
        let file = infuser::private_file::reserve(Path::new(output))?;
        let intent = if args.len() == 5 {
            None
        } else {
            ensure!(
                args.len() == 9 && args[5] == "--join-session" && args[7] == "--join-player",
                "optional preview intent: --join-session SESSION --join-player PLAYER"
            );
            Some(infuser::launch_intent::Intent::JoinFriend {
                session_id: args[6].clone(),
                player_id: args[8].clone(),
            })
        };
        let launch = infuser::platform::preview(&config, app, email, intent)?;
        infuser::private_file::finish_json(file, &launch)?;
        println!("Single-use guest preview written; open within two minutes.");
        return Ok(());
    }
    if args
        .first()
        .is_some_and(|s| s == "publish" || s == "unpublish")
    {
        return infuser::publisher::run(&args).await;
    }
    if args
        .first()
        .is_some_and(|s| s == "publisher-pass" || s == "metadata-pass")
    {
        let metadata = args[0] == "metadata-pass";
        let config = Config::load(Path::new(
            args.get(1).context("configuration path required")?,
        ))?;
        let group = args
            .iter()
            .position(|a| a == "--group")
            .map(|i| args.get(i + 1).context("group ID required"))
            .transpose()?
            .map(String::as_str)
            .unwrap_or(infuser::platform::groups::HOME);
        let all_groups = metadata && args.iter().any(|a| a == "--all-groups");
        ensure!(
            !(all_groups && args.iter().any(|a| a == "--group")),
            "choose --group or --all-groups"
        );
        let app = if metadata {
            if all_groups { "all-games" } else { group }
        } else {
            args.get(2).context("app ID required")?
        };
        let output = args
            .get(if metadata { 2 } else { 3 })
            .context("output .credentials.json path required")?;
        let file = infuser::private_file::reserve(Path::new(output))?;
        let db = infuser::platform::Database::open(&config.controller.state_dir)?;
        let (id, token) = db.edit(|d| {
            if !metadata {
                d.reserve_app(app, group)?;
            }
            infuser::platform::credential(
                d,
                app,
                if all_groups {
                    "metadata"
                } else if metadata {
                    "metadata-group"
                } else {
                    "publish"
                },
                if all_groups {
                    "Infuser art agent (all groups)"
                } else if metadata {
                    "Group art agent"
                } else {
                    "operator-issued game pass"
                },
            )
        })?;
        let mut connection = serde_json::json!({"id":id,"api_url":format!("https://{}",config.controller.domain),"token":token});
        if metadata {
            connection["scope"] = if all_groups {
                "all_games_metadata"
            } else {
                "group_metadata"
            }
            .into();
            if !all_groups {
                connection["group_id"] = group.into();
            }
        } else {
            connection["app_id"] = app.into();
        }
        infuser::private_file::finish_json(file, &connection)?;
        println!("Private connection file written. Keep it out of source control.");
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "firewall-serve") {
        return infuser::firewall::serve(Path::new(args.get(1).context("socket path required")?))
            .await;
    }
    if args.first().is_some_and(|s| s == "operator") {
        let config = Config::load(Path::new(args.get(1).context("config path required")?))?;
        ensure!(
            config.controller.private_groups,
            "enable controller.private_groups first"
        );
        let email = args
            .get(2)
            .context("existing member email required")?
            .trim()
            .to_lowercase();
        let name = infuser::platform::Database::open(&config.controller.state_dir)?.edit(|d| {
            let email = d.member_key(&email)?;
            ensure!(d.profile_enabled(&email), "enabled member required");
            let name = d.members[&email].name.clone();
            d.set_operator(&email, !args.iter().any(|a| a == "--revoke"))?;
            Ok(name)
        })?;
        println!("Operator permission updated for {name}.");
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "member-link") {
        let config = Config::load(Path::new(
            args.get(1).context("configuration path required")?,
        ))?;
        let member = args
            .get(2)
            .context("usage: infuser member-link CONFIG MEMBER [--revoke|--list]")?;
        let revoke = args.iter().any(|arg| arg == "--revoke");
        let list = args.iter().any(|arg| arg == "--list");
        ensure!(
            !member.starts_with('-')
                && !(revoke && list)
                && args.len() == if revoke || list { 4 } else { 3 },
            "usage: infuser member-link CONFIG MEMBER [--revoke|--list]"
        );
        let db = infuser::platform::Database::open(&config.controller.state_dir)?;
        if list {
            println!("{}", db.describe_member_link(member)?);
            return Ok(());
        }
        if revoke {
            let names = db.revoke_member_links(member)?;
            println!("Signed-out sessions and sign-in links removed for {names}.");
            return Ok(());
        }
        let (url, _) = db.mint_member_link(&config.controller.domain, member, "server operator")?;
        println!("{url}");
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "member") {
        let config = Config::load(Path::new(args.get(1).context("config path required")?))?;
        let email = args.get(2).context("email required")?;
        let name = args.get(3).context("display name required")?;
        infuser::platform::member(
            &config,
            email,
            name,
            args.iter().any(|s| s == "--admin"),
            !args.iter().any(|s| s == "--disable"),
        )?;
        println!("Member saved.");
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "validate-game") {
        let path = args.get(1).context("game manifest path required")?;
        let manifest: infuser::manifest::Manifest = serde_json::from_slice(&std::fs::read(path)?)?;
        manifest.validate()?;
        println!("Game manifest valid. Check installation capabilities before publishing.");
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "build-info") {
        println!(
            "{}",
            serde_json::json!({"commit": infuser::BUILD_SHA, "built_at": infuser::build_timestamp()})
        );
        return Ok(());
    }
    if args
        .first()
        .is_some_and(|s| s == "version" || s == "--version")
    {
        println!("{}", infuser::BUILD_SHA);
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "update") {
        let path = args.get(1).context("update configuration path required")?;
        println!(
            "{}",
            infuser::updater::run_force(
                infuser::updater::Settings::load(Path::new(path))?,
                args.iter().any(|a| a == "--force")
            )
            .await?
        );
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "snapshot") {
        let path = args.get(1).context("update configuration path required")?;
        println!(
            "{}",
            infuser::updater::run_snapshot(infuser::updater::Settings::load(Path::new(path))?)
                .await?
        );
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "rollback") {
        let path = args.get(1).context("update configuration path required")?;
        println!(
            "{}",
            infuser::updater::run_rollback(infuser::updater::Settings::load(Path::new(path))?)
                .await?
        );
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "rollback-verify") {
        let path = args.get(1).context("snapshot directory required")?;
        let snapshot = infuser::rollback::verify(Path::new(path))?;
        println!("Snapshot {} verified.", snapshot.release);
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "token") {
        println!("{}", secret());
        return Ok(());
    }
    let command = args.first().map(String::as_str).unwrap_or("help");
    if ![
        "serve",
        "init",
        "check",
        "state-check",
        "state-rehearse",
        "render-caddy",
    ]
    .contains(&command)
    {
        bail!(
            "usage: infuser capture-export CONFIG NEW_OUTPUT_DIRECTORY | infuser operator CONFIG EMAIL [--revoke] | infuser member-link CONFIG MEMBER [--revoke|--list] | infuser metadata-pass CONFIG OUTPUT [--group ID|--all-groups] | infuser publisher-pass CONFIG APP OUTPUT [--group ID] | infuser version | infuser token | infuser turn-fingerprint | infuser turn CONFIG | infuser validate-game MANIFEST | infuser update UPDATE_CONFIG [--force] | infuser snapshot UPDATE_CONFIG | infuser rollback UPDATE_CONFIG | infuser rollback-verify SNAPSHOT_DIR | infuser <serve|init|check|state-check|state-rehearse|render-caddy> CONFIG [--wildcard]"
        );
    }
    let path = args.get(1).context("configuration path required")?;
    let config = Config::load(Path::new(path))?;
    if command == "state-rehearse" {
        let count = infuser::platform::identity::rehearse(&config.controller.state_dir)?;
        println!("Identity migration rehearsal passed for {count} profiles; live state unchanged.");
        return Ok(());
    }
    if command == "state-check" {
        infuser::platform::identity::check_state(&config.controller.state_dir)?;
        println!(
            "Identity state compatible: version {}",
            infuser::platform::identity::VERSION
        );
        return Ok(());
    }
    if command == "check" {
        println!(
            "Configuration OK: {} apps, {} static sites",
            config.games.len(),
            config.sites.len()
        );
        return Ok(());
    }
    if command == "render-caddy" {
        let state: State = serde_json::from_slice(
            &std::fs::read(config.controller.state_dir.join("state.json"))
                .context("run infuser init first")?,
        )?;
        print!(
            "{}",
            caddy::render(&config, &state, args.iter().any(|a| a == "--wildcard"))?
        );
        return Ok(());
    }
    let engine = Arc::new(Docker {
        settings: config.controller.clone(),
    });
    let controller = Arc::new(Controller::open(config.clone(), engine)?);
    if command == "init" {
        println!("Catalog initialized; no containers started");
        return Ok(());
    }
    let token = std::fs::read_to_string(&config.controller.token_file)
        .context("cannot read control token file")?;
    let token = token.trim();
    ensure!(
        token.len() >= 32 && token.is_ascii() && !token.bytes().any(|b| b.is_ascii_whitespace()),
        "control token must contain at least 32 non-whitespace ASCII characters"
    );
    controller.recover().await?;
    let listener =
        tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, config.controller.api_port))
            .await?;
    tracing::info!(
        port = config.controller.api_port,
        "Infuser API listening on loopback"
    );
    let platform = infuser::portal::Platform::open(controller.clone())?;
    platform.listen_turn_bridge()?;
    platform.recover_packaging().await?;
    // The listener is already bound. Compression of existing builds stays off
    // this thread so /healthz can answer before those copies exist.
    platform.spawn_precompress_backfill();
    let voice = platform.clone();
    let voice_worker = tokio::spawn(async move {
        let mut last_warning = 0;
        loop {
            match voice.reconcile_voice().await {
                Err(error) if now().saturating_sub(last_warning) >= 30 => {
                    tracing::warn!(%error, "voice reconciliation failed; new admission paused");
                    last_warning = now();
                }
                Ok(()) if last_warning != 0 => {
                    tracing::info!("voice service recovered");
                    last_warning = 0;
                }
                _ => {}
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
    let packaging = platform.clone();
    let package_worker = tokio::spawn(async move {
        loop {
            if let Err(error) = packaging.packaging_step().await {
                tracing::warn!(%error, "server packaging paused; owned resources remain reserved until cleanup succeeds");
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
    let runner = controller.clone();
    let publications = platform.clone();
    let notifications = platform.clone();
    let push_worker = tokio::spawn(async move {
        loop {
            if let Err(error) = notifications.dispatch_push().await {
                tracing::warn!(%error, "chat notification delivery failed");
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
    let maintenance = platform.clone();
    let maintenance_worker = tokio::spawn(async move {
        loop {
            let p = maintenance.clone();
            tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                let Ok(_work) = p.controller.maintenance.enter() else {
                    return Ok(());
                };
                p.db.cleanup()?;
                if p.cleanup_feedback().is_err() {
                    tracing::warn!("feedback retention cleanup failed");
                }
                Ok(())
            })
            .await??;
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
        #[allow(unreachable_code)]
        Ok::<(), anyhow::Error>(())
    });
    let publication_worker = tokio::spawn(async move {
        loop {
            let p = publications.clone();
            tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                let Ok(_work) = p.controller.maintenance.enter() else {
                    return Ok(());
                };
                p.reconcile_publications()?;
                p.activate_pending()
            })
            .await??;
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        #[allow(unreachable_code)]
        Ok::<(), anyhow::Error>(())
    });
    let background = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(config.controller.tick_seconds)).await;
            // Persistence failure is fatal: continuing would lose resource accounting.
            runner.reconcile_step(now()).await?;
        }
        #[allow(unreachable_code)]
        Ok::<(), anyhow::Error>(())
    });
    let app = api::router_with_peers(controller, token, Some(platform.peer_rooms()))
        .merge(infuser::operations::routes(platform.clone(), token))
        .merge(infuser::portal::router(platform));
    let server = axum::serve(listener, app).with_graceful_shutdown(async {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("signal handler");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
    });
    tokio::select! {
        result = voice_worker => { result?; bail!("voice worker stopped"); },
        result = maintenance_worker => { result??; bail!("maintenance worker stopped"); },
        result = publication_worker => { result??; bail!("publication worker stopped"); },
        result = package_worker => { result?; anyhow::bail!("server packaging worker stopped"); },
        result = push_worker => { result?; anyhow::bail!("notification worker stopped"); },
        result = server => { result?; },
        result = background => { result??; bail!("controller loop stopped"); }
    }
    // Containers deliberately survive controller upgrades; recovery adopts them.
    Ok(())
}
