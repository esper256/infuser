use super::*;
use std::collections::{HashMap, VecDeque};
use std::path::{Path as FilePath, PathBuf};
use std::sync::OnceLock;
use std::time::SystemTime;

pub(super) fn url(
    app: &str,
    build: &str,
    data: &Build,
    metadata: Option<&platform::GameMetadata>,
    root: &FilePath,
) -> Option<String> {
    if let Some(revision) = metadata.and_then(|m| m.card_art_revision.as_deref()) {
        return Some(format!("/api/apps/{app}/card-art/{revision}"));
    }
    data.files.contains_key(crate::card_art::PATH).then(|| {
        // A reused build id can point at new bytes. The digest keeps the URL immutable.
        match file_digest(&root.join(app).join(build).join(crate::card_art::PATH)) {
            Some(digest) => format!("/api/apps/{app}/card-art/{build}/{digest}"),
            None => format!("/api/apps/{app}/card-art/{build}"),
        }
    })
}

#[derive(Deserialize)]
pub(super) struct ImageQuery {
    inspect_group: Option<String>,
    visit: Option<String>,
    w: Option<u32>,
}

struct DigestCache {
    len: u64,
    modified: Option<SystemTime>,
    digest: String,
}

fn digests() -> &'static Mutex<HashMap<PathBuf, DigestCache>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, DigestCache>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn remember_digest(path: &FilePath, digest: &str) {
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if let Ok(mut cache) = digests().lock() {
        cache.insert(
            path.to_path_buf(),
            DigestCache {
                len: meta.len(),
                modified: meta.modified().ok(),
                digest: digest.to_owned(),
            },
        );
    }
}

fn file_digest(path: &FilePath) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let len = meta.len();
    let modified = meta.modified().ok();
    if let Ok(cache) = digests().lock()
        && let Some(hit) = cache.get(path)
        && hit.len == len
        && hit.modified == modified
    {
        return Some(hit.digest.clone());
    }
    let bytes = std::fs::read(path).ok()?;
    let digest = crate::card_art::sha256_hex(&bytes);
    if let Ok(mut cache) = digests().lock() {
        cache.insert(
            path.to_path_buf(),
            DigestCache {
                len,
                modified,
                digest: digest.clone(),
            },
        );
    }
    Some(digest)
}

struct VariantCache {
    images: HashMap<(String, u32), Arc<[u8]>>,
    order: VecDeque<(String, u32)>,
}

fn variants() -> &'static Mutex<VariantCache> {
    static CACHE: OnceLock<Mutex<VariantCache>> = OnceLock::new();
    CACHE.get_or_init(|| {
        Mutex::new(VariantCache {
            images: HashMap::new(),
            order: VecDeque::new(),
        })
    })
}

fn derivative(master: &[u8], digest: &str, width: u32) -> Result<Arc<[u8]>> {
    let key = (digest.to_owned(), width);
    if let Ok(cache) = variants().lock()
        && let Some(hit) = cache.images.get(&key)
    {
        return Ok(Arc::clone(hit));
    }
    let encoded = crate::card_art::derivative(master, width)?;
    let bytes: Arc<[u8]> = Arc::from(encoded.into_boxed_slice());
    if let Ok(mut cache) = variants().lock()
        && let std::collections::hash_map::Entry::Vacant(slot) = cache.images.entry(key.clone())
    {
        slot.insert(Arc::clone(&bytes));
        cache.order.push_back(key);
        while cache.order.len() > 128 {
            if let Some(old) = cache.order.pop_front() {
                cache.images.remove(&old);
            }
        }
    }
    Ok(bytes)
}

pub(super) async fn image(
    State(p): State<Platform>,
    Path((app, build)): Path<(String, String)>,
    h: HeaderMap,
    Query(query): Query<ImageQuery>,
) -> Answer {
    serve(&p, app, build, None, h, query).await
}

pub(super) async fn addressed(
    State(p): State<Platform>,
    Path((app, build, digest)): Path<(String, String, String)>,
    h: HeaderMap,
    Query(query): Query<ImageQuery>,
) -> Answer {
    serve(&p, app, build, Some(digest), h, query).await
}

