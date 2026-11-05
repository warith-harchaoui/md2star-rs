//! Resources a document points at: images (and any other referenced file).
//!
//! Ownership: shared foundation, used by every backend. It answers three questions
//! about an image reference, all in pure Rust and in memory:
//!
//! 1. **Where are the bytes?** A path relative to the document ([`Resolver::base_dir`]),
//!    an absolute path, a `file://` URL, a `data:` URI (base64 or percent-encoded), or —
//!    only with the `net` feature *and* [`Resolver::allow_remote`] — an `http(s)://` URL.
//!    Without the feature a remote image is a clean [`Error::Resource`], never a hang.
//! 2. **What is it?** The format, read from the bytes' signature, never from the file
//!    name, and its MIME type.
//! 3. **How big is it?** Width and height in pixels, read from the header (`imagesize`
//!    for rasters, the root element's `width`/`height`/`viewBox` for SVG), so a writer
//!    can scale it to the page without decoding a single pixel.

use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine as _;

use crate::ast::Attr;
use crate::error::{Error, Result};
use crate::options::Options;

/// English Metric Units per inch, the unit of every OOXML length.
pub const EMU_PER_INCH: u64 = 914_400;
/// EMU per CSS pixel (96 per inch).
pub const EMU_PER_PX: u64 = EMU_PER_INCH / 96;

/// An image format md2star can identify.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    /// PNG.
    Png,
    /// JPEG.
    Jpeg,
    /// GIF.
    Gif,
    /// BMP.
    Bmp,
    /// TIFF.
    Tiff,
    /// WebP.
    Webp,
    /// SVG (vector; dimensions come from its root element).
    Svg,
    /// Windows icon.
    Ico,
}

impl ImageFormat {
    /// The IANA media type.
    pub fn mime(self) -> &'static str {
        match self {
            ImageFormat::Png => "image/png",
            ImageFormat::Jpeg => "image/jpeg",
            ImageFormat::Gif => "image/gif",
            ImageFormat::Bmp => "image/bmp",
            ImageFormat::Tiff => "image/tiff",
            ImageFormat::Webp => "image/webp",
            ImageFormat::Svg => "image/svg+xml",
            ImageFormat::Ico => "image/x-icon",
        }
    }

    /// The conventional file extension, without the dot (what an OOXML media part is
    /// named with).
    pub fn extension(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpeg",
            ImageFormat::Gif => "gif",
            ImageFormat::Bmp => "bmp",
            ImageFormat::Tiff => "tiff",
            ImageFormat::Webp => "webp",
            ImageFormat::Svg => "svg",
            ImageFormat::Ico => "ico",
        }
    }
}

/// What the bytes of an image say about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageInfo {
    /// The format, from the signature.
    pub format: ImageFormat,
    /// Width in pixels (CSS pixels for SVG).
    pub width_px: u32,
    /// Height in pixels (CSS pixels for SVG).
    pub height_px: u32,
}

impl ImageInfo {
    /// Width over height (1.0 for a degenerate zero-height image).
    pub fn aspect_ratio(&self) -> f64 {
        if self.height_px == 0 {
            1.0
        } else {
            f64::from(self.width_px) / f64::from(self.height_px)
        }
    }
}

/// Where a resource's bytes came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// A file on disk (the resolved path).
    File(PathBuf),
    /// A `data:` URI embedded in the document.
    DataUri,
    /// A remote URL (feature `net`).
    Remote(String),
}

/// A loaded image: bytes, identity and size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedImage {
    /// The raw file bytes, untouched.
    pub bytes: Vec<u8>,
    /// Format and pixel size.
    pub info: ImageInfo,
    /// Where the bytes came from.
    pub origin: Origin,
}

/// Resolves references written in a document to bytes.
#[derive(Debug, Clone, Default)]
pub struct Resolver {
    /// The directory relative paths resolve against (`None`: the current directory).
    pub base_dir: Option<PathBuf>,
    /// Whether `http(s)://` references may be fetched (needs the `net` feature).
    pub allow_remote: bool,
}

