//! OCF path resolution and media-type fallbacks shared by the streamer.

use livtet_epub::percent_decode;

/// Normalize an href's separators to forward slashes.
pub(crate) fn normalize_href(href: &str) -> String {
    href.replace('\\', "/")
}

/// Percent-decode an href, then reject anything unsafe.
///
/// Decoding runs before the checks so an encoded traversal (`%2e%2e%2f`) or
/// backslash (`%5c`) cannot slip past them. An empty, absolute,
/// backslash-bearing, NUL-bearing, or `..`-bearing result is refused.
pub(crate) fn decode_and_validate(href: &str) -> Option<String> {
    let decoded = percent_decode(href);
    let safe = !decoded.is_empty()
        && !decoded.starts_with('/')
        && !decoded.contains('\\')
        && !decoded.contains('\0')
        && !decoded.split('/').any(|part| part == "..");
    safe.then_some(decoded)
}

/// Resolve `href` against a base directory (`""` for the archive root),
/// collapsing `.` and `..` segments.
pub(crate) fn resolve(base: &str, href: &str) -> String {
    let href = normalize_href(href);
    let combined = if base.is_empty() {
        href
    } else {
        format!("{base}/{href}")
    };
    normalize_segments(&combined)
}

fn normalize_segments(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if out.last().is_some_and(|last| *last != "..") => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out.join("/")
}

/// Strip a URL fragment or query from an href, keeping only the resource path.
pub(crate) fn strip_target(href: &str) -> &str {
    let end = href.find(['#', '?']).unwrap_or(href.len());
    &href[..end]
}

/// Best-effort media type from a file extension, used when the OPF manifest
/// declares no item for the requested href.
pub(crate) fn media_type_for_extension(path: &str) -> Option<&'static str> {
    let extension = path.rsplit('.').next()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "xhtml" => "application/xhtml+xml",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "opf" => "application/oebps-package+xml",
        "ncx" => "application/x-dtbncx+xml",
        "js" => "application/javascript",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        _ => return None,
    })
}
