use super::*;

pub(super) fn url(app: &str, build: &str, data: &Build) -> Option<String> {
    data.files
        .contains_key(crate::card_art::PATH)
        .then(|| format!("/api/apps/{app}/card-art/{build}"))
}

pub(super) async fn image(
    State(p): State<Platform>,
    Path((app, build)): Path<(String, String)>,
    h: HeaderMap,
    mut request: Request,
) -> Answer {
    let email = p.member(&h, false).await?;
    {
        let data = p.db.read()?;
        let active = data
            .apps
            .get(&app)
            .ok_or_else(|| fault(404, "game not found"))?;
        ensure!(
            active.build == build && (active.listed || data.members[&email].admin),
            fault(404, "game not found")
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
    let mut response = ServeFile::new(path)
        .oneshot(request)
        .await
        .unwrap()
        .map(Body::new);
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
    Ok(response)
}
