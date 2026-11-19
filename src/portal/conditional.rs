//! Conditional responses still authorize the current member/group on every GET.
use super::*;
pub(super) fn matches(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .any(|v| v == "*" || v.strip_prefix("W/").unwrap_or(v) == etag)
}
pub(super) fn headers(response: &mut Response, etag: &str) {
    response
        .headers_mut()
        .insert(header::ETAG, etag.parse().unwrap());
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "private, no-cache".parse().unwrap());
    response.headers_mut().insert(
        header::VARY,
        "Cookie, X-Infuser-Profile, X-Infuser-Inspect-Group, X-Infuser-Visit"
            .parse()
            .unwrap(),
    );
}
pub(super) fn json(value: Value, request: &HeaderMap) -> Response {
    // Hash the actual authorized view: deletions, retention expiry, profiles and
    // permissions all affect the validator, even without a new message ID.
    let body = value.to_string();
    let etag = format!("\"{}\"", hash(&body));
    let mut response = if matches(request, &etag) {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        ([(header::CONTENT_TYPE, "application/json")], body).into_response()
    };
    headers(&mut response, &etag);
    response
}
pub(super) fn static_asset(
    request: &HeaderMap,
    content_type: &'static str,
    body: &'static [u8],
) -> Response {
    use sha2::{Digest, Sha256};
    let digest: String = Sha256::digest(body)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let etag = format!("\"{digest}\"");
    let mut response = if matches(request, &etag) {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        ([(header::CONTENT_TYPE, content_type)], body).into_response()
    };
    response
        .headers_mut()
        .insert(header::ETAG, etag.parse().unwrap());
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-cache".parse().unwrap());
    response
}
