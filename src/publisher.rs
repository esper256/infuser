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
/// Validated entry-point recipes, not every engine a browser could theoretically run.
pub fn supported_engines() -> Value {
    json!([{"id":"threejs-vite","label":"Three.js / Vite"},{"id":"web","label":"plain HTML/JavaScript"},{"id":"godot-web","label":"Godot web (preview)"},{"id":"godot-native","label":"Godot desktop single-player (preview)"}])
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
fn http_client(args: &[String], timeout: u64) -> Result<reqwest::Client> {
    let mut client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout))
        .redirect(reqwest::redirect::Policy::none());
    if has(args, "--ca-cert") {
        let path = option(args, "--ca-cert").context("--ca-cert requires a PEM file")?;
        let pem = std::fs::read(path).context("cannot read private CA certificate")?;
        client = client.add_root_certificate(reqwest::Certificate::from_pem(&pem)?);
    }
    Ok(client.build()?)
}
pub(crate) fn walk(root: &Path, path: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<()> {
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        // A plain-web export can also be the source project. Ship only the
        // delivery image from this reserved directory, never its art masters.
        if path == root.join("infuser-art") && name != "card-hero.webp" {
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

#[cfg(test)]
#[test]
fn plain_web_export_keeps_art_sources_local() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    std::fs::create_dir_all(root.join("infuser-art/source"))?;
    for name in [
        "index.html",
        crate::card_art::PATH,
        "infuser-art/README.md",
        "infuser-art/source/master.png",
    ] {
        std::fs::write(root.join(name), b"fixture")?;
    }
    let mut files = Vec::new();
    walk(root, root, &mut files)?;
    let mut names: Vec<_> = files.into_iter().map(|(name, _)| name).collect();
    names.sort();
    assert_eq!(names, vec!["index.html", crate::card_art::PATH]);
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
    let http = http_client(args, 120)?;
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
    let mut manifest = json!({"schema_version":1,"app_id":connection.app_id,"title":title,"tags":settings.get("tags").cloned().unwrap_or(json!([])),"play_modes":modes,"solo_runtime":if solo{settings.get("solo_runtime").cloned().unwrap_or(json!("client_only"))}else{Value::Null},"multiplayer":settings.get("multiplayer").cloned().unwrap_or(Value::Null)});
    if let Some(saves) = settings.get("cloud_saves") {
        manifest["cloud_saves"] = saves.clone();
    }
    if let Some(start) = settings.get("server_start") {
        manifest["server_start"] = start.clone();
    }
    if let Some(intents) = settings.get("launch_intents") {
        manifest["launch_intents"] = intents.clone();
    }
    if let Some(sdk) = settings.get("sdk") {
        manifest["sdk"] = sdk.clone();
    }
    let parsed: crate::manifest::Manifest = serde_json::from_value(manifest.clone())?;
    parsed.validate()?;
    let mut server = settings.get("server").cloned().unwrap_or(Value::Null);
    let mut server_source = None;
    let mut recipe = None;
    if server.get("runtime").is_some() {
        let dir = server
            .get("dir")
            .and_then(Value::as_str)
            .context("managed server needs dir pointing at its source folder")?
            .to_owned();
        server
            .as_object_mut()
            .context("server must be an object")?
            .remove("dir");
        let declaration: crate::server_package::Recipe = serde_json::from_value(server)?;
        let source = project
            .join(dir)
            .canonicalize()
            .context("server source directory does not exist")?;
        ensure!(
            source.is_dir() && source.starts_with(&project),
            "server dir must be inside this project"
        );
        recipe = Some(declaration);
        server_source = Some(source);
        server = Value::Null;
    }
    let capabilities = response(
        http.get(format!("{api}/api/publisher/capabilities"))
            .bearer_auth(&connection.token)
            .send()
            .await?,
    )
    .await?;
    if let Some(recipe) = &recipe {
        let packaging = &capabilities["server_packaging"];
        ensure!(
            packaging["version"] == 1 && packaging["enabled"] == true,
            "this Infuser has not enabled managed server packaging yet"
        );
        ensure!(
            packaging["recipes"]
                .as_array()
                .is_some_and(
                    |recipes| recipes.iter().any(|r| r["runtime"] == recipe.runtime
                        && r["node_majors"]
                            .as_array()
                            .is_some_and(|versions| versions.contains(&json!(recipe.node_major))))
                ),
            "requested Node recipe/version is unavailable on this Infuser"
        );
    }
    if let Some(sdk) = &parsed.sdk {
        sdk.admit(&capabilities["sdk"])?;
    }
    if let Some(saves) = &parsed.cloud_saves {
        ensure!(
            capabilities["cloud_checkpoints"]["version"] == 1,
            "this Infuser does not support declared cloud checkpoints yet"
        );
        for file in saves.groups.iter().flat_map(|group| &group.files) {
            ensure!(
                capabilities["cloud_checkpoints"]["formats"]
                    .as_array()
                    .is_some_and(|formats| formats.iter().any(|format| format == &file.format)),
                "this Infuser does not support cloud save format {}",
                file.format
            );
        }
    }
    if parsed.server_start.is_some() {
        ensure!(
            capabilities["runtime_sessions"]["version"] == 1,
            "this Infuser does not support on-demand server sessions yet"
        );
    }
    if let Some(m) = &parsed.multiplayer {
        if !parsed.launch_intents.is_empty() {
            ensure!(
                capabilities["launch_intents"]["version"] == 1,
                "this Infuser does not support Join friend launch intents yet"
            );
        }
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
    let native_only = has(args, "--native-only");
    let native_dirs: Vec<_> = [
        ("linux-x86_64", "--native-linux"),
        ("windows-x86_64", "--native-windows"),
    ]
    .into_iter()
    .filter_map(|(platform, flag)| {
        option(args, flag).map(|path| (platform.to_owned(), project.join(path)))
    })
    .collect();
    ensure!(
        !native_only || !native_dirs.is_empty(),
        "--native-only requires a native export directory"
    );
    if !native_dirs.is_empty() {
        ensure!(
            engine == "godot",
            "native preview currently supports Godot exports"
        );
        ensure!(
            capabilities["native_delivery"]["version"] == 1,
            "this server needs native delivery support first"
        );
        ensure!(
            parsed.multiplayer.is_none()
                && matches!(
                    parsed.solo_runtime,
                    Some(crate::manifest::SoloRuntime::ClientOnly)
                ),
            "native preview supports client-only single-player games; keep multiplayer in the tested web export for now"
        );
    }
    let native = if native_dirs.is_empty() {
        None
    } else {
        Some(crate::native::pack(&native_dirs, native_only)?)
    };
    let native_web = tempfile::tempdir()?;
    let export = if native_only {
        std::fs::write(
            native_web.path().join("index.html"),
            "<!doctype html><title>Infuser Play game</title><p>Open this game in Infuser Play from your Infuser Library.</p>",
        )?;
        native_web.path().to_path_buf()
    } else if let Some(dir) = override_dir {
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
    let export = export.canonicalize()?;
    ensure!(
        export.join("index.html").is_file(),
        "export needs index.html; use --web-dir to point at the built web folder"
    );
    // Upload the export only. The project, node_modules and credentials never leave this machine.
    let mut files = Vec::new();
    walk(&export, &export, &mut files)?;
    let mut server_files = Vec::new();
    if let Some(source) = &server_source {
        let web = export.canonicalize()?;
        ensure!(
            &web != source,
            "web export and server source must be separate directories; set web_dir to the public client export"
        );
        files.retain(|(_, path)| !path.starts_with(source));
        walk(source, source, &mut server_files)?;
        server_files.retain(|(_, path)| !path.starts_with(&web));
        ensure!(server_files.len() <= 4096, "too many server source files");
        let mut server_bytes = 0;
        for (name, path) in &server_files {
            ensure!(
                crate::server_package::source_path(name),
                "exclude unsupported file from server source: {name}"
            );
            let size = path.metadata()?.len();
            ensure!(
                size <= crate::server_package::SOURCE_FILE_BYTES as u64,
                "server source file exceeds 8 MiB: {name}"
            );
            server_bytes += size;
        }
        ensure!(
            server_bytes <= crate::server_package::SOURCE_BYTES,
            "server source exceeds 64 MiB"
        );
        let recipe = recipe.as_ref().unwrap();
        ensure!(
            server_files.iter().any(|(name, _)| name == &recipe.entry),
            "server entry file is missing from its source folder"
        );
        crate::server_package::dependencies(
            &std::fs::read(source.join("package.json"))?,
            &std::fs::read(source.join("package-lock.json"))?,
        )?;
        server_files.sort_by(|a, b| a.0.cmp(&b.0));
    }
    if let Some(native) = &native {
        files.retain(|(name, _)| !name.starts_with("infuser-native/"));
        walk(native.path(), native.path(), &mut files)?;
    }
    // Godot packages resources inside a PCK. Attach only this conventional
    // delivery image separately; never upload the art master or provenance files.
    let hero = project.join(crate::card_art::PATH);
    if !files.iter().any(|(name, _)| name == crate::card_art::PATH) && hero.exists() {
        ensure!(
            !project
                .join("infuser-art")
                .symlink_metadata()?
                .file_type()
                .is_symlink()
                && hero.symlink_metadata()?.file_type().is_file(),
            "card hero must be a regular project file"
        );
        files.push((crate::card_art::PATH.into(), hero));
    }
    if let Some((_, path)) = files.iter().find(|(name, _)| name == crate::card_art::PATH) {
        ensure!(
            path.metadata()?.len() <= crate::card_art::MAX_BYTES as u64,
            "card hero exceeds 250 KiB"
        );
        crate::card_art::validate(&std::fs::read(path)?)?;
        ensure!(
            capabilities["card_art"]["path"] == crate::card_art::PATH,
            "this Infuser server needs an update before it can accept card artwork"
        );
    }
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
    let mut body = json!({"manifest":manifest,"server":server,"spa":settings["spa"].as_bool().unwrap_or(engine=="vite"),"cross_origin_isolation":isolated});
    if let Some(recipe) = &recipe {
        body["server_recipe"] = serde_json::to_value(recipe)?;
    }
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
            let sdk_path = crate::sdk_contract::script(&parsed);
            ensure!(
                !(parsed.sdk.is_some() && html.contains("/_infuser/sdk.js")),
                "Remove the legacy SDK script from index.html when opting into a versioned SDK; use {sdk_path}"
            );
            if !html.contains(sdk_path) {
                let script = format!("<script type=\"module\" src=\"{sdk_path}\"></script>");
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
    if recipe.is_some() {
        println!("Uploading {} server source files…", server_files.len());
        for (name, path) in server_files {
            let mut url = reqwest::Url::parse(&format!("{build_url}/server/files/"))?;
            url.path_segments_mut()
                .map_err(|_| anyhow::anyhow!("invalid server upload URL"))?
                .pop_if_empty()
                .extend(name.split('/'));
            response(
                http.put(url)
                    .bearer_auth(&connection.token)
                    .header("content-type", "application/octet-stream")
                    .body(std::fs::read(path)?)
                    .send()
                    .await?,
            )
            .await
            .with_context(|| format!("upload server/{name}"))?;
        }
        let job_url = format!("{build_url}/server-build");
        let mut job = response(
            http.post(&job_url)
                .bearer_auth(&connection.token)
                .send()
                .await?,
        )
        .await?;
        println!("Server packaging queued. Build: {build}");
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(900);
        let mut last_state = String::new();
        loop {
            let state = job["state"]
                .as_str()
                .context("server-build response missing state")?;
            if state != last_state {
                println!("Server: {state}");
                last_state = state.to_owned();
            }
            if state == "ready" {
                break;
            }
            ensure!(
                state != "failed",
                "server packaging failed for {build}: {}\n{}",
                job["error"].as_str().unwrap_or("unknown error"),
                job["logs"].as_str().unwrap_or("")
            );
            ensure!(
                tokio::time::Instant::now() < deadline,
                "server packaging is still pending; inspect {job_url} with the same credentials, then publish this build when ready. The current game is unchanged."
            );
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            job = response(
                http.get(&job_url)
                    .bearer_auth(&connection.token)
                    .send()
                    .await?,
            )
            .await?;
        }
    }
    let promotion = response(
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
    if promotion["state"] == "queued" {
        println!(
            "Uploaded {title} ({build}). Update queued; it will activate automatically after the shared session ends and its server sleeps."
        );
    } else {
        println!(
            "Published {title} ({build}). {}",
            if has(args, "--preview") {
                "Admin-only preview: hidden from players."
            } else {
                "Ready in the Library."
            }
        );
    }
    if let Some(url) = promotion["url"].as_str() {
        println!("Game link: {url}");
    }
    Ok(())
}

/// Read a three-hour opaque setup code from stdin; write the durable credential privately.
pub async fn connect(args: &[String]) -> Result<()> {
    use std::io::Read;
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
    let file = crate::private_file::reserve(&output).context(
        "cannot create credential file; choose a new path or reuse the existing connection",
    )?;
    let http = http_client(args, 30)?;
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
    crate::private_file::finish_json(file, &connection)?;
    println!(
        "Game connected. Credentials saved to {}. Keep this file out of Git and game exports.",
        output.display()
    );
    Ok(())
}
