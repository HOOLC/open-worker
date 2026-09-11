//! One read-only preview pipeline for composer papers, message thumbnails and
//! the attachment viewer. Classification never depends on the opening surface.
use super::image::{self as images, DecodedImage};
use std::sync::Arc;

const TEXT_LIMIT: usize = 256 * 1024;
const SNAPSHOT_LIMIT: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::views) enum Kind {
    Image,
    Markdown,
    Text,
    Code(&'static str),
    Document,
    Video,
    Audio,
    Archive,
    Binary,
}
impl Kind {
    pub fn is_image(self) -> bool {
        self == Self::Image
    }
    pub fn is_text(self) -> bool {
        matches!(self, Self::Markdown | Self::Text | Self::Code(_))
    }
    pub fn has_thumbnail(self) -> bool {
        self.is_image() || self.is_text() || matches!(self, Self::Document | Self::Video)
    }
}

pub(in crate::views) fn kind(name: &str, mime: &str) -> Kind {
    if image_format(name, mime).is_some() || mime.starts_with("image/") {
        return Kind::Image;
    }
    let extension = std::path::Path::new(name)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "heic" | "heif" | "avif" => Kind::Image,
        "md" | "markdown" | "mdown" => Kind::Markdown,
        "rs" => Kind::Code("rust"),
        "py" | "pyi" => Kind::Code("python"),
        "js" | "jsx" | "mjs" | "cjs" => Kind::Code("javascript"),
        "ts" | "tsx" => Kind::Code("typescript"),
        "json" | "jsonl" => Kind::Code("json"),
        "html" | "htm" => Kind::Code("html"),
        "xml" => Kind::Code("xml"),
        "css" | "scss" => Kind::Code("css"),
        "sh" | "bash" | "zsh" => Kind::Code("bash"),
        "yaml" | "yml" => Kind::Code("yaml"),
        "toml" => Kind::Code("toml"),
        "go" => Kind::Code("go"),
        "java" => Kind::Code("java"),
        "c" | "h" => Kind::Code("c"),
        "cpp" | "hpp" | "cc" => Kind::Code("cpp"),
        "swift" => Kind::Code("swift"),
        "sql" => Kind::Code("sql"),
        "txt" | "text" | "log" | "csv" | "tsv" | "ini" | "conf" | "env" => Kind::Text,
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "ods" | "odp"
        | "rtf" | "pages" | "numbers" | "key" => Kind::Document,
        "mp4" | "mov" | "m4v" | "webm" | "mkv" | "avi" => Kind::Video,
        "mp3" | "wav" | "m4a" | "aac" | "flac" | "ogg" => Kind::Audio,
        "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" | "dmg" => Kind::Archive,
        _ => match mime.split(';').next().unwrap_or("").trim() {
            "text/markdown" => Kind::Markdown,
            "application/json" => Kind::Code("json"),
            "application/xml" => Kind::Code("xml"),
            "application/pdf" | "application/rtf" => Kind::Document,
            m if m.starts_with("text/") => Kind::Text,
            m if m.starts_with("video/") => Kind::Video,
            m if m.starts_with("audio/") => Kind::Audio,
            _ => Kind::Binary,
        },
    }
}

pub(in crate::views) fn image_format(name: &str, mime: &str) -> Option<gpui::ImageFormat> {
    let mime = mime
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    gpui::ImageFormat::from_mime_type(&mime).or_else(|| images::format(name))
}

pub(in crate::views) struct Content {
    pub kind: Kind,
    pub image: Option<DecodedImage>,
    pub text: Option<Arc<str>>,
    pub truncated: bool,
    pub overview: bool,
}

fn text(bytes: &[u8]) -> Option<(Arc<str>, bool)> {
    let decoded = if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        let little = bytes[0] == 0xff;
        let end = bytes.len().min(TEXT_LIMIT + 2);
        if end == bytes.len() && (end - 2) % 2 != 0 {
            return None;
        }
        let units = bytes[2..end]
            .chunks_exact(2)
            .map(|v| {
                if little {
                    u16::from_le_bytes([v[0], v[1]])
                } else {
                    u16::from_be_bytes([v[0], v[1]])
                }
            })
            .collect::<Vec<_>>();
        let units =
            if end < bytes.len() && units.last().is_some_and(|v| (0xd800..=0xdbff).contains(v)) {
                &units[..units.len() - 1]
            } else {
                &units
            };
        (String::from_utf16(units).ok()?, end < bytes.len())
    } else {
        let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
        let mut end = bytes.len().min(TEXT_LIMIT);
        let value = loop {
            match std::str::from_utf8(&bytes[..end]) {
                Ok(value) => break value,
                Err(error) if end < bytes.len() && error.error_len().is_none() => {
                    end = error.valid_up_to()
                }
                Err(_) => return None,
            }
        };
        (value.to_owned(), end < bytes.len())
    };
    if decoded
        .0
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t' | '\u{c}'))
    {
        return None;
    }
    Some((Arc::from(decoded.0), decoded.1))
}

