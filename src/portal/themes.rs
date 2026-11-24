//! Allowlisted Library theme files. Paths are exact; nothing is read from disk.
use super::*;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

const NIGHTGLASS_SCHEMES: &[&str] = &[
    "default",
    "deep-ocean",
    "twilight-violet",
    "amber-hearth",
    "rose-quartz",
    "cobalt-pulse",
    "forest-moss",
    "crimson-velvet",
    "northern-aurora",
    "copper-forge",
    "glacier-frost",
    "sunset-dusk",
    "neon-berry",
    "obsidian-gold",
    "custom",
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomColors {
    pub primary: String,
    pub accent: String,
    pub secondary: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Appearance {
    pub theme: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub schemes: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub custom: BTreeMap<String, CustomColors>,
}

fn valid_hex(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let bytes = trimmed.as_bytes();
    if bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(u8::is_ascii_hexdigit) {
        Some(trimmed.to_ascii_lowercase())
    } else {
        None
    }
}

fn known_theme(id: &str) -> bool {
    stylesheets().iter().any(|sheet| sheet.id == id)
}

fn schemes_for(theme: &str) -> &'static [&'static str] {
    match theme {
        "nightglass" => NIGHTGLASS_SCHEMES,
        _ => &[],
    }
}

impl Appearance {
    pub fn validate(self) -> Result<Self> {
        ensure!(
            known_theme(&self.theme),
            fault(400, "Choose a theme from the list.")
        );
        let mut schemes = BTreeMap::new();
        for (theme, scheme) in self.schemes {
            let allowed = schemes_for(&theme);
            ensure!(
                !allowed.is_empty(),
                fault(400, "That theme does not have color schemes.")
            );
            ensure!(
                allowed.contains(&scheme.as_str()),
                fault(400, "Choose a color scheme from the list.")
            );
            if scheme != "default" {
                schemes.insert(theme, scheme);
            }
        }
        let mut custom = BTreeMap::new();
        for (theme, colors) in self.custom {
            ensure!(
                schemes_for(&theme).contains(&"custom"),
                fault(400, "That theme does not support custom colors.")
            );
            let primary =
                valid_hex(&colors.primary).ok_or_else(|| fault(400, "Use a #RRGGBB hex color."))?;
            let accent =
                valid_hex(&colors.accent).ok_or_else(|| fault(400, "Use a #RRGGBB hex color."))?;
            let secondary = valid_hex(&colors.secondary)
                .ok_or_else(|| fault(400, "Use a #RRGGBB hex color."))?;
            custom.insert(
                theme,
                CustomColors {
                    primary,
                    accent,
                    secondary,
                },
            );
        }
        Ok(Self {
            theme: self.theme,
            schemes,
            custom,
        })
    }
}

pub(super) async fn get_appearance(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let email = p.member(&h, false).await?;
    let appearance = p.db.appearance_load(&email)?;
    Ok(reply(json!({"appearance": appearance})))
}

pub(super) async fn put_appearance(
    State(p): State<Platform>,
    h: HeaderMap,
    Json(body): Json<Appearance>,
) -> Answer {
    let email = p.member(&h, true).await?;
    let appearance = body.validate()?;
    ensure!(
        p.db.catalog()?.profile_enabled(&email),
        fault(403, "Your invitation is no longer active.")
    );
    if p.db.appearance_save(&email, &appearance)?
        && let Ok(d) = p.db.catalog()
    {
        let custom = appearance
            .schemes
            .get(&appearance.theme)
            .is_some_and(|s| s == "custom");
        p.controller.timeline.record(crate::timeline::Event::server(
            "settings.saved",
            crate::timeline::ContextFields {
                profile: d.player_id(&email),
                group: d.groups.member(&email).into(),
                ..Default::default()
            },
            json!({
                "theme": appearance.theme,
                "scheme_kind": if custom { "custom" } else { "builtin" },
                "revision": crate::timeline::ms()
            }),
            false,
        ));
    }
    Ok(reply(json!({"appearance": appearance})))
}

// Top-level `{id}.css` entries are the theme stylesheets. `script()` exposes
// that list to the client, which loads only the active sheet. `nightglass.css`
// stays linked from HTML as the base tokens. To add a theme, append its css
// and asset paths here, add `bytes` arms, and add a catalog entry in
// `web/themes.js`. Do not add a stylesheet `<link>` in HTML.
const FILES: &[&str] = &[
    "nightglass.css",
    "cartridge-club.css",
    "cartridge-club/lilita-one-latin.woff2",
    "cartridge-club/fredoka-600-latin.woff2",
    "cartridge-club/shelf.svg",
    "cartridge-club/grain.svg",
    "cartridge-club/OFL-lilita.txt",
    "cartridge-club/OFL-fredoka.txt",
    "cartridge-club/NOTICE.md",
    "funny-pages.css",
    "funny-pages/bangers-latin.woff2",
    "funny-pages/comic-neue-400-latin.woff2",
    "funny-pages/comic-neue-700-latin.woff2",
    "funny-pages/halftone.svg",
    "funny-pages/newsprint.svg",
    "funny-pages/OFL-bangers.txt",
    "funny-pages/OFL-comic-neue.txt",
    "funny-pages/NOTICE.md",
    "field-notes.css",
    "field-notes/newsreader-400-latin.woff2",
    "field-notes/newsreader-700-latin.woff2",
    "field-notes/caveat-700-latin.woff2",
    "field-notes/ruled.svg",
    "field-notes/OFL-newsreader.txt",
    "field-notes/OFL-caveat.txt",
    "field-notes/NOTICE.md",
    "castle-keep.css",
    "castle-keep/medievalsharp-latin.woff2",
    "castle-keep/nunito-400-latin.woff2",
    "castle-keep/nunito-700-latin.woff2",
    "castle-keep/stone.svg",
    "castle-keep/grass.svg",
    "castle-keep/oak.svg",
    "castle-keep/steel.svg",
    "castle-keep/parchment.svg",
    "castle-keep/OFL-medievalsharp.txt",
    "castle-keep/OFL-nunito.txt",
    "castle-keep/NOTICE.md",
];

pub(super) fn source_fingerprint() -> String {
    let mut hasher = Sha256::new();
    for path in FILES {
        let (_, body) = bytes(path).expect("listed theme asset");
        hasher.update(body);
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn digest(body: &[u8]) -> String {
    Sha256::digest(body)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

struct Stylesheet {
    id: &'static str,
    path: &'static str,
    version: String,
}

fn stylesheets() -> &'static [Stylesheet] {
    static SHEETS: OnceLock<Vec<Stylesheet>> = OnceLock::new();
    SHEETS.get_or_init(|| {
        FILES
            .iter()
            .filter_map(|path| {
                let id = path.strip_suffix(".css")?;
                if id.is_empty() || id.contains('/') {
                    return None;
                }
                let (_, body) = bytes(path)?;
                Some(Stylesheet {
                    id,
                    path,
                    version: digest(body),
                })
            })
            .collect()
    })
}

fn sheets_json() -> String {
    let sheets: Vec<Value> = stylesheets()
        .iter()
        .map(|sheet| {
            json!({
                "id": sheet.id,
                "href": format!("/assets/themes/{}?v={}", sheet.path, sheet.version),
            })
        })
        .collect();
    serde_json::to_string(&sheets).expect("theme sheet list")
}

/// `themes.js` plus the registry's stylesheet URLs, so a new `{id}.css` entry
/// is selectable without editing HTML.
pub(super) fn script() -> &'static [u8] {
    static BODY: OnceLock<Vec<u8>> = OnceLock::new();
    BODY.get_or_init(|| {
        let mut body = format!("globalThis.InfuserThemeSheets={};", sheets_json()).into_bytes();
        body.extend_from_slice(include_bytes!("../../web/themes.js"));
        body
    })
}

/// Fingerprint theme stylesheet links already in HTML. A plain URL still
/// works and revalidates; the `?v=` form is what the page asks for.
pub(super) fn fingerprint_html(html: &str) -> String {
    let mut html = html.to_string();
    for sheet in stylesheets() {
        let from = format!("href=\"/assets/themes/{}\"", sheet.path);
        let to = format!("href=\"/assets/themes/{}?v={}\"", sheet.path, sheet.version);
        html = html.replace(&from, &to);
    }
    html
}

fn requested_version(uri: &axum::http::Uri) -> Option<&str> {
    uri.query()?.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        (key == "v").then_some(value)
    })
}

