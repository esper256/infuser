use super::*;

pub(super) fn url(
    app: &str,
    build: &str,
    data: &Build,
    metadata: Option<&platform::GameMetadata>,
) -> Option<String> {
    if let Some(revision) = metadata.and_then(|m| m.card_art_revision.as_deref()) {
        return Some(format!("/api/apps/{app}/card-art/{revision}"));
    }
    data.files
        .contains_key(crate::card_art::PATH)
        .then(|| format!("/api/apps/{app}/card-art/{build}"))
}

#[derive(Deserialize)]
pub(super) struct ImageQuery {
    inspect_group: Option<String>,
}

pub(super) async fn image(
    State(p): State<Platform>,
    Path((app, build)): Path<(String, String)>,
    mut h: HeaderMap,
    Query(query): Query<ImageQuery>,
    mut request: Request,
) -> Answer {
    if let Some(group) = query.inspect_group {
        h.insert(
            groups::INSPECT,
            group.parse().map_err(|_| fault(400, "Invalid group."))?,
        );
    }
    let email = p.member(&h, false).await?;
    p.visible_app(&h, &email, &app)?;
    let (_, inspecting) = p.group_view(&h, &email)?;
    {
        let data = p.db.read()?;
        let active = data
            .apps
            .get(&app)
            .ok_or_else(|| fault(404, "game not found"))?;
        ensure!(
            active.listed || inspecting || data.members[&email].admin,
            fault(404, "game not found")
        );
        if data
            .metadata
            .get(&app)
            .and_then(|m| m.card_art_revision.as_deref())
            == Some(build.as_str())
        {
            let bytes =
                p.db.metadata_art(&app, &build)?
                    .ok_or_else(|| fault(404, "artwork was replaced"))?;
            return Ok(image_response(Response::new(Body::from(bytes))));
        }
        ensure!(
            active.build == build,
            fault(404, "game has no current art at this revision")
        );
        ensure!(
            data.builds[&build_key(&app, &build)]
                .files
                .contains_key(crate::card_art::PATH),
            fault(404, "game has no card art")
        );
    }
    request.headers_mut().remove("cookie");
    request.headers_mut().remove("authorization");
    let path = p.root().join(app).join(build).join(crate::card_art::PATH);
    let response = ServeFile::new(path)
        .oneshot(request)
        .await
        .unwrap()
        .map(Body::new);
    Ok(image_response(response))
}
fn image_response(mut response: Response) -> Response {
    for (key, value) in [
        ("content-type", "image/webp"),
        ("cache-control", "private, no-cache"),
        ("x-content-type-options", "nosniff"),
    ] {
        response.headers_mut().insert(
            header::HeaderName::from_static(key),
            header::HeaderValue::from_static(value),
        );
    }
    response
}
