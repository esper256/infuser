use super::*;
use crate::notifications::Subscription;

pub(super) fn routes() -> Router<Platform> {
    let mut router = Router::new().route(
        "/api/notifications",
        get(settings)
            .put(subscribe)
            .delete(unsubscribe)
            .layer(DefaultBodyLimit::max(4096)),
    );
    router = router.route(
        "/api/notifications/test",
        post(test_notification).layer(DefaultBodyLimit::max(4096)),
    );
    for path in [
        "/manifest.webmanifest",
        "/sw.js",
        "/assets/pwa.js",
        "/assets/app-192.png",
        "/assets/app-512.png",
        "/assets/apple-touch-icon.png",
    ] {
        router = router.route(path, get(asset));
    }
    router
}
async fn asset(State(p): State<Platform>, request: Request) -> Answer {
    p.main_host(request.headers())?;
    let (kind, body): (&'static str, &'static [u8]) = match request.uri().path() {
        "/manifest.webmanifest" => (
            "application/manifest+json",
            include_bytes!("../../web/manifest.webmanifest"),
        ),
        "/sw.js" => ("text/javascript", include_bytes!("../../web/sw.js")),
        "/assets/pwa.js" => ("text/javascript", include_bytes!("../../web/pwa.js")),
        "/assets/app-192.png" => (
            "image/png",
            include_bytes!("../../assets/branding/infuser/icon-192.png"),
        ),
        "/assets/app-512.png" => (
            "image/png",
            include_bytes!("../../assets/branding/infuser/icon-512.png"),
        ),
        "/assets/apple-touch-icon.png" => (
            "image/png",
            include_bytes!("../../assets/branding/infuser/apple-touch-icon.png"),
        ),
        _ => return Err(fault(404, "Not found.").into()),
    };
    let mut response = conditional::static_asset(request.headers(), kind, body);
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        "default-src 'self'; connect-src 'self'; script-src 'self'"
            .parse()
            .unwrap(),
    );
    Ok(response)
}
async fn settings(State(p): State<Platform>, h: HeaderMap) -> Answer {
    p.member(&h, false).await?;
    Ok(reply(json!({"public_key":p.push.public_key})))
}
async fn subscribe(
    State(p): State<Platform>,
    h: HeaderMap,
    Json(sub): Json<Subscription>,
) -> Answer {
    let email = p.member(&h, true).await?;
    p.db.push_subscribe(&email, &sub)?;
    Ok(reply(json!({"ok":true})))
}
async fn unsubscribe(
    State(p): State<Platform>,
    h: HeaderMap,
    Json(body): Json<Subscription>,
) -> Answer {
    let email = p.member(&h, true).await?;
    p.db.push_unsubscribe(&email, &body.endpoint)?;
    Ok(reply(json!({"ok":true})))
}
async fn test_notification(
    State(p): State<Platform>,
    h: HeaderMap,
    Json(sub): Json<Subscription>,
) -> Answer {
    let profile = p.member(&h, true).await?;
    sub.validate()?;
    p.db.reserve_push_test(&profile, &sub.endpoint)?;
    let request = p.push.request(&sub, &format!("https://{}", p.domain()))?;
    let response = p
        .http
        .execute(request)
        .await
        .map_err(|_| fault(502, "The push service could not be reached. Try again."))?;
    if matches!(response.status().as_u16(), 404 | 410) {
        p.db.push_unsubscribe(&profile, &sub.endpoint)?;
        return Err(fault(
            410,
            "This subscription expired. Turn notifications off and on again.",
        )
        .into());
    }
    ensure!(
        response.status().is_success(),
        fault(502, "The push service did not accept the test. Try again.")
    );
    Ok(reply(json!({"ok":true})))
}
impl Platform {
    pub async fn dispatch_push(&self) -> Result<()> {
        let Ok(_guard) = self.push_dispatch.try_lock() else {
            return Ok(());
        };
        use futures_util::{StreamExt, stream};
        let targets = self.db.push_targets()?;
        let subject = format!("https://{}", self.domain());
        stream::iter(targets)
            .map(|(endpoint, id, sub)| {
                let subject = &subject;
                async move {
                    let status = match self.push.request(&sub, subject) {
                        Ok(request) => self
                            .http
                            .execute(request)
                            .await
                            .ok()
                            .map(|r| r.status().as_u16()),
                        Err(_) => Some(400),
                    };
                    if status.is_none_or(|s| !(200..300).contains(&s)) {
                        // Endpoints and subscription keys are credentials: never log them.
                        tracing::warn!(status, "chat push was not accepted");
                    }
                    self.db.push_result(&endpoint, id, status)
                }
            })
            .buffer_unordered(4)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>>>()?;
        Ok(())
    }
}