fn bytes(path: &str) -> Option<(&'static str, &'static [u8])> {
    Some(match path {
        "nightglass.css" => (
            "text/css",
            include_bytes!("../../web/themes/nightglass.css").as_slice(),
        ),
        "cartridge-club.css" => (
            "text/css",
            include_bytes!("../../web/themes/cartridge-club.css").as_slice(),
        ),
        "cartridge-club/lilita-one-latin.woff2" => (
            "font/woff2",
            include_bytes!("../../web/themes/cartridge-club/lilita-one-latin.woff2").as_slice(),
        ),
        "cartridge-club/fredoka-600-latin.woff2" => (
            "font/woff2",
            include_bytes!("../../web/themes/cartridge-club/fredoka-600-latin.woff2").as_slice(),
        ),
        "cartridge-club/shelf.svg" => (
            "image/svg+xml",
            include_bytes!("../../web/themes/cartridge-club/shelf.svg").as_slice(),
        ),
        "cartridge-club/grain.svg" => (
            "image/svg+xml",
            include_bytes!("../../web/themes/cartridge-club/grain.svg").as_slice(),
        ),
        "cartridge-club/OFL-lilita.txt" => (
            "text/plain; charset=utf-8",
            include_bytes!("../../web/themes/cartridge-club/OFL-lilita.txt").as_slice(),
        ),
        "cartridge-club/OFL-fredoka.txt" => (
            "text/plain; charset=utf-8",
            include_bytes!("../../web/themes/cartridge-club/OFL-fredoka.txt").as_slice(),
        ),
        "cartridge-club/NOTICE.md" => (
            "text/plain; charset=utf-8",
            include_bytes!("../../web/themes/cartridge-club/NOTICE.md").as_slice(),
        ),
        "funny-pages.css" => (
            "text/css",
            include_bytes!("../../web/themes/funny-pages.css").as_slice(),
        ),
        "funny-pages/bangers-latin.woff2" => (
            "font/woff2",
            include_bytes!("../../web/themes/funny-pages/bangers-latin.woff2").as_slice(),
        ),
        "funny-pages/comic-neue-400-latin.woff2" => (
            "font/woff2",
            include_bytes!("../../web/themes/funny-pages/comic-neue-400-latin.woff2").as_slice(),
        ),
        "funny-pages/comic-neue-700-latin.woff2" => (
            "font/woff2",
            include_bytes!("../../web/themes/funny-pages/comic-neue-700-latin.woff2").as_slice(),
        ),
        "funny-pages/halftone.svg" => (
            "image/svg+xml",
            include_bytes!("../../web/themes/funny-pages/halftone.svg").as_slice(),
        ),
        "funny-pages/newsprint.svg" => (
            "image/svg+xml",
            include_bytes!("../../web/themes/funny-pages/newsprint.svg").as_slice(),
        ),
        "funny-pages/OFL-bangers.txt" => (
            "text/plain; charset=utf-8",
            include_bytes!("../../web/themes/funny-pages/OFL-bangers.txt").as_slice(),
        ),
        "funny-pages/OFL-comic-neue.txt" => (
            "text/plain; charset=utf-8",
            include_bytes!("../../web/themes/funny-pages/OFL-comic-neue.txt").as_slice(),
        ),
        "funny-pages/NOTICE.md" => (
            "text/plain; charset=utf-8",
            include_bytes!("../../web/themes/funny-pages/NOTICE.md").as_slice(),
        ),
        "field-notes.css" => (
            "text/css",
            include_bytes!("../../web/themes/field-notes.css").as_slice(),
        ),
        "field-notes/newsreader-400-latin.woff2" => (
            "font/woff2",
            include_bytes!("../../web/themes/field-notes/newsreader-400-latin.woff2").as_slice(),
        ),
        "field-notes/newsreader-700-latin.woff2" => (
            "font/woff2",
            include_bytes!("../../web/themes/field-notes/newsreader-700-latin.woff2").as_slice(),
        ),
        "field-notes/caveat-700-latin.woff2" => (
            "font/woff2",
            include_bytes!("../../web/themes/field-notes/caveat-700-latin.woff2").as_slice(),
        ),
        "field-notes/ruled.svg" => (
            "image/svg+xml",
            include_bytes!("../../web/themes/field-notes/ruled.svg").as_slice(),
        ),
        "field-notes/OFL-newsreader.txt" => (
            "text/plain; charset=utf-8",
            include_bytes!("../../web/themes/field-notes/OFL-newsreader.txt").as_slice(),
        ),
        "field-notes/OFL-caveat.txt" => (
            "text/plain; charset=utf-8",
            include_bytes!("../../web/themes/field-notes/OFL-caveat.txt").as_slice(),
        ),
        "field-notes/NOTICE.md" => (
            "text/plain; charset=utf-8",
            include_bytes!("../../web/themes/field-notes/NOTICE.md").as_slice(),
        ),
        "castle-keep.css" => (
            "text/css",
            include_bytes!("../../web/themes/castle-keep.css").as_slice(),
        ),
        "castle-keep/medievalsharp-latin.woff2" => (
            "font/woff2",
            include_bytes!("../../web/themes/castle-keep/medievalsharp-latin.woff2").as_slice(),
        ),
        "castle-keep/nunito-400-latin.woff2" => (
            "font/woff2",
            include_bytes!("../../web/themes/castle-keep/nunito-400-latin.woff2").as_slice(),
        ),
        "castle-keep/nunito-700-latin.woff2" => (
            "font/woff2",
            include_bytes!("../../web/themes/castle-keep/nunito-700-latin.woff2").as_slice(),
        ),
        "castle-keep/stone.svg" => (
            "image/svg+xml",
            include_bytes!("../../web/themes/castle-keep/stone.svg").as_slice(),
        ),
        "castle-keep/grass.svg" => (
            "image/svg+xml",
            include_bytes!("../../web/themes/castle-keep/grass.svg").as_slice(),
        ),
        "castle-keep/oak.svg" => (
            "image/svg+xml",
            include_bytes!("../../web/themes/castle-keep/oak.svg").as_slice(),
        ),
        "castle-keep/steel.svg" => (
            "image/svg+xml",
            include_bytes!("../../web/themes/castle-keep/steel.svg").as_slice(),
        ),
        "castle-keep/parchment.svg" => (
            "image/svg+xml",
            include_bytes!("../../web/themes/castle-keep/parchment.svg").as_slice(),
        ),
        "castle-keep/OFL-medievalsharp.txt" => (
            "text/plain; charset=utf-8",
            include_bytes!("../../web/themes/castle-keep/OFL-medievalsharp.txt").as_slice(),
        ),
        "castle-keep/OFL-nunito.txt" => (
            "text/plain; charset=utf-8",
            include_bytes!("../../web/themes/castle-keep/OFL-nunito.txt").as_slice(),
        ),
        "castle-keep/NOTICE.md" => (
            "text/plain; charset=utf-8",
            include_bytes!("../../web/themes/castle-keep/NOTICE.md").as_slice(),
        ),
        _ => return None,
    })
}

