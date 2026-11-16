use super::*;
fn view(data: &platform::Data, app: &str, domain: &str) -> Value {
    let slug = &data.game_urls[app].slug;
    json!({"app_id":app,"slug":slug,"url":format!("https://{domain}/games/{slug}")})
}
pub(super) async fn resolve(
    State(p): State<Platform>,
    Path(slug): Path<String>,
    h: HeaderMap,
) -> Answer {
    let email = p.member(&h, false).await?;
    let (group, inspecting) = p.group_view(&h, &email)?;
    let d = p.db.read()?;
    let app = crate::game_urls::resolve_in_group(&d, &slug, &group)
        .ok_or_else(|| fault(404, "Game unavailable."))?;
    let selected = d
        .apps
        .get(app)
        .filter(|a| a.listed || inspecting || d.members[&email].admin)
        .ok_or_else(|| fault(404, "Game unavailable."))?;
    let mut value = view(&d, app, p.domain());
    value["title"] = json!(metadata::title(
        &d,
        app,
        &d.builds[&build_key(app, &selected.build)].manifest.title
    ));
    Ok(reply(value))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Rename {
    slug: String,
}
pub(super) async fn publisher_rename(
    State(p): State<Platform>,
    Path(app): Path<String>,
    h: HeaderMap,
    Json(body): Json<Rename>,
) -> Answer {
    p.publisher(&h, Some(&app), "publish")?;
    rename(&p, &app, &body.slug)
}
pub(super) async fn owner_rename(
    State(p): State<Platform>,
    Path(app): Path<String>,
    h: HeaderMap,
    Json(body): Json<Rename>,
) -> Answer {
    let email = p.member(&h, true).await?;
    let d = p.db.read()?;
    d.require_app_group(&email, &app)?;
    ensure!(
        d.members[&email].admin || d.owners.get(&app) == Some(&email),
        fault(
            403,
            "Only the creator or an admin can change this game URL."
        )
    );
    rename(&p, &app, &body.slug)
}
fn rename(p: &Platform, app: &str, slug: &str) -> Answer {
    let value = p.db.edit(|d| {
        crate::game_urls::rename(d, app, slug)?;
        Ok(view(d, app, p.domain()))
    })?;
    Ok(reply(value))
}
