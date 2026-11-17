//! Local illustrated game reactions; persisted values remain Unicode emoji IDs.
use super::*;

pub(super) async fn image(Path(file): Path<String>) -> Answer {
    let bytes: &[u8] = match file.as_str() {
        "atmosphere.webp" => include_bytes!("../../assets/reactions/atmosphere.webp"),
        "cozy.webp" => include_bytes!("../../assets/reactions/cozy.webp"),
        "gem.webp" => include_bytes!("../../assets/reactions/gem.webp"),
        "clever.webp" => include_bytes!("../../assets/reactions/clever.webp"),
        "wild.webp" => include_bytes!("../../assets/reactions/wild.webp"),
        "surprises.webp" => include_bytes!("../../assets/reactions/surprises.webp"),
        _ => return Err(fault(404, "Unknown reaction artwork.").into()),
    };
    Ok((
        [
            (header::CONTENT_TYPE, "image/webp"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        bytes,
    )
        .into_response())
}
