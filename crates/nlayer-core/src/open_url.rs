//! Checking a URL before it is opened outside the app: a `Text` `href` a
//! press opens ([`crate::Command::OpenUrl`]).
//!
//! The check is nsh's (`nsh_core::open_url`), without its capability:
//! the address is absolute, at most 16 KiB, free of whitespace and control
//! characters, and of a scheme another app can open.

/// The longest URL opened, in bytes (a `mailto:` link with a body fits).
pub const MAX_URL_LEN: usize = 16 * 1024;

/// Schemes never opened: they run code or reach the app's own files and
/// data rather than another app.
const REFUSED_SCHEMES: &[&str] = &[
    "javascript",
    "vbscript",
    "data",
    "blob",
    "file",
    "about",
    "asset",
];

/// Checks a URL a `Text` `href` asks to open: absolute (a scheme), at most
/// [`MAX_URL_LEN`] bytes, no whitespace or control characters, and of a
/// scheme that opens another app (not `javascript:`, `data:`, `blob:`,
/// `file:`, `about:` or `asset:`). The error says what is wrong.
pub fn check_url(url: &str) -> Result<(), String> {
    if url.len() > MAX_URL_LEN {
        return Err(format!("openUrl: a URL is at most {MAX_URL_LEN} bytes"));
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("openUrl: a URL has no whitespace or control characters".into());
    }
    let parsed = url::Url::parse(url).map_err(|e| format!("openUrl: not a URL: {e} ({url})"))?;
    let scheme = parsed.scheme();
    if REFUSED_SCHEMES.contains(&scheme) {
        return Err(format!("openUrl: `{scheme}:` URLs are not opened"));
    }
    if matches!(scheme, "http" | "https") && parsed.host_str().is_none_or(str::is_empty) {
        return Err(format!("openUrl: a web address has a host ({url})"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_of_other_apps_are_opened() {
        for url in [
            "https://example.com/a?b=c#d",
            "http://localhost:8080",
            "mailto:ada@example.com?subject=Hi%20there",
            "tel:+1-555-0100",
            "sms:+15550100",
            "maps://?q=Paris",
            "lb://router/stock/700.HK",
        ] {
            assert_eq!(check_url(url), Ok(()), "{url}");
        }
    }

    #[test]
    fn other_urls_are_refused() {
        for url in [
            "",
            "example.com",
            "/relative/path",
            "#anchor",
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            "vbscript:msgbox(1)",
            "data:text/html,<b>x</b>",
            "file:///etc/passwd",
            "blob:https://example.com/1",
            "about:blank",
            "asset:/logo.png",
            "https://exa mple.com",
            "https://example.com/\n",
        ] {
            assert!(check_url(url).is_err(), "{url}");
        }
        let long = format!("https://example.com/{}", "a".repeat(MAX_URL_LEN));
        assert!(check_url(&long).is_err());
    }
}
