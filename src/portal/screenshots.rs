use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use image_webp::WebPDecoder;
use std::io::Cursor;

pub(super) fn routes() -> Router<Platform> {
    Router::new()
        .route(
            "/_infuser/screenshot-capture.js",
            get(|| async {
                (
                    [
                        ("content-type", "text/javascript"),
                        ("cache-control", "no-cache"),
                    ],
                    include_str!("../../web/screenshot-capture.js"),
                )
            }),
        )
        .route("/api/screenshots", get(list))
        .route("/api/screenshots/{id}", get(image))
        .route(
            "/_infuser/screenshots",
            post(capture).layer(DefaultBodyLimit::max(9 * 1024 * 1024)),
        )
        .route(
            "/assets/screenshots.js",
            get(|| async {
                (
                    [
                        ("content-type", "text/javascript"),
                        ("cache-control", "no-cache"),
                    ],
                    include_str!("../../web/screenshots.js"),
                )
            }),
        )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Upload {
    id: String,
    image: String,
    thumbnail: String,
}
fn decode(encoded: &str, limit: usize, pixels: u64, side: u32) -> Result<(Vec<u8>, u32, u32)> {
    ensure!(
        encoded.len() <= limit * 4 / 3 + 4,
        fault(413, "Screenshot is too large.")
    );
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| fault(422, "Invalid screenshot."))?;
    ensure!(bytes.len() <= limit, fault(413, "Screenshot is too large."));
    let mut decoder =
        WebPDecoder::new(Cursor::new(&bytes)).map_err(|_| fault(422, "Use a WebP screenshot."))?;
    let (w, h) = decoder.dimensions();
    ensure!(
        w > 0
            && h > 0
            && w <= side
            && h <= side
            && u64::from(w) * u64::from(h) <= pixels
            && !decoder.is_animated(),
        fault(422, "Unsupported screenshot dimensions.")
    );
    decoder.set_memory_limit(160 * 1024 * 1024);
    let channels = if decoder.has_alpha() { 4 } else { 3 };
    decoder
        .read_image(&mut vec![0; w as usize * h as usize * channels])
        .map_err(|_| fault(422, "Screenshot is incomplete."))?;
    Ok((bytes, w, h))
}
async fn capture(State(p): State<Platform>, h: HeaderMap, Json(body): Json<Upload>) -> Answer {
    static DECODING: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);
    let _permit = DECODING
        .try_acquire()
        .map_err(|_| fault(429, "Another screenshot is saving. Try again shortly."))?;
    p.origin(&h, host(&h))?;
    let (_, launch) = p.game(&h)?;
    ensure!(
        body.id.len() == 36 && body.id.bytes().all(|c| c.is_ascii_hexdigit() || c == b'-'),
        fault(422, "Invalid screenshot ID.")
    );
    // The client supplies a retry nonce, never the stored image's identity.
    // Evicted IDs cannot be reused to replace an old attachment with new pixels.
    let id = hash(&format!(
        "{}:{}:{}:{}:{}",
        launch.email,
        launch.app,
        body.id,
        hash(&body.image),
        hash(&body.thumbnail)
    ));
    let stored_id = id.clone();
    let upload = tokio::task::spawn_blocking(move || -> Result<_> {
        // Keep decoding bounded even if the requesting browser disconnects.
        let _permit = _permit;
        let (image, width, height) = decode(&body.image, 6 * 1024 * 1024, 33_554_432, 8192)?;
        let (thumbnail, _, _) = decode(&body.thumbnail, 96 * 1024, 320 * 320, 320)?;
        Ok(platform::screenshots::ScreenshotUpload {
            id: stored_id,
            image,
            thumbnail,
            width,
            height,
        })
    })
    .await
    .map_err(anyhow::Error::from)??;
    // Recheck the live launch after decoding; visits may have ended meanwhile.
    p.game(&h)?;
    p.db.screenshot_store(&launch.email, &launch.app, upload)?;
    Ok(reply(json!({"id":id,"saved":true})))
}
async fn list(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let owner = p.member(&h, false).await?;
    ensure!(
        !h.contains_key(groups::INSPECT),
        fault(403, "Return home to see your screenshots.")
    );
    Ok(reply(p.db.screenshot_list(&owner)?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImageQuery {
    #[serde(default)]
    thumbnail: bool,
    chat: Option<u64>,
    app: Option<String>,
}
async fn image(
    State(p): State<Platform>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<ImageQuery>,
) -> Answer {
    let email = p.member(&h, false).await?;
    let (group, inspecting) = p.group_view(&h, &email)?;
    let runtime = if let Some(app) = query.app {
        p.visible_app(&h, &email, &app)?;
        Some(
            p.db.read()?
                .deployment_for(&app, &group)
                .ok_or_else(|| fault(404, "Game not found."))?
                .to_owned(),
        )
    } else {
        None
    };
    let bytes = p.db.screenshot_image(
        &email,
        &id,
        &group,
        inspecting,
        query.chat,
        runtime.as_deref(),
        query.thumbnail,
    )?;
    let mut response =
        reply(json!({"image":format!("data:image/webp;base64,{}",STANDARD.encode(bytes))}));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "private, no-store".parse().unwrap());
    Ok(response)
}