pub(in crate::views) fn decode(
    name: &str,
    mime: &str,
    bytes: &[u8],
    renderer: &gpui::SvgRenderer,
    side: u32,
) -> anyhow::Result<Content> {
    anyhow::ensure!(bytes.len() <= SNAPSHOT_LIMIT, "preview exceeds size limit");
    let mut kind = kind(name, mime);
    // Do not show an ASCII-only PDF/ZIP as source text when metadata is absent.
    if bytes.starts_with(b"%PDF-") {
        kind = Kind::Document;
    } else if bytes.starts_with(b"PK\x03\x04") && kind != Kind::Document {
        kind = Kind::Archive;
    }
    let mut result = Content {
        kind,
        image: None,
        text: None,
        truncated: false,
        overview: false,
    };
    if kind.is_image() {
        let format = image_format(name, mime);
        result.image = Some(match format {
            Some(format) => images::decode(bytes, format, renderer).or_else(|error| {
                super::system_preview::system_thumbnail(name, bytes, side)
                    .map(images::from_raster)
                    .ok_or(error)
            })?,
            None => super::system_preview::system_thumbnail(name, bytes, side)
                .map(images::from_raster)
                .ok_or_else(|| anyhow::anyhow!("unsupported image format"))?,
        });
        if format == Some(gpui::ImageFormat::Svg) {
            if let Some((text, truncated)) = text(bytes) {
                result.text = Some(text);
                result.truncated = truncated;
            }
        }
        return Ok(result);
    }
    if kind.is_text() || kind == Kind::Binary {
        if let Some((text, truncated)) = text(bytes) {
            result.text = Some(text);
            result.truncated = truncated;
            if kind == Kind::Binary {
                result.kind = Kind::Text;
            }
            return Ok(result);
        }
        anyhow::ensure!(!kind.is_text(), "unsupported text encoding or invalid text");
    }
    if matches!(kind, Kind::Document | Kind::Video) {
        let name = if bytes.starts_with(b"%PDF-") {
            "preview.pdf"
        } else {
            name
        };
        if let Some(image) = super::system_preview::system_thumbnail(name, bytes, side) {
            result.image = Some(images::from_raster(image));
            result.overview = true;
        }
    }
    Ok(result)
}

pub(super) fn thumbnail_source(
    content: &Content,
    renderer: &gpui::SvgRenderer,
) -> Option<image::DynamicImage> {
    if let Some(image) = &content.image {
        let size = image.rendered.size(0);
        let mut pixels = image::RgbaImage::from_raw(
            i32::from(size.width) as u32,
            i32::from(size.height) as u32,
            image.rendered.as_bytes(0)?.to_vec(),
        )?;
        for pixel in pixels.pixels_mut() {
            pixel.0.swap(0, 2);
        }
        return Some(image::DynamicImage::ImageRgba8(pixels));
    }
    let text = content.text.as_ref()?;
    // A bounded excerpt uses the same decoded text on every platform. It does
    // not depend on an installed Quick Look text plugin or its appearance.
    let mut svg = String::from(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="432" height="608"><rect width="432" height="608" fill="white"/><g fill="#30343b" font-family="monospace" font-size="16">"##,
    );
    for (row, line) in text.lines().take(24).enumerate() {
        let line = line
            .chars()
            .take(80)
            .collect::<String>()
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        svg.push_str(&format!(
            r#"<text x="24" y="{}" xml:space="preserve">{line}</text>"#,
            36 + row * 23
        ));
    }
    svg.push_str("</g></svg>");
    let rendered = renderer.render_single_frame(svg.as_bytes(), 0.5).ok()?;
    let mut pixels = image::RgbaImage::from_raw(432, 608, rendered.as_bytes(0)?.to_vec())?;
    for pixel in pixels.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Some(image::DynamicImage::ImageRgba8(pixels))
}

pub(super) fn thumbnail(
    name: &str,
    bytes: &[u8],
    renderer: &gpui::SvgRenderer,
) -> anyhow::Result<Arc<gpui::RenderImage>> {
    let content = decode(name, "", bytes, renderer, 384)?;
    let source = thumbnail_source(&content, renderer)
        .ok_or_else(|| anyhow::anyhow!("no thumbnail available"))?;
    Ok(images::render(images::pad_thumbnail(
        source.thumbnail(600, 400).to_rgba8(),
    )))
}
