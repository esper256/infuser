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
    serde_json::json!({"path":PATH,"width":1024,"height":1024,"max_bytes":MAX_BYTES,"format":"webp","derivatives":DERIVATIVE_WIDTHS})
}

/// Display sizes the Library asks for. The stored file stays 1024.
pub const DERIVATIVE_WIDTHS: [u32; 3] = [384, 640, 768];

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A smaller lossy WebP for one card slot. The stored master is never rewritten.
pub fn derivative(master: &[u8], width: u32) -> Result<Vec<u8>> {
    ensure!(
        DERIVATIVE_WIDTHS.contains(&width),
        "unsupported card art size"
    );
    let mut image = WebPDecoder::new(Cursor::new(master)).context("card hero must be WebP")?;
    let (source_width, source_height) = image.dimensions();
    ensure!(
        (source_width, source_height) == (1024, 1024),
        "card hero must be 1024 × 1024 pixels"
    );
    image.set_memory_limit(16 * 1024 * 1024);
    let mut pixels = vec![0; 1024 * 1024 * 3];
    image
        .read_image(&mut pixels)
        .context("card hero is damaged or incomplete")?;
    let source = image::RgbImage::from_raw(1024, 1024, pixels).context("card hero pixels")?;
    let scaled =
        image::imageops::resize(&source, width, width, image::imageops::FilterType::Lanczos3);
    // Quality matches a cover that is already a compressed 1024 master, not a new original.
    let encoded = webp::Encoder::from_rgb(scaled.as_raw(), width, width).encode(75.0);
    Ok(encoded.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derivatives_are_smaller_square_webp() {
        let master =
            include_bytes!("../docs/design-explorations/card-art-guide/firefly-drift.webp");
        validate(master).unwrap();
        for width in DERIVATIVE_WIDTHS {
            let bytes = derivative(master, width).unwrap();
            assert!(
                bytes.len() < master.len() / 2,
                "{width}px derivative should be much smaller than the master"
            );
            let decoded = WebPDecoder::new(Cursor::new(bytes)).unwrap();
            assert_eq!(decoded.dimensions(), (width, width));
            assert!(!decoded.has_alpha());
        }
    }
}