async fn serve(
    p: &Platform,
    app: String,
    build: String,
    addressed: Option<String>,
    mut h: HeaderMap,
    query: ImageQuery,
) -> Answer {
    if let Some(visit) = query.visit {
        h.insert(
            groups::VISIT,
            visit.parse().map_err(|_| fault(400, "Invalid visit."))?,
        );
    }
    if let Some(group) = query.inspect_group {
        h.insert(
            groups::INSPECT,
            group.parse().map_err(|_| fault(400, "Invalid group."))?,
        );
    }
    let width = match query.w {
        None | Some(1024) => None,
        Some(width) if crate::card_art::DERIVATIVE_WIDTHS.contains(&width) => Some(width),
        Some(_) => return Err(fault(400, "Choose a 384, 640, or 768 pixel cover.").into()),
    };
    let email = p.member(&h, false).await?;
    p.visible_app(&h, &email, &app)?;
    let (bytes, digest, immutable) = {
        let data = p.db.catalog()?;
        let (group, _) = p.group_view(&h, &email)?;
        let runtime = data
            .deployment_for(&app, &group)
            .ok_or_else(|| fault(404, "Game not found."))?;
        let active = data
            .deployed(runtime)
            .ok_or_else(|| fault(404, "game not found"))?;
        if data
            .metadata
            .get(&app)
            .and_then(|m| m.card_art_revision.as_deref())
            == Some(build.as_str())
        {
            ensure!(
                addressed.is_none(),
                fault(404, "game has no current art at this revision")
            );
            let bytes =
                p.db.metadata_art(&app, &build)?
                    .ok_or_else(|| fault(404, "artwork was replaced"))?;
            // The revision is the content hash, so this URL cannot change bytes.
            (bytes, build.clone(), true)
        } else {
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
            let path = p.root().join(&app).join(&build).join(crate::card_art::PATH);
            let bytes = tokio::fs::read(&path).await?;
            let digest = crate::card_art::sha256_hex(&bytes);
            remember_digest(&path, &digest);
            if let Some(expected) = &addressed {
                ensure!(expected == &digest, fault(404, "artwork was replaced"));
            }
            // Build ids can be reused after deletion, so only a digest URL is immutable.
            (bytes, digest, addressed.is_some())
        }
    };
    let etag = match width {
        Some(width) => format!("\"{digest}-{width}\""),
        None => format!("\"{digest}\""),
    };
    if conditional::matches(&h, &etag) {
        let mut response = StatusCode::NOT_MODIFIED.into_response();
        cache_headers(&mut response, &etag, immutable);
        return Ok(response);
    }
    let body = match width {
        Some(width) => {
            let master = bytes;
            let digest = digest.clone();
            tokio::task::spawn_blocking(move || derivative(&master, &digest, width))
                .await
                .map_err(|_| fault(500, "Something went wrong. Please try again."))??
        }
        None => Arc::<[u8]>::from(bytes),
    };
    let mut response = (
        [
            (header::CONTENT_TYPE, "image/webp"),
            (
                header::HeaderName::from_static("x-content-type-options"),
                "nosniff",
            ),
        ],
        body.as_ref().to_vec(),
    )
        .into_response();
    cache_headers(&mut response, &etag, immutable);
    Ok(response)
}

fn cache_headers(response: &mut Response, etag: &str, immutable: bool) {
    let cache = if immutable {
        "private, max-age=31536000, immutable"
    } else {
        "private, no-cache"
    };
    let headers = response.headers_mut();
    headers.insert(header::ETAG, etag.parse().unwrap());
    headers.insert(header::CACHE_CONTROL, cache.parse().unwrap());
    if !immutable {
        headers.insert(
            header::VARY,
            "Cookie, X-Infuser-Profile, X-Infuser-Inspect-Group, X-Infuser-Visit"
                .parse()
                .unwrap(),
        );
    }
}
