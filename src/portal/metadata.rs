use super::*;

pub(super) fn title<'a>(data: &'a platform::Data, app: &str, fallback: &'a str) -> &'a str {
    data.metadata
        .get(app)
        .and_then(|m| m.title.as_deref())
        .unwrap_or(fallback)
}

fn view(p: &Platform, app: &str) -> Result<Value> {
    let d = p.db.read()?;
    let active = d.apps.get(app).ok_or_else(|| fault(404, "unknown game"))?;
    let build = &d.builds[&build_key(app, &active.build)];
    let metadata = d.metadata.get(app).cloned().unwrap_or_default();
    Ok(
        json!({"app_id":app,"build":active.build,"listed":active.listed,
        "title":title(&d,app,&build.manifest.title),"title_override":metadata.title,
        "card_art_url":card_art::url(app,&active.build,build,d.metadata.get(app)),
        "card_art_override":metadata.card_art_revision,"updated":metadata.updated,
        "tags":d.tags.get(app).cloned().unwrap_or_default(),
        "slug":d.game_urls.get(app).map(|u|&u.slug)}),
    )
}

pub(super) async fn get_metadata(
    State(p): State<Platform>,
    Path(app): Path<String>,
    h: HeaderMap,
) -> Answer {
    p.publisher(&h, Some(&app), "publish")?;
    Ok(reply(view(&p, &app)?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Patch {
    #[serde(default, deserialize_with = "present_title")]
    title: Option<Option<String>>,
}
fn present_title<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<Option<String>>, D::Error> {
    Ok(Some(Option::<String>::deserialize(d)?))
}
pub(super) async fn patch_metadata(
    State(p): State<Platform>,
    Path(app): Path<String>,
    h: HeaderMap,
    Json(body): Json<Patch>,
) -> Answer {
    p.publisher(&h, Some(&app), "publish")?;
    let title = body
        .title
        .ok_or_else(|| fault(400, "provide title, or null to restore the build title"))?;
    let title = title.as_deref().map(str::trim);
    if let Some(title) = title {
        ensure!(
            !title.is_empty()
                && title.chars().count() <= 80
                && !title.chars().any(char::is_control),
            fault(
                422,
                "title must be 1–80 characters without control characters"
            )
        );
    }
    p.db.metadata_update(&app, Some(title), None)?;
    Ok(reply(view(&p, &app)?))
}

pub(super) async fn put_art(
    State(p): State<Platform>,
    Path(app): Path<String>,
    h: HeaderMap,
    body: Bytes,
) -> Answer {
    p.publisher(&h, Some(&app), "publish")?;
    crate::card_art::validate(&body).map_err(|e| fault(422, &format!("{e:#}")))?;
    let _guard = p.publishing.lock().unwrap();
    let d = p.db.read()?;
    ensure!(d.apps.contains_key(&app), fault(404, "unknown game"));
    let prior = d.metadata.get(&app).map_or(0, |m| m.card_art_bytes);
    let total = d
        .builds
        .values()
        .flat_map(|b| b.files.values())
        .sum::<u64>()
        + d.metadata.values().map(|m| m.card_art_bytes).sum::<u64>();
    let size = body.len() as u64;
    ensure!(
        total - prior + size <= p.config().controller.content_budget_mb * 1024 * 1024,
        fault(507, "installation content budget is full")
    );
    let disk = rustix::fs::statvfs(&p.config().controller.state_dir)?;
    ensure!(
        disk.f_bavail * disk.f_frsize
            >= p.config().controller.disk_reserve_mb * 1024 * 1024 + size * 2,
        fault(507, "host disk reserve would be exhausted")
    );
    p.db.metadata_update(&app, None, Some(Some(&body)))?;
    Ok(reply(view(&p, &app)?))
}

pub(super) async fn delete_art(
    State(p): State<Platform>,
    Path(app): Path<String>,
    h: HeaderMap,
) -> Answer {
    p.publisher(&h, Some(&app), "publish")?;
    p.db.metadata_update(&app, None, Some(None))?;
    Ok(reply(view(&p, &app)?))
}
