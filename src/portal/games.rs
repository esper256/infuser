//! Admin removal and Library-title lock. The play address stays on the app id's slug.
use super::*;
use crate::store::Instance;
use std::{fs, io::Write, os::unix::fs::OpenOptionsExt, path::PathBuf};

pub(super) fn normalize_title(value: &str) -> String {
    value.trim().to_lowercase()
}

pub(super) fn new_publication(data: &platform::Data, app: &str) -> bool {
    let prefix = format!("{app}/");
    !data.apps.contains_key(app)
        && !data
            .builds
            .iter()
            .any(|(key, build)| key.starts_with(&prefix) && build.ready)
}

pub(super) fn duplicate_matches(data: &platform::Data, app: &str, title: &str) -> Vec<Value> {
    let wanted = normalize_title(title);
    if wanted.is_empty() {
        return Vec::new();
    }
    let group = data.groups.app(app);
    let mut found = Vec::new();
    for (id, game) in &data.apps {
        if id == app || data.groups.app(id) != group {
            continue;
        }
        let Some(build) = data.builds.get(&build_key(id, &game.build)) else {
            continue;
        };
        let display = metadata::title(data, id, &build.manifest.title);
        if normalize_title(display) != wanted {
            continue;
        }
        found.push(described(data, id, display, !game.listed));
    }
    for (id, pending) in &data.pending_promotions {
        if id == app || data.apps.contains_key(id) || data.groups.app(id) != group {
            continue;
        }
        let Some(build) = data.builds.get(&build_key(id, &pending.build)) else {
            continue;
        };
        let display = metadata::title(data, id, &build.manifest.title);
        if normalize_title(display) != wanted {
            continue;
        }
        let hidden = !pending.listed || data.admin_hidden_apps.contains(id);
        found.push(described(data, id, display, hidden));
    }
    found
}

fn described(data: &platform::Data, id: &str, title: &str, hidden: bool) -> Value {
    json!({
        "app_id": id,
        "title": title,
        "slug": data.game_urls.get(id).map(|url| &url.slug),
        "hidden": hidden,
    })
}

pub(super) fn duplicate_conflict(found: &[Value]) -> Response {
    let names = found
        .iter()
        .map(|value| {
            let title = value["title"].as_str().unwrap_or("a game");
            let id = value["app_id"].as_str().unwrap_or("");
            let hidden = value["hidden"].as_bool().unwrap_or(false);
            let place = value["slug"].as_str().map_or_else(
                || "no public address yet".to_owned(),
                |slug| format!("address {slug}"),
            );
            let visibility = if hidden { "hidden" } else { "visible" };
            format!("\"{title}\" (id {id}, {place}, {visibility})")
        })
        .collect::<Vec<_>>()
        .join("; ");
    let message = format!(
        "A game with this title already exists: {names}. Titles match after trimming whitespace and ignoring case; no other characters are folded. Publish an update to that game instead of creating a second copy. To create a separate game on purpose, retry this publish with {{\"allow_duplicate_title\":true}}."
    );
    (
        StatusCode::CONFLICT,
        [("Cache-Control", "no-store")],
        Json(json!({"error": message, "code": "duplicate_title", "matches": found})),
    )
        .into_response()
}

pub(super) async fn list(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let email = p.admin(&h, false).await?;
    let data = p.db.catalog()?;
    let (group, _) = p.group_view(&h, &email)?;
    let mut games: Vec<_> = data
        .apps
        .iter()
        .filter(|(id, _)| data.groups.app(id) == group)
        .map(|(id, game)| {
            json!({
                "id": id,
                "title": display_title(&data, id),
                "slug": data.game_urls.get(id).map(|url| &url.slug),
                "hidden": !game.listed,
                "title_locked": data.metadata.get(id).is_some_and(|m| m.title_locked),
            })
        })
        .collect();
    games.sort_by(|a, b| a["title"].as_str().cmp(&b["title"].as_str()));
    Ok(reply(json!({"games": games})))
}

