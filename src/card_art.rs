//! Optional build artifact, independent of the game SDK and save contracts.
use anyhow::{Context, Result, ensure};
use image_webp::WebPDecoder;
use std::io::Cursor;

pub const PATH: &str = "infuser-art/card-hero.webp";
pub const MAX_BYTES: usize = 250 * 1024;

pub fn validate(bytes: &[u8]) -> Result<()> {
    ensure!(bytes.len() <= MAX_BYTES, "card hero exceeds 250 KiB");
    let mut image = WebPDecoder::new(Cursor::new(bytes)).context("card hero must be WebP")?;
    ensure!(
        image.dimensions() == (1024, 1024),
        "card hero must be 1024 × 1024 pixels"
    );
    ensure!(
        !image.is_animated() && !image.has_alpha(),
        "card hero must be static, opaque RGB WebP"
    );
    ensure!(
        image.icc_profile()?.is_none(),
        "convert card hero to sRGB and strip its color profile before upload"
    );
    image.set_memory_limit(16 * 1024 * 1024);
    image
        .read_image(&mut vec![0; 1024 * 1024 * 3])
        .context("card hero is damaged or incomplete")?;
    Ok(())
}

pub fn capabilities() -> serde_json::Value {
    serde_json::json!({"path":PATH,"width":1024,"height":1024,"max_bytes":MAX_BYTES,"format":"webp"})
}
