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
    let (kind, body): (&str, &[u8]) = match request.uri().path() {
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
    Ok((
        [
            (header::CONTENT_TYPE, kind),
            (header::CACHE_CONTROL, "no-cache"),
            (
                header::CONTENT_SECURITY_POLICY,
                "default-src 'self'; connect-src 'self'; script-src 'self'",
            ),
        ],
        body,
    )
        .into_response())
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
