//! Image bytes for one view.
//!
//! Extracted from `nsh-platform-ios/src/image.rs` and cut to a bundled
//! `asset:/` file or an `http(s)` URL. A URI is decoded once; later views
//! share that bitmap, and a burst of the same URI shares the load. The
//! cache is bounded and drops the least recently used bitmaps. Disk cache,
//! downsampling, SVG, GIF frames, tint, blur and prefetch are not copied.
//! v1 shows PNG, JPEG and WebP only.

use std::path::{Component, Path, PathBuf};

/// `asset:/img/a.png` inside `root`, or `None` when `uri` is not a bundled
/// asset or the path leaves `root`.
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
pub(crate) fn asset_file(root: &Path, uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("asset:/")?;
    let rest = rest.trim_start_matches('/');
    if rest.is_empty() {
        return None;
    }
    let rel = Path::new(rest);
    if rel.is_absolute()
        || rel.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        })
    {
        return None;
    }
    Some(root.join(rel))
}

/// An `http:` or `https:` URL with a host. `data:` and other schemes are not
/// images in v1.
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
pub(crate) fn is_http(uri: &str) -> bool {
    let Some((scheme, rest)) = uri.split_once("://") else {
        return false;
    };
    !rest.is_empty()
        && (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
}

/// A bitmap v1 will draw. Anything else (GIF, SVG, an unknown file) is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
pub(crate) enum BitmapKind {
    /// PNG.
    Png,
    /// JPEG.
    Jpeg,
    /// WebP.
    Webp,
}

/// The bitmap `bytes` starts with, if it is one v1 decodes.
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
pub(crate) fn bitmap_kind(bytes: &[u8]) -> Option<BitmapKind> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(BitmapKind::Png);
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(BitmapKind::Jpeg);
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some(BitmapKind::Webp);
    }
    None
}

/// A decoded bitmap, in pixels, orientation already applied.
///
/// A finished `CGImage` is immutable. It is decoded off the main thread and
/// then shared with every view that shows the same URI.
#[cfg(target_os = "ios")]
#[derive(Clone)]
pub(crate) struct Decoded {
    pub(crate) image: objc2_core_foundation::CFRetained<objc2_core_graphics::CGImage>,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

// SAFETY: ImageIO publishes a finished image; nothing mutates its pixels.
#[cfg(target_os = "ios")]
unsafe impl Send for Decoded {}

/// Bytes of decoded pixels kept before the oldest entry is dropped.
/// nsh's `NSCache` uses the same ceiling and is purged under pressure;
/// this map only has the ceiling.
#[cfg(target_os = "ios")]
const CACHE_BYTES: usize = 96 << 20;

#[cfg(target_os = "ios")]
struct Cache {
    /// Oldest at the front. A hit moves the entry to the back.
    entries: Vec<(std::sync::Arc<str>, Decoded)>,
    bytes: usize,
}

#[cfg(target_os = "ios")]
fn cache() -> &'static std::sync::Mutex<Cache> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<Cache>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| {
        std::sync::Mutex::new(Cache {
            entries: Vec::new(),
            bytes: 0,
        })
    })
}

#[cfg(target_os = "ios")]
fn cost(decoded: &Decoded) -> usize {
    (decoded.width as usize)
        .saturating_mul(decoded.height as usize)
        .saturating_mul(4)
}

/// A bitmap already decoded for `uri`, if one is still cached.
#[cfg(target_os = "ios")]
pub(crate) fn cached(uri: &str) -> Option<Decoded> {
    let mut cache = cache().lock().expect("image cache");
    let index = cache
        .entries
        .iter()
        .position(|(key, _)| key.as_ref() == uri)?;
    let decoded = cache.entries[index].1.clone();
    let entry = cache.entries.remove(index);
    cache.entries.push(entry);
    Some(decoded)
}

/// Keeps `decoded` for later views of `uri`.
#[cfg(target_os = "ios")]
pub(crate) fn store(uri: std::sync::Arc<str>, decoded: &Decoded) {
    let mut cache = cache().lock().expect("image cache");
    if let Some(index) = cache
        .entries
        .iter()
        .position(|(key, _)| key.as_ref() == uri.as_ref())
    {
        cache.bytes = cache.bytes.saturating_sub(cost(&cache.entries[index].1));
        cache.entries.remove(index);
    }
    let added = cost(decoded);
    while cache.bytes + added > CACHE_BYTES && !cache.entries.is_empty() {
        let (_, old) = cache.entries.remove(0);
        cache.bytes = cache.bytes.saturating_sub(cost(&old));
    }
    cache.bytes += added;
    cache.entries.push((uri, decoded.clone()));
}

/// Reads `uri` and decodes it. The point size is `width` / `height` divided
/// by the screen scale; this function does not know the scale.
#[cfg(target_os = "ios")]
pub(crate) fn load(assets: &Path, uri: &str) -> Result<Decoded, String> {
    let data = read_data(assets, uri)?;
    // SAFETY: the data is immutable for this sniff.
    let bytes = unsafe { data.as_bytes_unchecked() };
    if bitmap_kind(bytes).is_none() {
        return Err("not a PNG, JPEG or WebP".to_owned());
    }
    decode(&data)
}

