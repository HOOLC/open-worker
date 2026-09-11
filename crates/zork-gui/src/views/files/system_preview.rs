//! Bounded, read-only system preview generation from the attachment snapshot.
fn decode_raster(bytes: &[u8]) -> Option<image::DynamicImage> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    reader.decode().ok()
}

pub(super) fn system_thumbnail(name: &str, bytes: &[u8], side: u32) -> Option<image::DynamicImage> {
    let png = zork_client_core::desktop::preview::thumbnail_bytes(name, bytes, side)?;
    let extension = std::path::Path::new(name).extension()?.to_str()?;
    let mut image = decode_raster(&png)?;
    // Quick Look's plain-text generator can produce black ink on its dark
    // appearance background. Normalize only this monochrome text rendering;
    // never change a supplied image or a PDF's authored page colors.
    if matches!(
        extension.to_ascii_lowercase().as_str(),
        "txt" | "md" | "markdown" | "rs" | "py" | "js" | "ts" | "json" | "csv" | "log"
    ) {
        let mut rgba = image.to_rgba8();
        let background = rgba[(rgba.width() - 1, rgba.height() - 1)][0];
        if (16..=64).contains(&background)
            && rgba
                .pixels()
                .all(|p| p[0] == p[1] && p[1] == p[2] && p[0] <= background)
        {
            for pixel in rgba.pixels_mut() {
                let value = (pixel[0] as u16 * 255 / background as u16) as u8;
                pixel[0] = value;
                pixel[1] = value;
                pixel[2] = value;
            }
            image = image::DynamicImage::ImageRgba8(rgba);
        }
    }
    Some(image)
}