fn allowed(path: &str) -> bool {
    if path.is_empty() || path.contains('\\') || path.matches('/').count() > 1 {
        return false;
    }
    let mut parts = path.split('/');
    let first = parts.next().unwrap_or("");
    let second = parts.next();
    if first.is_empty() || first == "." || first == ".." {
        return false;
    }
    let file = match second {
        None => first,
        Some(file) => {
            if file.is_empty() || file == "." || file == ".." || !theme_id(first) {
                return false;
            }
            file
        }
    };
    let extension = file.rsplit_once('.').map(|(_, extension)| extension);
    matches!(
        extension,
        Some("css" | "woff2" | "svg" | "webp" | "png" | "txt" | "md")
    )
}

fn theme_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|char| char.is_ascii_lowercase() || char.is_ascii_digit() || char == '-')
}

pub(super) async fn asset(
    headers: HeaderMap,
    Path(path): Path<String>,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
) -> Response {
    if !allowed(&path) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some((mime, body)) = bytes(&path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut response = conditional::static_asset(&headers, mime, body);
    if mime != "text/css" {
        response.headers_mut().insert(
            header::CONTENT_SECURITY_POLICY,
            "default-src 'none'".parse().unwrap(),
        );
    } else if requested_version(&uri).is_some_and(|version| version == digest(body)) {
        // Same bytes as the ETag. The query is the cache key, matching the
        // immutable vendor asset that already has its version in the URL.
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            "public, max-age=31536000, immutable".parse().unwrap(),
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listed_files_are_embedded_and_traversal_is_rejected() {
        for path in FILES {
            assert!(bytes(path).is_some(), "{path}");
            assert!(allowed(path), "{path}");
        }
        assert!(bytes("cartridge-club/nope.woff2").is_none());
        assert!(!allowed("cartridge-club/../../Cargo.toml"));
        assert!(!allowed("../nightglass.css"));
        assert!(!allowed("cartridge-club/.."));
        assert!(!allowed("cartridge-club/note.html"));
    }

    #[test]
    fn registry_stylesheets_are_exposed_with_stable_fingerprints() {
        let script = std::str::from_utf8(script()).unwrap();
        assert!(script.starts_with("globalThis.InfuserThemeSheets="));
        assert!(script.contains("window.InfuserThemes"));
        let sheets = stylesheets();
        assert!(sheets.iter().any(|sheet| sheet.id == "nightglass"));
        assert!(sheets.iter().any(|sheet| sheet.id == "cartridge-club"));
        assert!(sheets.iter().any(|sheet| sheet.id == "funny-pages"));
        assert!(sheets.iter().any(|sheet| sheet.id == "field-notes"));
        for sheet in sheets {
            assert!(
                script.contains(&format!(
                    "/assets/themes/{}?v={}",
                    sheet.path, sheet.version
                )),
                "{}",
                sheet.path
            );
            let (_, body) = bytes(sheet.path).unwrap();
            assert_eq!(sheet.version, digest(body));
        }
        let html = fingerprint_html(
            r#"<link rel="stylesheet" href="/assets/themes/nightglass.css"><link rel="stylesheet" href="/assets/themes/cartridge-club.css">"#,
        );
        assert_eq!(html.matches("?v=").count(), 2);
        assert!(!html.contains("href=\"/assets/themes/nightglass.css\""));
        assert_eq!(html, fingerprint_html(&html));
    }

    #[tokio::test]
    async fn fingerprinted_stylesheet_is_immutable_and_plain_urls_revalidate() {
        let (_, body) = bytes("nightglass.css").unwrap();
        let version = digest(body);
        let plain = request("nightglass.css", "").await;
        assert_eq!(plain.headers()[header::CONTENT_TYPE], "text/css");
        assert_eq!(plain.headers()[header::CACHE_CONTROL], "no-cache");
        assert!(
            plain.headers()[header::ETAG]
                .to_str()
                .unwrap()
                .contains(&version)
        );
        let hit = request("nightglass.css", &format!("?v={version}")).await;
        assert_eq!(
            hit.headers()[header::CACHE_CONTROL],
            "public, max-age=31536000, immutable"
        );
        let miss = request("nightglass.css", "?v=not-the-hash").await;
        assert_eq!(miss.headers()[header::CACHE_CONTROL], "no-cache");
        let (_, font_body) = bytes("cartridge-club/lilita-one-latin.woff2").unwrap();
        let font = request(
            "cartridge-club/lilita-one-latin.woff2",
            &format!("?v={}", digest(font_body)),
        )
        .await;
        assert_eq!(font.headers()[header::CACHE_CONTROL], "no-cache");
        assert_eq!(
            font.headers()[header::CONTENT_SECURITY_POLICY],
            "default-src 'none'"
        );
    }

    #[test]
    fn appearance_validation_accepts_known_themes_and_hex_colors() {
        let valid = Appearance {
            theme: "nightglass".into(),
            schemes: [("nightglass".into(), "twilight-violet".into())].into(),
            custom: [(
                "nightglass".into(),
                CustomColors {
                    primary: " #1A2B3C ".into(),
                    accent: "#Ff88Cc".into(),
                    secondary: "#e8d5a4".into(),
                },
            )]
            .into(),
        }
        .validate()
        .unwrap();
        assert_eq!(valid.theme, "nightglass");
        assert_eq!(valid.schemes["nightglass"], "twilight-violet");
        assert_eq!(valid.custom["nightglass"].primary, "#1a2b3c");
        assert_eq!(valid.custom["nightglass"].accent, "#ff88cc");

        let default_dropped = Appearance {
            theme: "nightglass".into(),
            schemes: [("nightglass".into(), "default".into())].into(),
            custom: BTreeMap::new(),
        }
        .validate()
        .unwrap();
        assert!(default_dropped.schemes.is_empty());

        for bad in [
            Appearance {
                theme: "unknown-theme".into(),
                schemes: BTreeMap::new(),
                custom: BTreeMap::new(),
            },
            Appearance {
                theme: "nightglass".into(),
                schemes: [("nightglass".into(), "not-a-scheme".into())].into(),
                custom: BTreeMap::new(),
            },
            Appearance {
                theme: "field-notes".into(),
                schemes: [("field-notes".into(), "deep-ocean".into())].into(),
                custom: BTreeMap::new(),
            },
            Appearance {
                theme: "nightglass".into(),
                schemes: BTreeMap::new(),
                custom: [(
                    "nightglass".into(),
                    CustomColors {
                        primary: "#fff".into(),
                        accent: "#b5ead6".into(),
                        secondary: "#e8d5a4".into(),
                    },
                )]
                .into(),
            },
            Appearance {
                theme: "nightglass".into(),
                schemes: BTreeMap::new(),
                custom: [(
                    "field-notes".into(),
                    CustomColors {
                        primary: "#122128".into(),
                        accent: "#b5ead6".into(),
                        secondary: "#e8d5a4".into(),
                    },
                )]
                .into(),
            },
        ] {
            assert!(bad.validate().is_err());
        }
    }

    async fn request(path: &str, query: &str) -> Response {
        let uri: axum::http::Uri = format!("/assets/themes/{path}{query}").parse().unwrap();
        asset(
            HeaderMap::new(),
            Path(path.to_string()),
            axum::extract::OriginalUri(uri),
        )
        .await
    }
}
