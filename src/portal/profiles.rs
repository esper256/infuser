//! A profile selector is not a credential. The Google account authenticates
//! every request; selection is checked against that account's enabled profiles.
use super::*;
pub(super) const SELECTED: &str = "x-infuser-profile";
pub(super) const ACCOUNT: &str = "x-infuser-verified-account";

#[derive(Deserialize)]
struct ImageSelection {
    profile: Option<String>,
}
pub(super) fn requested(h: &HeaderMap, uri: &axum::http::Uri, get: bool) -> Result<Option<String>> {
    ensure!(
        h.get_all(SELECTED).iter().count() <= 1,
        fault(400, "Choose one profile.")
    );
    let header = h
        .get(SELECTED)
        .map(|v| v.to_str())
        .transpose()
        .map_err(|_| fault(400, "Invalid profile selection."))?;
    // An <img> cannot set headers. Authorize this selector against the same
    // signed-in account; accept query selection only on artwork GET requests.
    let image = if get && uri.path().starts_with("/api/apps/") && uri.path().contains("/card-art/")
    {
        Query::<ImageSelection>::try_from_uri(uri)
            .map_err(|_| fault(400, "Invalid profile selection."))?
            .0
            .profile
    } else {
        None
    };
    ensure!(
        header.is_none() || image.as_deref().is_none_or(|p| Some(p) == header),
        fault(400, "Choose one profile.")
    );
    Ok(header.map(str::to_owned).or(image))
}

pub(super) fn routes() -> Router<Platform> {
    Router::new()
        .route("/api/profiles", get(list))
        .route("/profiles", get(page))
        .route(
            "/assets/profiles.js",
            get(|h: HeaderMap| async move {
                conditional::static_asset(
                    &h,
                    "text/javascript",
                    include_bytes!("../../web/profiles.js"),
                )
            }),
        )
        .route(
            "/assets/profile-chooser.js",
            get(|h: HeaderMap| async move {
                conditional::static_asset(
                    &h,
                    "text/javascript",
                    include_bytes!("../../web/profile-chooser.js"),
                )
            }),
        )
}
async fn page(State(p): State<Platform>, h: HeaderMap) -> Answer {
    p.main_host(&h)?;
    Ok(([("Cache-Control","no-store"),("Content-Security-Policy","default-src 'self'; img-src 'self' data:; style-src 'self'; script-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'")],Html(include_str!("../../web/profiles.html"))).into_response())
}
async fn list(State(p): State<Platform>, h: HeaderMap) -> Answer {
    p.main_host(&h)?;
    let account = h
        .get(ACCOUNT)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| fault(401, "Please sign in."))?;
    let d = p.db.catalog()?;
    let profiles = d.account_profiles(account)?;
    ensure!(
        !profiles.is_empty(),
        fault(403, "Ask your admin for an Infuser invitation.")
    );
    Ok(reply(
        json!({"profiles":profiles.iter().map(|(key,profile)| {
        let member=&d.members[*key];let group=d.groups.member(key);
        json!({"player_id":profile.id,"name":member.name,"avatar":member.avatar,
            "group":{"id":group,"name":d.groups.names.get(group)}})
    }).collect::<Vec<_>>()}),
    ))
}
