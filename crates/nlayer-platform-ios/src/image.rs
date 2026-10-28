//! Image bytes for one view.
//!
//! Extracted from `nsh-platform-ios/src/image.rs` and cut to a bundled
//! `asset:/` file or an `http(s)` URL, decoded once with ImageIO. Disk
//! cache, downsampling, SVG, GIF frames, tint, blur and prefetch are not
//! copied. v1 shows PNG, JPEG and WebP only.

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
#[cfg(target_os = "ios")]
pub(crate) struct Decoded {
    pub(crate) image: objc2_core_foundation::CFRetained<objc2_core_graphics::CGImage>,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// Reads `uri` and decodes it. The point size is `width` / `height` divided
/// by the screen scale; this function does not know the scale.
#[cfg(target_os = "ios")]
pub(crate) fn load(assets: &Path, uri: &str) -> Result<Decoded, String> {
    let bytes = read_uri(assets, uri)?;
    if bitmap_kind(&bytes).is_none() {
        return Err("not a PNG, JPEG or WebP".to_owned());
    }
    decode(&bytes)
}

#[cfg(target_os = "ios")]
fn read_uri(assets: &Path, uri: &str) -> Result<Vec<u8>, String> {
    if uri.starts_with("asset:/") {
        let path = asset_file(assets, uri).ok_or_else(|| format!("bad asset URI {uri}"))?;
        return std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()));
    }
    if is_http(uri) {
        return read_http(uri);
    }
    Err(format!("unsupported image URI {uri}"))
}

#[cfg(target_os = "ios")]
fn read_http(uri: &str) -> Result<Vec<u8>, String> {
    use objc2_foundation::{NSData, NSString, NSURL};
    let url =
        NSURL::URLWithString(&NSString::from_str(uri)).ok_or_else(|| format!("bad URL {uri}"))?;
    NSData::dataWithContentsOfURL(&url)
        .map(|data| data.to_vec())
        .ok_or_else(|| format!("could not fetch {uri}"))
}

/// ImageIO, one frame, EXIF orientation applied. Copied from nsh's bitmap
/// decode without the thumbnail cap, blur or animation.
#[cfg(target_os = "ios")]
fn decode(bytes: &[u8]) -> Result<Decoded, String> {
    use objc2_core_foundation::{
        CFBoolean, CFData, CFDictionary, CFString, CFType, kCFBooleanTrue,
    };
    use objc2_core_graphics::CGImage;
    use objc2_image_io::{
        CGImageSource, kCGImageSourceCreateThumbnailFromImageAlways,
        kCGImageSourceCreateThumbnailWithTransform, kCGImageSourceShouldCacheImmediately,
    };

    let data = CFData::from_bytes(bytes);
    // SAFETY: `data` is immutable for this call. The option dictionary holds
    // the documented ImageIO keys and boolean values. The thumbnail is the
    // whole image with its orientation transform applied.
    let image = unsafe {
        let source = CGImageSource::with_data(&data, None).ok_or("ImageIO could not read it")?;
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
