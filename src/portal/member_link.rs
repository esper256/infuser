use super::*;
use crate::platform::member_link::{Redeemed, set_cookie};

pub(super) async fn redeem(
    State(p): State<Platform>,
    Path(token): Path<String>,
    h: HeaderMap,
) -> Answer {
    if host(&h) != p.domain() || prefetch(&h) {
        return Err(fault(404, "not found").into());
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