/// Remote downloads are capped so a hostile or mistaken URL cannot exhaust memory.
#[cfg(feature = "net")]
const MAX_REMOTE_BYTES: u64 = 64 * 1024 * 1024;

impl Resolver {
    /// A resolver for documents living in `base_dir`, local resources only.
    pub fn new(base_dir: Option<PathBuf>) -> Self {
        Resolver {
            base_dir,
            allow_remote: false,
        }
    }

    /// The resolver a conversion with `options` should use.
    pub fn from_options(options: &Options) -> Self {
        Resolver {
            base_dir: options.base_dir.clone(),
            allow_remote: options.remote_images_allowed(),
        }
    }

    /// Resolve a local reference to a path: absolute paths and `file://` URLs as they
    /// are, relative ones against [`Resolver::base_dir`].
    pub fn resolve_path(&self, src: &str) -> PathBuf {
        let src = src.strip_prefix("file://").unwrap_or(src);
        let path = Path::new(src);
        match &self.base_dir {
            Some(base) if path.is_relative() => base.join(path),
            _ => path.to_path_buf(),
        }
    }

    /// Read the bytes `src` refers to, whatever kind of reference it is.
    ///
    /// # Errors
    ///
    /// [`Error::Resource`] when the file is missing, the `data:` URI is malformed, or the
    /// reference is remote and remote loading is not allowed (or not compiled in).
    pub fn read(&self, src: &str) -> Result<(Vec<u8>, Origin)> {
        if src.starts_with("data:") {
            return decode_data_uri(src).map(|(_, bytes)| (bytes, Origin::DataUri));
        }
        if is_remote(src) {
            return self
                .fetch(src)
                .map(|b| (b, Origin::Remote(src.to_string())));
        }
        let path = self.resolve_path(src);
        match fs::read(&path) {
            Ok(bytes) => Ok((bytes, Origin::File(path))),
            Err(first) => {
                // Markdown writers often percent-encode spaces: `my%20chart.png`.
                let decoded = percent_decode(src);
                if decoded != src.as_bytes() {
                    if let Ok(decoded) = String::from_utf8(decoded) {
                        let alt = self.resolve_path(&decoded);
                        if let Ok(bytes) = fs::read(&alt) {
                            return Ok((bytes, Origin::File(alt)));
                        }
                    }
                }
                Err(resource_error(src, format!("{} ({first})", path.display())))
            }
        }
    }

    /// Load an image: its bytes, format, pixel size and origin.
    ///
    /// # Errors
    ///
    /// Everything [`Resolver::read`] can return, plus [`Error::Resource`] when the bytes
    /// are not an image format [`probe`] recognises.
    pub fn load_image(&self, src: &str) -> Result<LoadedImage> {
        let (bytes, origin) = self.read(src)?;
        let info = probe(&bytes).map_err(|e| match e {
            Error::Resource { reason, .. } => resource_error(src, reason),
            other => other,
        })?;
        Ok(LoadedImage {
            bytes,
            info,
            origin,
        })
    }

    /// Fetch a remote URL (feature `net`): bounded in time and in size.
    #[cfg(feature = "net")]
    fn fetch(&self, url: &str) -> Result<Vec<u8>> {
        use std::io::Read as _;
        use std::time::Duration;

        if !self.allow_remote {
            return Err(resource_error(
                url,
                "remote images are off: pass --allow-remote-images (and not --offline)",
            ));
        }
        // Every read is bounded: an unbounded socket read would hang forever and disable
        // every retry path above it.
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(Duration::from_secs(60)))
            .build()
            .into();
        let response = agent
            .get(url)
            .call()
            .map_err(|e| resource_error(url, e.to_string()))?;
        let mut bytes = Vec::new();
        response
            .into_body()
            .into_reader()
            .take(MAX_REMOTE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| resource_error(url, e.to_string()))?;
        if bytes.len() as u64 > MAX_REMOTE_BYTES {
            return Err(resource_error(url, "larger than the 64 MiB cap"));
        }
        Ok(bytes)
    }

    /// Without the `net` feature, a remote reference is refused up front.
    #[cfg(not(feature = "net"))]
    fn fetch(&self, url: &str) -> Result<Vec<u8>> {
        Err(resource_error(
            url,
            "remote images need md2star-rs built with the `net` feature",
        ))
    }
}

