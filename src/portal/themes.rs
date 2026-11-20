//! Allowlisted Library theme files. Paths are exact; nothing is read from disk.
use super::*;
use sha2::{Digest, Sha256};

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

pub(super) async fn asset(headers: HeaderMap, Path(path): Path<String>) -> Response {
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
}
