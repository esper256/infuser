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
    if args
        .first()
        .is_some_and(|s| s == "publish" || s == "unpublish")
    {
        return infuser::publisher::run(&args).await;
    }
    if args.first().is_some_and(|s| s == "publisher-pass") {
        use std::{io::Write, os::unix::fs::OpenOptionsExt};
        let config = Config::load(Path::new(
            args.get(1).context("configuration path required")?,
        ))?;
        let app = args.get(2).context("app ID required")?;
        let output = args
            .get(3)
            .context("output .credentials.json path required")?;
        let db = infuser::platform::Database::open(&config.controller.state_dir)?;
        let (id, token) = db.edit(|d| {
            infuser::platform::credential(d, app, "publish", "operator-issued game pass")
        })?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(output)?;
        let connection = serde_json::json!({"id":id,"app_id":app,"api_url":format!("https://{}",config.controller.domain),"token":token});
        file.write_all(serde_json::to_string_pretty(&connection)?.as_bytes())?;
        file.sync_all()?;
        println!("Private game pass written. Keep it out of source control.");
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "firewall-serve") {
        return infuser::firewall::serve(Path::new(args.get(1).context("socket path required")?))
            .await;
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
            "usage: infuser version | infuser token | infuser validate-game MANIFEST | infuser update UPDATE_CONFIG | infuser <serve|init|check|render-caddy> CONFIG [--wildcard]"
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
    let background = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(config.controller.tick_seconds)).await;
            // Persistence failure is fatal: continuing would lose resource accounting.
            runner.reconcile(now()).await?;
        }
        #[allow(unreachable_code)]
        Ok::<(), anyhow::Error>(())
    });
    let app = api::router(controller, token).merge(infuser::portal::router(platform));
    let server = axum::serve(listener, app).with_graceful_shutdown(async {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("signal handler");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
    });
    tokio::select! {
        result = server => { result?; },
        result = background => { result??; bail!("controller loop stopped"); }
    }
    // Containers deliberately survive controller upgrades; recovery adopts them.
    Ok(())
}
