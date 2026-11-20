use super::*;
use crate::platform::member_link::{Redeemed, clear_cookie, set_cookie};

pub(super) async fn preview(
    State(p): State<Platform>,
    Path(token): Path<String>,
    h: HeaderMap,
) -> Answer {
    if host(&h) != p.domain() || prefetch(&h) {
        return Err(fault(404, "not found").into());
    }
    let people = p.db.preview_member_link(&token)?;
    let mut response = Html(confirm_page(&token, &people)).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    headers.insert(header::REFERRER_POLICY, "no-referrer".parse().unwrap());
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        "default-src 'none'; style-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'"
            .parse()
            .unwrap(),
    );
    Ok(response)
}

pub(super) async fn redeem(
    State(p): State<Platform>,
    Path(token): Path<String>,
    h: HeaderMap,
) -> Answer {
    if host(&h) != p.domain() || prefetch(&h) {
        return Err(fault(404, "not found").into());
    }
    let origin = format!("https://{}", p.domain());
    if h.get("origin").and_then(|value| value.to_str().ok()) != Some(origin.as_str()) {
        return Err(fault(403, "Open this link in Infuser and press Continue.").into());
    }
    let session = match p.db.redeem_member_link(&token)? {
        Redeemed::Session(session) => session,
        Redeemed::Refused => {
            return Err(fault(403, "This sign-in link cannot be used for this account.").into());
        }
    };
    let mut response = StatusCode::SEE_OTHER.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::SET_COOKIE,
        set_cookie(&session)
            .parse()
            .map_err(|_| fault(500, "Could not start the session."))?,
    );
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    headers.insert(header::REFERRER_POLICY, "no-referrer".parse().unwrap());
    headers.insert(
        header::LOCATION,
        format!("https://{}/library", p.domain())
            .parse()
            .map_err(|_| fault(500, "Could not start the session."))?,
    );
    Ok(response)
}

pub(super) async fn sign_out(State(p): State<Platform>, h: HeaderMap) -> Answer {
    if host(&h) != p.domain() {
        return Err(fault(404, "not found").into());
    }
    if let Some(token) = h
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(crate::platform::member_link::cookie_value)
    {
        p.db.end_member_session(token)?;
    }
    let mut response = StatusCode::SEE_OTHER.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::SET_COOKIE,
        clear_cookie()
            .parse()
            .map_err(|_| fault(500, "Could not sign out."))?,
    );
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    headers.insert(
        header::LOCATION,
        "/oauth2/sign_out?rd=%2F"
            .parse()
            .map_err(|_| fault(500, "Could not sign out."))?,
    );
    Ok(response)
}

pub(super) async fn mint(
    State(p): State<Platform>,
    Path(member): Path<String>,
    h: HeaderMap,
) -> Answer {
    let actor = p.operator(&h, true).await?;
    let (group, _) = p.group_view(&h, &actor)?;
    let key =
        p.db.catalog()?
            .group_member_key(&member, &group)?
            .ok_or_else(|| fault(404, "Member not found."))?;
    let (url, expires) =
        p.db.mint_member_link(p.domain(), &key, &actor)
            .map_err(mint_error)?;
    Ok(reply(json!({"url": url, "expires_at": expires})))
}

pub(super) async fn revoke(
    State(p): State<Platform>,
    Path(member): Path<String>,
    h: HeaderMap,
) -> Answer {
    let actor = p.operator(&h, true).await?;
    let (group, _) = p.group_view(&h, &actor)?;
    let key =
        p.db.catalog()?
            .group_member_key(&member, &group)?
            .ok_or_else(|| fault(404, "Member not found."))?;
    p.db.revoke_member_links(&key)?;
    Ok(reply(json!({"ok": true})))
}

fn mint_error(error: anyhow::Error) -> Error {
    if error.to_string().contains("Google") {
        return fault(403, "This account signs in with Google.").into();
    }
    error.into()
}

fn confirm_page(token: &str, people: &[(String, String)]) -> String {
    let lines = people
        .iter()
        .map(|(name, group)| format!("<li>{} in {}</li>", escape(name), escape(group)))
        .collect::<String>();
    format!(
        r#"<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="referrer" content="no-referrer"><title>Sign in · Infuser</title><link rel="stylesheet" href="/assets/clubhouse.css" referrerpolicy="no-referrer"><link rel="stylesheet" href="/assets/themes/nightglass.css" referrerpolicy="no-referrer"><body class="member-link-page"><main class="member-link-confirm"><h1>Sign in to Infuser</h1><p>This link signs in as:</p><ul>{lines}</ul><form method="post" action="/member-link/{token}"><button type="submit">Continue</button></form><p class="quiet">The link works once. Continue only if this is the right person.</p></main>"#
    )
}

fn escape(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            '&' => "&amp;".into(),
            '<' => "&lt;".into(),
            '>' => "&gt;".into(),
            '"' => "&quot;".into(),
            _ => c.to_string(),
        })
        .collect()
}

fn prefetch(h: &HeaderMap) -> bool {
    ["sec-purpose", "purpose"].into_iter().any(|name| {
        h.get(name)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value.split([',', ';']).any(|part| {
                    let part = part.trim();
                    part.eq_ignore_ascii_case("prefetch") || part.eq_ignore_ascii_case("preview")
                })
            })
    })
}