pub(super) async fn preview(
    State(p): State<Platform>,
    Path(app): Path<String>,
    h: HeaderMap,
) -> Answer {
    let email = p.admin(&h, false).await?;
    ensure!(slug(&app), fault(404, "Game not found."));
    let data = p.db.catalog()?;
    data.require_app_group(&email, &app)?;
    ensure!(data.apps.contains_key(&app), fault(404, "Game not found."));
    let title = display_title(&data, &app);
    let hidden = !data.apps[&app].listed;
    let blocked = if !hidden {
        Some("Hide this game before deleting it.")
    } else if title.trim().is_empty() {
        Some("This game has no name to confirm. Rename it, then delete it.")
    } else {
        busy(&p, &data, &app)
    };
    let inventory = inventory(&p, &data, &app)?;
    let lines = removal_lines(&inventory);
    Ok(reply(json!({
        "app_id": app,
        "title": title,
        "confirm": title.trim(),
        "slug": inventory.slug,
        "hidden": hidden,
        "blocked": blocked,
        "removes": inventory_json(&inventory),
        "lines": lines,
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RenameBody {
    title: String,
}

pub(super) async fn rename(
    State(p): State<Platform>,
    Path(app): Path<String>,
    h: HeaderMap,
    Json(body): Json<RenameBody>,
) -> Answer {
    let email = p.admin(&h, true).await?;
    ensure!(slug(&app), fault(404, "Game not found."));
    let title = body.title.trim();
    ensure!(
        !title.is_empty() && title.chars().count() <= 80 && !title.chars().any(char::is_control),
        fault(
            422,
            "title must be 1–80 characters without control characters"
        )
    );
    let title = title.to_owned();
    let (previous, slug) = {
        let data = p.db.catalog()?;
        data.require_app_group(&email, &app)?;
        ensure!(data.apps.contains_key(&app), fault(404, "Game not found."));
        (
            display_title(&data, &app),
            data.game_urls
                .get(&app)
                .map(|url| url.slug.clone())
                .unwrap_or_default(),
        )
    };
    p.db.metadata_update(
        &app,
        platform::MetadataUpdate {
            title: Some(Some(&title)),
            override_lock: true,
            ..Default::default()
        },
    )?;
    p.db.edit(|data| {
        for credential in data.credentials.values_mut().filter(|credential| {
            credential.app == app && credential.role == "publish" && credential.label == previous
        }) {
            credential.label.clone_from(&title);
        }
        Ok(())
    })?;
    tracing::info!(actor = %email, app_id = %app, title = %title, "admin renamed game");
    Ok(reply(json!({
        "app_id": app,
        "title": title,
        "title_locked": true,
        "slug": slug,
        "slug_unchanged": true,
        "note": "The Library name is locked. Later publishes keep this name. The address does not change.",
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DeleteBody {
    confirm: String,
}

struct Plan {
    title: String,
    slug: Option<String>,
    owner: String,
    instances: Vec<(String, Instance)>,
    server_keys: Vec<String>,
}

pub(super) async fn delete_game(
    State(p): State<Platform>,
    Path(app): Path<String>,
    h: HeaderMap,
    Json(body): Json<DeleteBody>,
) -> Answer {
    let email = p.admin(&h, true).await?;
    ensure!(slug(&app), fault(404, "Game not found."));
    let lock = p.app_lock(&app);
    let _held = match lock.try_lock_owned() {
        Ok(guard) => guard,
        Err(_) => {
            return Err(fault(
                409,
                "A publish is still in progress for this game. Wait for it to finish, then delete it.",
            )
            .into());
        }
    };
    let platform = p.clone();
    let actor = email.clone();
    let app_id = app.clone();
    let confirm = body.confirm;
    let plan =
        tokio::task::spawn_blocking(move || remove_records(&platform, &actor, &app_id, &confirm))
            .await
            .map_err(|error| fault(500, &format!("delete worker failed: {error}")))??;
    if let Err(error) = remove_containers(&p, &plan).await {
        return Err(fault(
            500,
            &format!(
                "The game record was removed, but a server container or image is still present: {error:#}"
            ),
        )
        .into());
    }
    Ok(reply(
        json!({"ok":true,"app_id":app,"title":plan.title,"slug":plan.slug}),
    ))
}

fn remove_records(p: &Platform, actor: &str, app: &str, confirm: &str) -> Result<Plan> {
    let _guard = p.publishing.lock().unwrap();
    let data = p.db.catalog()?;
    data.require_app_group(actor, app)?;
    let game = data
        .apps
        .get(app)
        .ok_or_else(|| fault(404, "Game not found."))?;
    ensure!(
        !game.listed,
        fault(409, "Hide this game before deleting it.")
    );
    if let Some(message) = busy(p, &data, app) {
        return Err(fault(409, message));
    }
    let title = display_title(&data, app);
    ensure!(
        !title.trim().is_empty(),
        fault(
            409,
            "This game has no name to confirm. Rename it, then delete it."
        )
    );
    ensure!(
        confirm.trim() == title.trim(),
        fault(400, "Type the game's current name to confirm deletion.")
    );
    let ids = runtime_ids(&data, app);
    let prefix = format!("{app}/");
    let state = p.controller.snapshot();
    let plan = Plan {
        title: title.trim().to_owned(),
        slug: data.game_urls.get(app).map(|url| url.slug.clone()),
        owner: state.owner.clone(),
        instances: ids
            .iter()
            .filter_map(|id| {
                state
                    .games
                    .get(id)
                    .cloned()
                    .map(|instance| (id.clone(), instance))
            })
            .collect(),
        server_keys: data
            .server_builds
            .keys()
            .filter(|key| key.starts_with(&prefix))
            .cloned()
            .collect(),
    };
    for id in &ids {
        if state.games.contains_key(id) || state.published_games.contains_key(id) {
            ensure!(
                p.controller.unregister(id)?,
                fault(
                    409,
                    "This game's server is still shutting down. Wait for it to stop, then delete it."
                )
            );
        }
    }
    // Files and captures are removed only after the server record releases.
    // A refused delete leaves them in place. A retried delete treats missing paths as already gone.
    remove_files(p, app, &ids)?;
    let _captures = crate::publish_capture::forget_app(&p.config().controller, app)?;
    p.db.edit_transaction(|data, tx| purge(data, tx, app))?;
    p.native_manifests
        .lock()
        .unwrap()
        .retain(|(key, _), _| !key.starts_with(&prefix));
    p.dedicated.lock().unwrap().0.retain(|_, report| {
        report.scope.app_id != app && !ids.iter().any(|id| report.scope.runtime_id() == id)
    });
    p.connections.lock().unwrap().0.retain(|_, attempt| {
        attempt.identity.app_id != app && !ids.iter().any(|id| attempt.identity.runtime_id() == id)
    });
    let entry = json!({
        "at": now(),
        "actor": actor,
        "action": "delete_game",
        "app_id": app,
        "title": plan.title,
        "slug": plan.slug,
    });
    audit(&p.config().controller.state_dir, &entry)?;
    tracing::info!(actor = %actor, app_id = %app, title = %plan.title, "admin deleted hidden game");
    Ok(plan)
}

async fn remove_containers(p: &Platform, plan: &Plan) -> Result<()> {
    if plan.instances.is_empty() && plan.server_keys.is_empty() {
        return Ok(());
    }
    for (id, instance) in &plan.instances {
        if !instance.name.is_empty() {
            p.controller
                .engine
                .remove(id, instance, &plan.owner)
                .await?;
        }
        if instance.spec.persistent {
            p.controller.engine.discard_world(&plan.owner, id).await?;
        }
    }
    for key in &plan.server_keys {
        p.packager.cleanup(&plan.owner, key, true).await?;
    }
    Ok(())
}

fn purge(data: &mut platform::Data, tx: &rusqlite::Transaction<'_>, app: &str) -> Result<()> {
    let ids = runtime_ids(data, app);
    let ids_json = serde_json::to_string(&ids)?;
    let shots = {
        let mut statement = tx.prepare("SELECT id FROM screenshots WHERE app=?1")?;
        let rows = statement.query_map([app], |row| row.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    if !shots.is_empty() {
        let shots_json = serde_json::to_string(&shots)?;
        tx.execute(
            "UPDATE library_chat SET screenshot=NULL WHERE screenshot IN (SELECT value FROM json_each(?1))",
            [&shots_json],
        )?;
        for reactions in data.reactions.values_mut() {
            for reaction in reactions.values_mut() {
                if reaction
                    .screenshot
                    .as_ref()
                    .is_some_and(|id| shots.contains(id))
                {
                    reaction.screenshot = None;
                }
            }
        }
    }
    tx.execute("DELETE FROM screenshots WHERE app=?1", [app])?;
    tx.execute(
        "DELETE FROM saves WHERE app IN (SELECT value FROM json_each(?1))",
        [&ids_json],
    )?;
    for id in &ids {
        crate::cloud::delete_app(tx, id)?;
        tx.execute("DELETE FROM update_reactions WHERE deployment=?1", [id])?;
    }
    tx.execute("DELETE FROM game_metadata WHERE app=?1", [app])?;
    tx.execute("DELETE FROM game_urls WHERE app=?1", [app])?;
    let raw: String = tx.query_row("SELECT data FROM native_v1 WHERE id=1", [], |row| {
        row.get(0)
    })?;
    let mut native: crate::native::Data = serde_json::from_str(&raw)?;
    let before = native.requests.len();
    native.requests.retain(|_, play| {
        play.app != app && !ids.iter().any(|id| play.deployment.as_deref() == Some(id))
    });
    if native.requests.len() != before {
        tx.execute(
            "UPDATE native_v1 SET data=?1 WHERE id=1",
            [serde_json::to_string(&native)?],
        )?;
    }
    let prefix = format!("{app}/");
    data.builds.retain(|key, _| !key.starts_with(&prefix));
    data.server_builds
        .retain(|key, _| !key.starts_with(&prefix));
    data.apps.remove(app);
    data.admin_hidden_apps.remove(app);
    data.pending_promotions.remove(app);
    data.tags.remove(app);
    data.owners.remove(app);
    data.author_credits.remove(app);
    data.metadata.remove(app);
    data.game_urls.remove(app);
    data.groups.apps.remove(app);
    data.setup_claims.retain(|_, claim| claim.app != app);
    data.credentials.retain(|_, credential| {
        !(credential.app == app && matches!(credential.role.as_str(), "publish" | "server"))
    });
    data.launches
        .retain(|_, launch| launch.app != app && !ids.iter().any(|id| launch.runtime_id() == id));
    for id in &ids {
        data.deployments.remove(id);
        data.reactions.remove(id);
    }
    for visit in data.visits.values_mut() {
        visit.games.remove(app);
    }
    Ok(())
}

fn audit(dir: &std::path::Path, value: &Value) -> Result<()> {
    let path = dir.join("admin-audit.jsonl");
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(&path)?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn display_title(data: &platform::Data, app: &str) -> String {
    let fallback = data
        .apps
        .get(app)
        .and_then(|game| data.builds.get(&build_key(app, &game.build)))
        .map(|build| build.manifest.title.as_str())
        .unwrap_or("");
    metadata::title(data, app, fallback).to_owned()
}

fn runtime_ids(data: &platform::Data, app: &str) -> Vec<String> {
    let mut ids = vec![app.to_owned()];
    ids.extend(
        data.deployments
            .iter()
            .filter(|(_, deployment)| deployment.app == app)
            .map(|(id, _)| id.clone()),
    );
    ids
}

fn busy(p: &Platform, data: &platform::Data, app: &str) -> Option<&'static str> {
    let ids = runtime_ids(data, app);
    let time = now();
    if ids.iter().any(|id| p.peer_rooms().active(id, time)) {
        return Some("This game still has a live room. Wait for players to leave, then delete it.");
    }
    let state = p.controller.snapshot();
    if ids
        .iter()
        .any(|id| state.games.get(id).is_some_and(Instance::reserved))
    {
        return Some("This game's server is still running. Wait for it to stop, then delete it.");
    }
    let prefix = format!("{app}/");
    if data.pending_promotions.contains_key(app)
        || data
            .server_builds
            .iter()
            .any(|(key, job)| key.starts_with(&prefix) && job.active())
        || state
            .build_reservation
            .as_ref()
            .is_some_and(|reservation| reservation.job.starts_with(&prefix))
    {
        return Some(
            "A publish is still in progress for this game. Wait for it to finish, then delete it.",
        );
    }
    None
}

struct Inventory {
    releases: u64,
    web_files: u64,
    web_bytes: u64,
    saves: u64,
    screenshots: u64,
    reactions: u64,
    cloud_checkpoints: u64,
    credentials: u64,
    captures: u64,
    containers: Vec<Value>,
    packaged_images: Vec<String>,
    volumes: u64,
    slug: Option<String>,
    aliases: Vec<String>,
}

fn inventory(p: &Platform, data: &platform::Data, app: &str) -> Result<Inventory> {
    let ids = runtime_ids(data, app);
    let prefix = format!("{app}/");
    let (web_files, web_bytes) = dir_stats(&p.root().join(app))?;
    let ids_json = serde_json::to_string(&ids)?;
    let connection = p.db.connection.lock().unwrap();
    let saves = connection.query_row(
        "SELECT COUNT(*) FROM saves WHERE app IN (SELECT value FROM json_each(?1))",
        [&ids_json],
        |row| row.get(0),
    )?;
    let screenshots = connection.query_row(
        "SELECT COUNT(*) FROM screenshots WHERE app=?1",
        [app],
        |row| row.get(0),
    )?;
    let cloud_checkpoints = connection.query_row(
        "SELECT COUNT(*) FROM cloud_revisions WHERE app IN (SELECT value FROM json_each(?1))",
        [&ids_json],
        |row| row.get(0),
    )?;
    drop(connection);
    let state = p.controller.snapshot();
    let containers = ids
        .iter()
        .filter_map(|id| {
            state
                .games
                .get(id)
                .map(|instance| json!({"id": id, "running": instance.reserved()}))
        })
        .collect::<Vec<_>>();
    let volumes = ids
        .iter()
        .filter(|id| {
            server_persistent(data, id.as_str())
                || state
                    .games
                    .get(id.as_str())
                    .is_some_and(|game| game.spec.persistent)
        })
        .count() as u64;
    Ok(Inventory {
        releases: data
            .builds
            .keys()
            .filter(|key| key.starts_with(&prefix))
            .count() as u64,
        web_files,
        web_bytes,
        saves,
        screenshots,
        reactions: ids
            .iter()
            .map(|id| data.reactions.get(id).map(|game| game.len()).unwrap_or(0) as u64)
            .sum(),
        cloud_checkpoints,
        credentials: data
            .credentials
            .values()
            .filter(|credential| {
                credential.app == app && matches!(credential.role.as_str(), "publish" | "server")
            })
            .count() as u64,
        captures: crate::publish_capture::count_app(&p.config().controller, app)?,
        containers,
        packaged_images: data
            .server_builds
            .iter()
            .filter(|(key, _)| key.starts_with(&prefix))
            .filter_map(|(_, job)| job.artifact.as_ref().map(|artifact| artifact.image.clone()))
            .collect(),
        volumes,
        slug: data.game_urls.get(app).map(|url| url.slug.clone()),
        aliases: data
            .game_urls
            .get(app)
            .map(|url| url.aliases.clone())
            .unwrap_or_default(),
    })
}

fn server_persistent(data: &platform::Data, runtime: &str) -> bool {
    data.deployed(runtime)
        .and_then(|game| {
            data.builds
                .get(&build_key(data.source_app(runtime), &game.build))
        })
        .and_then(|build| build.server.as_ref())
        .is_some_and(|server| server.persistent)
}

fn inventory_json(inventory: &Inventory) -> Value {
    json!({
        "releases": inventory.releases,
        "web_files": inventory.web_files,
        "web_bytes": inventory.web_bytes,
        "saves": inventory.saves,
        "screenshots": inventory.screenshots,
        "leaderboards": 0,
        "reactions": inventory.reactions,
        "cloud_checkpoints": inventory.cloud_checkpoints,
        "credentials": inventory.credentials,
        "publish_captures": inventory.captures,
        "containers": inventory.containers,
        "packaged_images": inventory.packaged_images,
        "world_volumes": inventory.volumes,
        "routing": {
            "slug": inventory.slug,
            "aliases": inventory.aliases,
            "play_prefix": inventory.slug.as_ref().map(|slug| format!("{slug}--g-")),
        },
    })
}

fn removal_lines(inventory: &Inventory) -> Vec<String> {
    let mut lines = vec![
        format!(
            "{} and {} ({} bytes).",
            count(inventory.releases, "release", "releases"),
            count(inventory.web_files, "web file", "web files"),
            inventory.web_bytes
        ),
        format!("{}.", count(inventory.saves, "save", "saves")),
        format!(
            "{}.",
            count(inventory.screenshots, "screenshot", "screenshots")
        ),
        "No separate leaderboard is stored. Scores kept in saves are removed with those saves."
            .to_owned(),
        format!(
            "{}, {}, and {}.",
            count(inventory.reactions, "reaction", "reactions"),
            count(
                inventory.cloud_checkpoints,
                "cloud checkpoint",
                "cloud checkpoints"
            ),
            count(
                inventory.credentials,
                "publishing connection",
                "publishing connections"
            )
        ),
        format!(
            "{}.",
            count(inventory.captures, "publish capture", "publish captures")
        ),
    ];
    if inventory.containers.is_empty()
        && inventory.packaged_images.is_empty()
        && inventory.volumes == 0
    {
        lines.push("No dedicated server container, world volume, or packaged image.".to_owned());
    } else {
        lines.push(format!(
            "{}, {}, and {}. Operator-supplied images that were not built by Infuser stay on the host.",
            count(
                inventory.containers.len() as u64,
                "server record",
                "server records"
            ),
            count(inventory.volumes, "world volume", "world volumes"),
            count(
                inventory.packaged_images.len() as u64,
                "packaged image",
                "packaged images"
            )
        ));
    }
    let address = match &inventory.slug {
        Some(slug) => {
            let older = if inventory.aliases.is_empty() {
                "no older addresses".to_owned()
            } else {
                format!("older addresses {}", inventory.aliases.join(", "))
            };
            format!(
                "Address {slug} ({older}). Play links use {slug}--g-… on the shared wildcard host. No separate DNS record is removed. The address can be used by a future game; old links will not redirect."
            )
        }
        None => "No public address is assigned.".to_owned(),
    };
    lines.push(address);
    lines
}

fn count(n: u64, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

fn dir_stats(path: &std::path::Path) -> Result<(u64, u64)> {
    if !path.exists() {
        return Ok((0, 0));
    }
    let mut files = 0;
    let mut bytes = 0;
    fn walk(path: &std::path::Path, files: &mut u64, bytes: &mut u64) -> Result<()> {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                continue;
            } else if kind.is_dir() {
                walk(&entry.path(), files, bytes)?;
            } else if kind.is_file() {
                *files += 1;
                *bytes += entry.metadata()?.len();
            }
        }
        Ok(())
    }
    walk(path, &mut files, &mut bytes)?;
    Ok((files, bytes))
}

fn remove_files(p: &Platform, app: &str, ids: &[String]) -> Result<()> {
    if let Some(path) = owned_child(&p.root(), app) {
        remove_path(&path)?;
    }
    let state = &p.config().controller.state_dir;
    if let Some(path) = owned_child(&state.join("server-sources"), app) {
        remove_path(&path)?;
    }
    for id in ids {
        for directory in ["server-keys", "server-sources"] {
            if let Some(path) = owned_child(&state.join(directory), id) {
                remove_path(&path)?;
            }
        }
        if let Some(path) = diagnostics_file(&state.join("runtime-diagnostics"), id) {
            remove_path(&path)?;
        }
    }
    Ok(())
}

fn owned_child(parent: &std::path::Path, name: &str) -> Option<PathBuf> {
    if !slug(name) {
        return None;
    }
    let path = parent.join(name);
    (path.file_name().and_then(|value| value.to_str()) == Some(name)).then_some(path)
}

fn diagnostics_file(dir: &std::path::Path, id: &str) -> Option<PathBuf> {
    if !slug(id) {
        return None;
    }
    let name = format!("{id}.json");
    let path = dir.join(&name);
    (path.file_name().and_then(|value| value.to_str()) == Some(name.as_str())).then_some(path)
}

fn remove_path(path: &std::path::Path) -> Result<()> {
    let kind = match fs::symlink_metadata(path) {
        Ok(meta) => meta.file_type(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if kind.is_symlink() || kind.is_file() {
        fs::remove_file(path)?;
    } else if kind.is_dir() {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::normalize_title;

    #[test]
    fn titles_trim_and_ignore_case_only() {
        assert_eq!(normalize_title("  Pond "), "pond");
        assert_eq!(normalize_title("POND"), normalize_title("pond"));
        assert_ne!(normalize_title("Pond  Life"), normalize_title("Pond Life"));
        assert_ne!(normalize_title("Pond-Life"), normalize_title("Pond Life"));
    }
}
