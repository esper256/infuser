use super::*;

pub(super) fn title<'a>(data: &'a platform::Data, app: &str, fallback: &'a str) -> &'a str {
    data.metadata
        .get(app)
        .and_then(|m| m.title.as_deref())
        .unwrap_or(fallback)
}

pub(super) fn art_pending(data: &platform::Data, app: &str) -> bool {
    data.metadata.get(app).is_some_and(|metadata| {
        metadata
            .art_prompt
            .as_deref()
            .is_some_and(|prompt| Some(hash(prompt)) != metadata.art_prompt_completed_revision)
    })
}

fn view(d: &platform::Data, app: &str) -> Result<Value> {
    let active = d.apps.get(app).ok_or_else(|| fault(404, "unknown game"))?;
    let build = &d.builds[&build_key(app, &active.build)];
    let metadata = d.metadata.get(app).cloned().unwrap_or_default();
    let prompt_revision = metadata.art_prompt.as_deref().map(hash);
    let prompt_pending = art_pending(d, app);
    Ok(
        json!({"app_id":app,"private_group":d.groups.app(app),"build":active.build,"listed":active.listed,
        "title":title(d,app,&build.manifest.title),"title_override":metadata.title,
        "description":metadata.description,
        "art_prompt":metadata.art_prompt,"art_prompt_revision":prompt_revision,"art_prompt_pending":prompt_pending,
        "card_art_url":card_art::url(app,&active.build,build,d.metadata.get(app)),
        "card_art_override":metadata.card_art_revision,"updated":metadata.updated,
        "tags":d.tags.get(app).cloned().unwrap_or_default(),
        "slug":d.game_urls.get(app).map(|u|&u.slug)}),
    )
}

pub(super) fn authorize(p: &Platform, h: &HeaderMap, app: &str) -> Result<()> {
    if p.publisher(h, Some("all-games"), "metadata").is_ok() {
        return Ok(());
    }
    let group = p.db.read()?.groups.app(app).to_owned();
    if p.publisher(h, Some(&group), "metadata-group").is_ok() {
        return Ok(());
    }
    p.publisher(h, Some(app), "publish")?;
    Ok(())
}

pub(super) async fn list_metadata(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let group = if p.publisher(&h, Some("all-games"), "metadata").is_ok() {
        None
    } else {
        Some(p.publisher(&h, None, "metadata-group")?)
    };
    let data = p.db.read()?;
    let apps = data
        .apps
        .keys()
        .filter(|app| {
            group
                .as_ref()
                .is_none_or(|group| data.groups.app(app) == group)
        })
        .map(|app| view(&data, app))
        .collect::<Result<Vec<_>>>()?;
    Ok(reply(
        json!({"version":1,"apps":apps,"description_max_chars":2000,
        "art_prompt_max_chars":4000,"card_art":crate::card_art::capabilities()}),
    ))
}

pub(super) async fn get_metadata(
    State(p): State<Platform>,
    Path(app): Path<String>,
    h: HeaderMap,
) -> Answer {
    authorize(&p, &h, &app)?;
    Ok(reply(view(&p.db.read()?, &app)?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Patch {
    #[serde(default, deserialize_with = "present_text")]
    title: Option<Option<String>>,
    #[serde(default, deserialize_with = "present_text")]
    description: Option<Option<String>>,
    #[serde(default, deserialize_with = "present_text")]
    art_prompt: Option<Option<String>>,
}
fn present_text<'de, D: serde::Deserializer<'de>>(
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
    authorize(&p, &h, &app)?;
    ensure!(
        body.title.is_some() || body.description.is_some() || body.art_prompt.is_some(),
        fault(
            400,
            "provide title, description, or art_prompt; null clears that field"
        )
    );
    let title = body.title.as_ref().map(|v| v.as_deref().map(str::trim));
    let description = body
        .description
        .as_ref()
        .map(|v| v.as_deref().map(str::trim));
    let art_prompt = body
        .art_prompt
        .as_ref()
        .map(|v| v.as_deref().map(str::trim));
    if let Some(Some(title)) = title {
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
    if let Some(Some(description)) = description {
        ensure!(
            !description.is_empty()
                && description.chars().count() <= 2000
                && !description.chars().any(char::is_control),
            fault(
                422,
                "description must be 1–2000 characters without control characters"
            )
        );
    }
    if let Some(Some(prompt)) = art_prompt {
        ensure!(
            !prompt.is_empty()
                && prompt.chars().count() <= 4000
                && !prompt.chars().any(|c| c.is_control() && c != '\n'),
            fault(
                422,
                "art_prompt must be 1–4000 characters; newlines are allowed"
            )
        );
    }
    p.db.metadata_update(
        &app,
        platform::MetadataUpdate {
            title,
            description,
            art_prompt,
            ..Default::default()
        },
    )?;
    Ok(reply(view(&p.db.read()?, &app)?))
}

pub(super) async fn put_art(
    State(p): State<Platform>,
    Path(app): Path<String>,
    h: HeaderMap,
    body: Bytes,
) -> Answer {
    authorize(&p, &h, &app)?;
    crate::card_art::validate(&body).map_err(|e| fault(422, &format!("{e:#}")))?;
    let _guard = p.publishing.lock().unwrap();
    let d = p.db.read()?;
    ensure!(d.apps.contains_key(&app), fault(404, "unknown game"));
    let prior = d.metadata.get(&app).map_or(0, |m| m.card_art_bytes);
    let total = super::server_builds::content_bytes(&d);
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
    let complete_prompt = h
        .get("x-infuser-art-prompt-revision")
        .map(|v| {
            v.to_str()
                .map_err(|_| fault(400, "invalid art prompt revision"))
        })
        .transpose()?;
    p.db.metadata_update(
        &app,
        platform::MetadataUpdate {
            art: Some(Some(&body)),
            complete_prompt,
            ..Default::default()
        },
    )?;
    Ok(reply(view(&p.db.read()?, &app)?))
}

pub(super) async fn delete_art(
    State(p): State<Platform>,
    Path(app): Path<String>,
    h: HeaderMap,
) -> Answer {
    authorize(&p, &h, &app)?;
    p.db.metadata_update(
        &app,
        platform::MetadataUpdate {
            art: Some(None),
            ..Default::default()
        },
    )?;
    Ok(reply(view(&p.db.read()?, &app)?))
}