fn resource_error(src: &str, reason: impl Into<String>) -> Error {
    Error::Resource {
        src: src.to_string(),
        reason: reason.into(),
    }
}

/// Is `src` an `http://` or `https://` URL?
pub fn is_remote(src: &str) -> bool {
    let lower = src.get(..8).unwrap_or(src).to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// Identify an image from its bytes: format and pixel size.
///
/// # Errors
///
/// [`Error::Resource`] (with an empty `src`; [`Resolver::load_image`] fills it in) when
/// the bytes are not a recognised image.
///
/// # Examples
///
/// ```
/// // A 1×1 PNG header is enough: nothing is decoded.
/// let png = [
///     0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, b'I', b'H', b'D', b'R',
///     0, 0, 0, 3, 0, 0, 0, 2, 8, 6, 0, 0, 0,
/// ];
/// let info = md2star_rs::resources::probe(&png).unwrap();
/// assert_eq!((info.width_px, info.height_px), (3, 2));
/// assert_eq!(info.format.mime(), "image/png");
/// ```
pub fn probe(bytes: &[u8]) -> Result<ImageInfo> {
    if let Some(info) = probe_svg(bytes) {
        return Ok(info);
    }
    let format = match imagesize::image_type(bytes) {
        Ok(imagesize::ImageType::Png) => ImageFormat::Png,
        Ok(imagesize::ImageType::Jpeg) => ImageFormat::Jpeg,
        Ok(imagesize::ImageType::Gif) => ImageFormat::Gif,
        Ok(imagesize::ImageType::Bmp) => ImageFormat::Bmp,
        Ok(imagesize::ImageType::Tiff) => ImageFormat::Tiff,
        Ok(imagesize::ImageType::Webp) => ImageFormat::Webp,
        Ok(imagesize::ImageType::Ico) => ImageFormat::Ico,
        Ok(other) => {
            return Err(resource_error(
                "",
                format!("unsupported image format {other:?}"),
            ))
        }
        Err(_) => return Err(resource_error("", "not a recognised image")),
    };
    let size = imagesize::blob_size(bytes)
        .map_err(|e| resource_error("", format!("unreadable image header: {e}")))?;
    Ok(ImageInfo {
        format,
        width_px: u32::try_from(size.width).unwrap_or(u32::MAX),
        height_px: u32::try_from(size.height).unwrap_or(u32::MAX),
    })
}

/// Read an SVG's size from its root element: `width`/`height` (px, pt, pc, mm, cm, in,
/// unitless), else the `viewBox`, else the browser default 300×150.
fn probe_svg(bytes: &[u8]) -> Option<ImageInfo> {
    let head = std::str::from_utf8(&bytes[..bytes.len().min(4096)])
        .or_else(|e| std::str::from_utf8(&bytes[..e.valid_up_to()]))
        .ok()?;
    let start = head.find("<svg")?;
    // Only a document whose first element is <svg> (after a prolog / comments).
    let before = &head[..start];
    if before.contains('<') {
        let prolog_only = before
            .split('<')
            .skip(1)
            .all(|t| t.starts_with('?') || t.starts_with('!'));
        if !prolog_only {
            return None;
        }
    }
    let tag_end = head[start..].find('>')? + start;
    let tag = &head[start..tag_end];
    let width = xml_attr(tag, "width").and_then(css_length);
    let height = xml_attr(tag, "height").and_then(css_length);
    let viewbox: Option<(f64, f64)> = xml_attr(tag, "viewBox").and_then(|v| {
        let nums: Vec<f64> = v
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse().ok())
            .collect();
        (nums.len() == 4).then(|| (nums[2], nums[3]))
    });
    let (w, h) = match (width, height, viewbox) {
        (Some(w), Some(h), _) => (w, h),
        (Some(w), None, Some((vw, vh))) if vw > 0.0 => (w, w * vh / vw),
        (None, Some(h), Some((vw, vh))) if vh > 0.0 => (h * vw / vh, h),
        (_, _, Some((vw, vh))) => (vw, vh),
        _ => (300.0, 150.0),
    };
    Some(ImageInfo {
        format: ImageFormat::Svg,
        width_px: w.round().max(0.0) as u32,
        height_px: h.round().max(0.0) as u32,
    })
}

