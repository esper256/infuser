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
    if args.first().is_some_and(|s| s == "connect") {
        return infuser::publisher::connect(&args).await;
    }
    if args.first().is_some_and(|s| s == "preview-game") {
        let config = Config::load(Path::new(
            args.get(1).context("configuration path required")?,
        ))?;
        let app = args.get(2).context("preview app ID required")?;
        let email = args.get(3).context("existing adult admin email required")?;
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
            let member = d
                .members
                .get(&email)
                .filter(|m| m.enabled)
                .context("enabled member required")?;
            let name = member.name.clone();
            if args.iter().any(|a| a == "--revoke") {
                d.groups.operators.remove(&email);
            } else {
                d.groups.operators.insert(email.clone());
            }
            Ok(name)
        })?;
        println!("Operator permission updated for {name}.");
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
            infuser::updater::run(infuser::updater::Settings::load(Path::new(path))?).await?
        );
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "token") {
        println!("{}", secret());
        return Ok(());
    }
    let command = args.first().map(String::as_str).unwrap_or("help");
    if !["serve", "init", "check", "render-caddy"].contains(&command) {
        bail!(
            "usage: infuser operator CONFIG EMAIL [--revoke] | infuser metadata-pass CONFIG OUTPUT [--group ID|--all-groups] | infuser publisher-pass CONFIG APP OUTPUT [--group ID] | infuser version | infuser token | infuser validate-game MANIFEST | infuser update UPDATE_CONFIG | infuser <serve|init|check|render-caddy> CONFIG [--wildcard]"
        );
    }
    let path = args.get(1).context("configuration path required")?;
    let config = Config::load(Path::new(path))?;
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
    let background = tokio::spawn(async move {
        let mut last_cleanup = 0;
        loop {
            tokio::time::sleep(Duration::from_secs(config.controller.tick_seconds)).await;
            // Persistence failure is fatal: continuing would lose resource accounting.
            runner.reconcile(now()).await?;
            publications.activate_pending()?;
            if now().saturating_sub(last_cleanup) >= 60 {
                publications.db.cleanup()?;
                if publications.cleanup_feedback().is_err() {
                    tracing::warn!("feedback retention cleanup failed");
                }
                last_cleanup = now();
            }
        }
        #[allow(unreachable_code)]
        Ok::<(), anyhow::Error>(())
    });
    let app = api::router_with_peers(controller, token, Some(platform.peer_rooms()))
        .merge(infuser::portal::router(platform));
    let server = axum::serve(listener, app).with_graceful_shutdown(async {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("signal handler");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
    });
    tokio::select! {
        result = push_worker => { result?; anyhow::bail!("notification worker stopped"); },
        result = server => { result?; },
        result = background => { result??; bail!("controller loop stopped"); }
    }
    // Containers deliberately survive controller upgrades; recovery adopts them.
    Ok(())
}
