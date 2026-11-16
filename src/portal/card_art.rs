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
    visit: Option<String>,
}

pub(super) async fn image(
    State(p): State<Platform>,
    Path((app, build)): Path<(String, String)>,
    mut h: HeaderMap,
    Query(query): Query<ImageQuery>,
    mut request: Request,
) -> Answer {
    if let Some(visit)=query.visit {h.insert(groups::VISIT,visit.parse().map_err(|_|fault(400,"Invalid visit."))?);}
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
        let (group,_)=p.group_view(&h,&email)?;
        let runtime=data.deployment_for(&app,&group).ok_or_else(||fault(404,"Game not found."))?;
        let active = data.deployed(runtime)
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
            let etag=format!("\"{build}\"");
            if conditional::matches(&h,&etag) {
                let mut response=StatusCode::NOT_MODIFIED.into_response();
                conditional::headers(&mut response,&etag);return Ok(response);
            }
            let bytes =
                p.db.metadata_art(&app, &build)?
                    .ok_or_else(|| fault(404, "artwork was replaced"))?;
            let mut response=image_response(Response::new(Body::from(bytes)));
            conditional::headers(&mut response,&etag);return Ok(response);
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
    // Build IDs can be reused after deletion; validate bytes rather than the ID.
    use sha2::{Digest,Sha256};
    let bytes=tokio::fs::read(&path).await?;
    let digest:String=Sha256::digest(&bytes).iter().map(|b|format!("{b:02x}")).collect();
    let etag=format!("\"{digest}\"");
    if conditional::matches(&h,&etag) {
        let mut response=StatusCode::NOT_MODIFIED.into_response();
        conditional::headers(&mut response,&etag);return Ok(response);
    }
    let response = ServeFile::new(path)
        .oneshot(request)
        .await
        .unwrap()
        .map(Body::new);
    let mut response=image_response(response);
    conditional::headers(&mut response,&etag);
    Ok(response)
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