/// The value of attribute `name` in a start tag (`name="v"` or `name='v'`).
fn xml_attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = tag;
    while let Some(at) = rest.find(name) {
        let preceded = rest[..at]
            .chars()
            .next_back()
            .is_some_and(char::is_whitespace);
        let after = rest[at + name.len()..].trim_start();
        if preceded {
            if let Some(value) = after.strip_prefix('=') {
                let value = value.trim_start();
                let quote = value.chars().next()?;
                if quote == '"' || quote == '\'' {
                    let end = value[1..].find(quote)?;
                    return Some(&value[1..1 + end]);
                }
            }
        }
        rest = &rest[at + name.len()..];
    }
    None
}

/// A CSS length in pixels (96 px per inch). Percentages have no absolute size: `None`.
fn css_length(v: &str) -> Option<f64> {
    let v = v.trim();
    let split = v
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e'))
        .unwrap_or(v.len());
    let (num, unit) = v.split_at(split);
    let n: f64 = num.parse().ok()?;
    let px = match unit.trim() {
        "" | "px" => n,
        "pt" => n * 96.0 / 72.0,
        "pc" => n * 16.0,
        "in" => n * 96.0,
        "cm" => n * 96.0 / 2.54,
        "mm" => n * 96.0 / 25.4,
        _ => return None,
    };
    Some(px)
}

/// The size, in EMU, at which to show an image inside a `max_width_emu` ×
/// `max_height_emu` box.
///
/// Start from the image's own size (its pixels at 96 dpi); apply the author's
/// `width=` / `height=` attributes (`50%` of the box, or a length: `3in`, `4cm`, `20mm`,
/// `300px`, `72pt`, a bare number of pixels), deriving the missing side from the aspect
/// ratio; then scale down, never up, until it fits the box. A zero box side means "no
/// limit" on that side.
///
/// # Examples
///
/// ```
/// use md2star_rs::ast::Attr;
/// use md2star_rs::resources::{fit_emu, ImageFormat, ImageInfo, EMU_PER_INCH};
///
/// let photo = ImageInfo { format: ImageFormat::Jpeg, width_px: 1920, height_px: 1080 };
/// // 20 in wide at 96 dpi: scaled down to a 6.5 in column, aspect kept.
/// let (w, h) = fit_emu(&photo, &Attr::default(), 6 * EMU_PER_INCH + EMU_PER_INCH / 2, 0);
/// assert_eq!(w, 5_943_600);
/// assert_eq!(h, 3_343_275);
/// // `{width=50%}` asks for half the column.
/// let half = Attr { kv: vec![("width".into(), "50%".into())], ..Attr::default() };
/// assert_eq!(fit_emu(&photo, &half, 6 * EMU_PER_INCH, 0).0, 3 * EMU_PER_INCH);
/// ```
pub fn fit_emu(
    info: &ImageInfo,
    attr: &Attr,
    max_width_emu: u64,
    max_height_emu: u64,
) -> (u64, u64) {
    let aspect = info.aspect_ratio();
    let length = |key: &str, of: u64| -> Option<f64> {
        let v = attr.get(key)?.trim();
        match v.strip_suffix('%') {
            Some(pct) if of > 0 => pct
                .trim()
                .parse::<f64>()
                .ok()
                .map(|p| of as f64 * p / 100.0),
            Some(_) => None,
            None => css_length(v).map(|px| px * EMU_PER_PX as f64),
        }
    };
    let natural_w = f64::from(info.width_px) * EMU_PER_PX as f64;
    let natural_h = f64::from(info.height_px) * EMU_PER_PX as f64;
    let (mut w, mut h) = match (
        length("width", max_width_emu),
        length("height", max_height_emu),
    ) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (w, w / aspect),
        (None, Some(h)) => (h * aspect, h),
        (None, None) => (natural_w, natural_h),
    };
    if max_width_emu > 0 && w > max_width_emu as f64 {
        h *= max_width_emu as f64 / w;
        w = max_width_emu as f64;
    }
    if max_height_emu > 0 && h > max_height_emu as f64 {
        w *= max_height_emu as f64 / h;
        h = max_height_emu as f64;
    }
    (w.round().max(1.0) as u64, h.round().max(1.0) as u64)
}

