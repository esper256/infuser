//! Agent-facing workflow: an existing web project plus a private connection file.
use crate::store::secret;
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
};
#[derive(Deserialize)]
struct Connection {
    api_url: String,
    app_id: String,
    token: String,
}
async fn response(r: reqwest::Response) -> Result<Value> {
    let status = r.status();
    let value: Value = r
        .json()
        .await
        .context("Infuser returned a non-JSON response")?;
    ensure!(
        status.is_success(),
        "{}: {}",
        status,
        value["error"].as_str().unwrap_or("request failed")
    );
    Ok(value)
}
fn option(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}
fn has(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}
fn walk(root: &Path, path: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<()> {
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        let kind = entry.file_type()?;
        ensure!(
            !kind.is_symlink(),
            "export contains a symlink: {}",
            entry.path().display()
        );
        if kind.is_dir() {
            walk(root, &entry.path(), out)?;
        } else if kind.is_file() {
            ensure!(
                !name.ends_with(".credentials.json")
                    && !name.ends_with(".secret")
                    && !name.ends_with(".map"),
                "remove secrets/source maps from export: {name}"
            );
            let relative = entry
                .path()
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            out.push((relative, entry.path()));
        }
    }
    Ok(())
}
pub async fn run(args: &[String]) -> Result<()> {
    let connection_path=args.get(1).context("usage: infuser publish CONNECTION.credentials.json PROJECT --solo [--preview] [--title TITLE] [--web-dir EXPORT]")?;
    let connection: Connection = serde_json::from_slice(&std::fs::read(connection_path)?)?;
    let url = reqwest::Url::parse(&connection.api_url)?;
    ensure!(
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "connection requires an HTTPS API URL"
    );
    ensure!(
        crate::config::slug(&connection.app_id),
        "invalid app ID in connection file"
    );
    let api = connection.api_url.trim_end_matches('/');
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    if args[0] == "unpublish" {
        response(
            http.delete(format!("{api}/api/publisher/apps/{}", connection.app_id))
                .bearer_auth(&connection.token)
                .send()
                .await?,
        )
        .await?;
        println!("Game removed from the library. Builds and saves are retained.");
        return Ok(());
    }
    let project = Path::new(
        args.get(2)
            .context("project or exported web folder required")?,
    )
    .canonicalize()?;
    let config_path = project.join("infuser.game.json");
    let settings: Value = if config_path.exists() {
        serde_json::from_slice(&std::fs::read(config_path)?)?
    } else {
        json!({})
    };
    let package: Value = std::fs::read(project.join("package.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(json!({}));
    let engine = if project.join("project.godot").exists() {
        "godot"
    } else if package["devDependencies"]["vite"].is_string()
        || package["dependencies"]["vite"].is_string()
    {
        "vite"
    } else {
        "web"
    };
    let title = option(args, "--title")
        .or_else(|| settings["title"].as_str().map(str::to_owned))
        .unwrap_or_else(|| {
            connection
                .app_id
                .split('-')
                .map(|s| {
                    let mut c = s.chars();
                    c.next()
                        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>()
                .join(" ")
        });
    let modes = if has(args, "--solo") {
        json!(["solo"])
    } else {
        settings.get("play_modes").cloned().context("Declare --solo, or put play_modes and multiplayer settings in infuser.game.json. Infuser never guesses multiplayer topology.")?
    };
    let solo = modes.as_array().is_some_and(|a| a.contains(&json!("solo")));
    let manifest = json!({"schema_version":1,"app_id":connection.app_id,"title":title,"tags":settings.get("tags").cloned().unwrap_or(json!([])),"play_modes":modes,"solo_runtime":if solo{settings.get("solo_runtime").cloned().unwrap_or(json!("client_only"))}else{Value::Null},"multiplayer":settings.get("multiplayer").cloned().unwrap_or(Value::Null)});
    let parsed: crate::manifest::Manifest = serde_json::from_value(manifest.clone())?;
    parsed.validate()?;
    let capabilities = response(
        http.get(format!("{api}/api/publisher/capabilities"))
            .bearer_auth(&connection.token)
            .send()
            .await?,
    )
    .await?;
    if let Some(m) = &parsed.multiplayer {
        let mode = serde_json::to_value(&m.mode)?;
        ensure!(
            capabilities["multiplayer"][mode.as_str().unwrap()] == true,
            "this Infuser cannot host {} yet",
            mode
        );
    }
    println!("Detected {engine}. Preparing {title}…");
    let override_dir =
        option(args, "--web-dir").or_else(|| settings["web_dir"].as_str().map(str::to_owned));
    let export = if let Some(dir) = override_dir {
        project.join(dir)
    } else if engine == "vite" {
        ensure!(
            project.join("node_modules").exists(),
            "Run npm ci (or the project's package manager install) first, then publish again."
        );
        let status = tokio::process::Command::new("npm")
            .args(["run", "build"])
            .current_dir(&project)
            .stdin(Stdio::null())
            .status()
            .await
            .context("npm is needed on the game-making machine, not the Infuser server")?;
        ensure!(status.success(), "Vite build failed");
        project.join("dist")
    } else if engine == "godot" {
        let presets=std::fs::read_to_string(project.join("export_presets.cfg")).context("Add a Godot Web export preset, then publish again. The game-making agent can create it; no server/container configuration is needed.")?;
        let mut name = String::new();
        let mut web = None;
        for line in presets.lines() {
            if line.starts_with("[preset.") {
                name.clear();
            }
            if let Some(n) = line.strip_prefix("name=\"") {
                name = n.trim_end_matches('"').into();
            }
            if line == "platform=\"Web\"" {
                web = Some(name.clone());
                break;
            }
        }
        let preset = web
            .filter(|n| !n.is_empty())
            .context("No Web export preset found")?;
        let export = std::env::temp_dir().join(format!("infuser-export-{}", secret()));
        std::fs::create_dir_all(&export)?;
        let binary = option(args, "--godot").unwrap_or_else(|| "godot".into());
        let status=tokio::process::Command::new(binary).args(["--headless","--path"]).arg(&project).args(["--export-release",&preset]).arg(export.join("index.html")).stdin(Stdio::null()).status().await.context("Install Godot with matching export templates, or use --web-dir with an existing export")?;
        ensure!(status.success(), "Godot Web export failed");
        export
    } else {
        project.clone()
    };
    ensure!(
        export.join("index.html").is_file(),
        "export needs index.html; use --web-dir to point at the built web folder"
    );
    // Upload the export only. The project, node_modules and credentials never leave this machine.
    let mut files = Vec::new();
    walk(&export, &export, &mut files)?;
    files.sort_by(|a, b| a.0.cmp(&b.0));
    ensure!(files.len() <= 4096, "too many exported files");
    let mut total = 0;
    for (_, path) in &files {
        let size = path.metadata()?.len();
        ensure!(
            size <= 64 * 1024 * 1024,
            "{} exceeds the 64 MiB per-file limit",
            path.display()
        );
        total += size;
    }
    ensure!(total <= 256 * 1024 * 1024, "export exceeds 256 MiB");
    let build = format!("b-{}-{}", crate::controller::now(), &secret()[..8]);
    let build_url = format!(
        "{api}/api/publisher/apps/{}/builds/{build}",
        connection.app_id
    );
    let isolated = settings["cross_origin_isolation"]
        .as_bool()
        .unwrap_or(engine == "godot" || files.iter().any(|(n, _)| n.ends_with(".pck")));
    let body = json!({"manifest":manifest,"server":settings.get("server").cloned().unwrap_or(Value::Null),"spa":settings["spa"].as_bool().unwrap_or(engine=="vite"),"cross_origin_isolation":isolated});
    response(
        http.put(&build_url)
            .bearer_auth(&connection.token)
            .json(&body)
            .send()
            .await?,
    )
    .await?;
    println!(
        "Uploading {} files ({:.1} MiB)…",
        files.len(),
        total as f64 / 1048576.
    );
    for (name, path) in files {
        let mut bytes = std::fs::read(path)?;
        if name == "index.html" {
            let mut html = String::from_utf8(bytes).context("index.html must be UTF-8")?;
            if !html.contains("/_infuser/sdk.js") {
                let script = "<script type=\"module\" src=\"/_infuser/sdk.js\"></script>";
                if html.contains("</head>") {
                    html = html.replacen("</head>", &format!("{script}</head>"), 1);
                } else {
                    html = format!("{script}{html}");
                }
            }
            bytes = html.into_bytes();
        }
        let mut url = reqwest::Url::parse(&format!("{build_url}/files/"))?;
        url.path_segments_mut()
            .map_err(|_| anyhow::anyhow!("invalid upload URL"))?
            .pop_if_empty()
            .extend(name.split('/'));
        response(
            http.put(url)
                .bearer_auth(&connection.token)
                .header("content-type", "application/octet-stream")
                .body(bytes)
                .send()
                .await?,
        )
        .await
        .with_context(|| format!("upload {name}"))?;
    }
    response(
        http.post(format!("{build_url}/publish"))
            .bearer_auth(&connection.token)
            .json(&json!({"listed":!has(args,"--preview")}))
            .send()
            .await?,
    )
    .await?;
    if engine == "godot"
        && export.starts_with(std::env::temp_dir())
        && export
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("infuser-export-"))
    {
        let _ = std::fs::remove_dir_all(export);
    }
    println!(
        "Published {title} ({build}). {}",
        if has(args, "--preview") {
            "Admin-only preview: hidden from players."
        } else {
            "Ready in the Library."
        }
    );
    Ok(())
}

/// Read a one-hour opaque setup code from stdin; write the durable credential privately.
pub async fn connect(args: &[String]) -> Result<()> {
    use std::{
        io::{Read, Write},
        os::unix::fs::OpenOptionsExt,
    };
    let mut code = String::new();
    std::io::stdin().take(16384).read_to_string(&mut code)?;
    let token = code
        .trim()
        .strip_prefix("infuser_setup_")
        .context("expected the setup code from the Infuser prompt on stdin")?;
    ensure!(
        token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid setup code"
    );
    let app = option(args, "--app").context("--app APP_ID is required")?;
    ensure!(crate::config::slug(&app), "invalid app ID");
    let api_url = option(args, "--server").context("--server HTTPS_ORIGIN is required")?;
    let api_url = api_url.trim_end_matches('/');
    let url = reqwest::Url::parse(api_url)?;
    ensure!(
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && url.path() == "/",
        "setup requires an HTTPS API origin"
    );
    let output = PathBuf::from(
        option(args, "--output").unwrap_or_else(|| format!(".local/{app}.credentials.json")),
    );
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    // Reserve the destination before consuming the one-time token. Never overwrite
    // an existing working connection file just because another prompt was pasted.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&output)
        .context(
            "cannot create credential file; choose a new path or reuse the existing connection",
        )?;
    let result: Result<()> = async {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let connection = response(
            http.post(format!("{}/api/publisher/claim", api_url))
                .json(&json!({"token":token,"app_id":app}))
                .send()
                .await?,
        )
        .await?;
        ensure!(
            connection["app_id"] == app
                && connection["api_url"] == api_url
                && connection["token"].as_str().is_some_and(|t| t.len() >= 32),
            "server returned an unexpected connection"
        );
        file.write_all(serde_json::to_string_pretty(&connection)?.as_bytes())?;
        file.sync_all()?;
        Ok(())
    }
    .await;
    if result.is_err() {
        drop(file);
        let _ = std::fs::remove_file(&output);
    }
    result?;
    println!(
        "Game connected. Credentials saved to {}. Keep this file out of Git and game exports.",
        output.display()
    );
    Ok(())
}