#[cfg(target_os = "ios")]
fn read_data(
    assets: &Path,
    uri: &str,
) -> Result<objc2::rc::Retained<objc2_foundation::NSData>, String> {
    use objc2_foundation::{NSData, NSDataReadingOptions, NSString, NSURL};

    if uri.starts_with("asset:/") {
        let path = asset_file(assets, uri).ok_or_else(|| format!("bad asset URI {uri}"))?;
        // Mapped when the file is a regular file: ImageIO reads it without
        // a second copy into a `Vec`.
        return NSData::dataWithContentsOfFile_options_error(
            &NSString::from_str(&path.to_string_lossy()),
            NSDataReadingOptions::MappedIfSafe,
        )
        .map_err(|error| format!("{}: {}", path.display(), error.localizedDescription()));
    }
    if is_http(uri) {
        let url = NSURL::URLWithString(&NSString::from_str(uri))
            .ok_or_else(|| format!("bad URL {uri}"))?;
        return NSData::dataWithContentsOfURL(&url).ok_or_else(|| format!("could not fetch {uri}"));
    }
    Err(format!("unsupported image URI {uri}"))
}

/// ImageIO, one frame, EXIF orientation applied. Copied from nsh's bitmap
/// decode without the thumbnail cap, blur or animation. `data` is decoded
/// in place: it is not copied into a `CFData`.
#[cfg(target_os = "ios")]
fn decode(data: &objc2_foundation::NSData) -> Result<Decoded, String> {
    use objc2_core_foundation::{
        CFBoolean, CFData, CFDictionary, CFString, CFType, kCFBooleanTrue,
    };
    use objc2_core_graphics::CGImage;
    use objc2_image_io::{
        CGImageSource, kCGImageSourceCreateThumbnailFromImageAlways,
        kCGImageSourceCreateThumbnailWithTransform, kCGImageSourceShouldCacheImmediately,
    };

    // SAFETY: `data` is immutable for this call and outlives the source.
    // NSData is toll-free bridged with CFData. The option dictionary holds
    // the documented ImageIO keys. The thumbnail is the whole image with
    // its orientation transform applied, and it is decoded now so a mapped
    // file can be unmapped after this returns.
    let image = unsafe {
        let cf = &*(std::ptr::from_ref(data).cast::<CFData>());
        let source = CGImageSource::with_data(cf, None).ok_or("ImageIO could not read it")?;
        let yes = kCFBooleanTrue.ok_or("missing kCFBooleanTrue")?;
        let yes: &CFType = &*(yes as *const CFBoolean).cast::<CFType>();
        let keys: [&CFString; 3] = [
            kCGImageSourceCreateThumbnailFromImageAlways,
            kCGImageSourceCreateThumbnailWithTransform,
            kCGImageSourceShouldCacheImmediately,
        ];
        let values: [&CFType; 3] = [yes, yes, yes];
        let options = CFDictionary::from_slices(&keys, &values);
        source
            .thumbnail_at_index(0, Some(options.as_opaque()))
            .ok_or("ImageIO could not decode it")?
    };
    let width = CGImage::width(Some(&image));
    let height = CGImage::height(Some(&image));
    if width == 0 || height == 0 {
        return Err("image has no pixels".to_owned());
    }
    Ok(Decoded {
        image,
        width: u32::try_from(width).unwrap_or(u32::MAX),
        height: u32::try_from(height).unwrap_or(u32::MAX),
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{BitmapKind, asset_file, bitmap_kind, is_http};

    #[test]
    fn an_asset_stays_inside_its_directory() {
        let root = Path::new("/docs");
        assert_eq!(
            asset_file(root, "asset:/img/a.png").as_deref(),
            Some(Path::new("/docs/img/a.png"))
        );
        assert_eq!(
            asset_file(root, "asset:///img/a.png").as_deref(),
            Some(Path::new("/docs/img/a.png"))
        );
        assert!(asset_file(root, "asset:/../a.png").is_none());
        assert!(asset_file(root, "asset:/img/../../a.png").is_none());
        assert!(asset_file(root, "asset://").is_none());
        assert!(asset_file(root, "https://example.com/a.png").is_none());
    }

    #[test]
    fn only_http_urls_are_remote_images() {
        assert!(is_http("https://example.com/a.png"));
        assert!(is_http("http://example.com/a.jpg"));
        assert!(!is_http("asset:/a.png"));
        assert!(!is_http("data:image/png;base64,aaaa"));
        assert!(!is_http("https://"));
    }

    #[test]
    fn bitmap_sniff_accepts_png_jpeg_and_webp_only() {
        assert_eq!(bitmap_kind(b"\x89PNG\r\n\x1a\nrest"), Some(BitmapKind::Png));
        assert_eq!(
            bitmap_kind(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some(BitmapKind::Jpeg)
        );
        let mut webp = b"RIFF....WEBP".to_vec();
        webp[4..8].copy_from_slice(b"size");
        assert_eq!(bitmap_kind(&webp), Some(BitmapKind::Webp));
        assert_eq!(bitmap_kind(b"GIF89a"), None);
        assert_eq!(bitmap_kind(b"<svg"), None);
        assert_eq!(bitmap_kind(&[0xFF, 0xD8]), None);
    }
}