/// Decode a `data:` URI into its media type and bytes.
///
/// # Errors
///
/// [`Error::Resource`] when the URI has no comma or its base64 payload is invalid.
///
/// # Examples
///
/// ```
/// let (mime, bytes) = md2star_rs::resources::decode_data_uri("data:text/plain;base64,aGk=").unwrap();
/// assert_eq!((mime.as_str(), bytes.as_slice()), ("text/plain", b"hi".as_slice()));
/// let (_, svg) = md2star_rs::resources::decode_data_uri("data:image/svg+xml,%3Csvg%2F%3E").unwrap();
/// assert_eq!(svg, b"<svg/>");
/// ```
pub fn decode_data_uri(uri: &str) -> Result<(String, Vec<u8>)> {
    let short = || uri.chars().take(40).collect::<String>();
    let body = uri
        .strip_prefix("data:")
        .ok_or_else(|| resource_error(&short(), "not a data: URI"))?;
    let comma = body
        .find(',')
        .ok_or_else(|| resource_error(&short(), "data: URI without a comma"))?;
    let (header, payload) = (&body[..comma], &body[comma + 1..]);
    let base64 = header.ends_with(";base64");
    let mime = header.split(';').next().unwrap_or("").trim();
    let mime = if mime.is_empty() {
        "text/plain".to_string()
    } else {
        mime.to_string()
    };
    let bytes = if base64 {
        let cleaned: String = payload.chars().filter(|c| !c.is_whitespace()).collect();
        base64::engine::general_purpose::STANDARD
            .decode(cleaned.as_bytes())
            .map_err(|e| resource_error(&short(), format!("invalid base64: {e}")))?
    } else {
        percent_decode(payload)
    };
    Ok((mime, bytes))
}

/// Decode `%XX` escapes; anything that is not a valid escape is kept as is.
fn percent_decode(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(b) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/images")
            .join(name)
    }

    #[test]
    fn png_fixture_is_identified_and_measured() {
        let resolver = Resolver::new(Some(fixture("")));
        let image = resolver.load_image("pixel-4x3.png").unwrap();
        assert_eq!(image.info.format, ImageFormat::Png);
        assert_eq!((image.info.width_px, image.info.height_px), (4, 3));
        assert!(matches!(image.origin, Origin::File(_)));
    }

    #[test]
    fn jpeg_fixture_is_identified_and_measured() {
        let resolver = Resolver::new(Some(fixture("")));
        let image = resolver.load_image("photo-8x6.jpg").unwrap();
        assert_eq!(image.info.format, ImageFormat::Jpeg);
        assert_eq!(image.info.format.mime(), "image/jpeg");
        assert_eq!((image.info.width_px, image.info.height_px), (8, 6));
    }

    #[test]
    fn svg_fixture_size_comes_from_its_root() {
        let resolver = Resolver::new(Some(fixture("")));
        let image = resolver.load_image("diagram.svg").unwrap();
        assert_eq!(image.info.format, ImageFormat::Svg);
        assert_eq!((image.info.width_px, image.info.height_px), (200, 100));
    }

    #[test]
    fn svg_sizes() {
        let measure = |s: &str| {
            let i = probe(s.as_bytes()).unwrap();
            (i.width_px, i.height_px)
        };
        assert_eq!(measure(r#"<svg width="72pt" height="1in"/>"#), (96, 96));
        assert_eq!(measure(r#"<svg viewBox="0 0 40 20"/>"#), (40, 20));
        assert_eq!(
            measure(r#"<svg width="80" viewBox="0 0 40 20"/>"#),
            (80, 40)
        );
        assert_eq!(
            measure(
                "<?xml version=\"1.0\"?>\n<!-- c -->\n<svg xmlns=\"http://www.w3.org/2000/svg\"/>"
            ),
            (300, 150)
        );
    }

    #[test]
    fn a_data_uri_image_loads() {
        let png = fs::read(fixture("pixel-4x3.png")).unwrap();
        let uri = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&png)
        );
        let image = Resolver::default().load_image(&uri).unwrap();
        assert_eq!(image.origin, Origin::DataUri);
        assert_eq!(image.info.width_px, 4);
    }

    #[test]
    fn percent_encoded_paths_fall_back_to_the_decoded_name() {
        let resolver = Resolver::new(Some(fixture("")));
        let image = resolver.load_image("pixel%204x3.png");
        // No such file under either spelling: the error names the reference as written.
        match image {
            Err(Error::Resource { src, .. }) => assert_eq!(src, "pixel%204x3.png"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_missing_file_is_a_resource_error() {
        let err = Resolver::default()
            .load_image("definitely/not/here.png")
            .unwrap_err();
        assert!(matches!(err, Error::Resource { .. }));
    }

    #[test]
    fn not_an_image_is_a_resource_error_naming_the_source() {
        let resolver = Resolver::new(Some(fixture("..")));
        let err = resolver.load_image("refs.bib").unwrap_err();
        assert!(matches!(err, Error::Resource { src, .. } if src == "refs.bib"));
    }

    #[test]
    fn remote_images_are_refused_without_opt_in() {
        let err = Resolver::default()
            .load_image("https://example.invalid/x.png")
            .unwrap_err();
        assert!(matches!(err, Error::Resource { .. }));
    }

    #[test]
    fn fitting_honours_lengths_and_never_upscales() {
        let small = ImageInfo {
            format: ImageFormat::Png,
            width_px: 96,
            height_px: 48,
        };
        let none = Attr::default();
        // 1 in × 0.5 in in a 6 in column: kept as is.
        assert_eq!(
            fit_emu(&small, &none, 6 * EMU_PER_INCH, 0),
            (EMU_PER_INCH, EMU_PER_INCH / 2)
        );
        let attr = |k: &str, v: &str| Attr {
            kv: vec![(k.into(), v.into())],
            ..Attr::default()
        };
        assert_eq!(
            fit_emu(&small, &attr("width", "2in"), 6 * EMU_PER_INCH, 0),
            (2 * EMU_PER_INCH, EMU_PER_INCH)
        );
        assert_eq!(
            fit_emu(&small, &attr("height", "2.54cm"), 0, 0),
            (2 * EMU_PER_INCH, EMU_PER_INCH)
        );
        // A height cap scales both sides.
        assert_eq!(
            fit_emu(&small, &attr("width", "4in"), 0, EMU_PER_INCH),
            (2 * EMU_PER_INCH, EMU_PER_INCH)
        );
    }

    #[test]
    fn remote_detection() {
        assert!(is_remote("https://a/b.png"));
        assert!(is_remote("HTTP://a"));
        assert!(!is_remote("img/http.png"));
        assert!(!is_remote("data:image/png;base64,"));
    }
}
