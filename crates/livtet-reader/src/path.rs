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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn resolve_at_archive_root_keeps_href_and_normalizes_backslashes() {
        assert_eq!(resolve("", "Text\\ch1.xhtml"), "Text/ch1.xhtml");
        assert_eq!(resolve("", "./a/./b.css"), "a/b.css");
    }

    #[test]
    fn resolve_collapses_parent_segments_against_base() {
        assert_eq!(
            resolve("OEBPS/Text", "../Images/c.jpg"),
            "OEBPS/Images/c.jpg"
        );
        assert_eq!(resolve("OEBPS", "a//b/../c.xhtml"), "OEBPS/a/c.xhtml");
    }

    #[test]
    fn resolve_keeps_leading_parents_that_cannot_be_popped() {
        // Escaping the root is preserved (not silently dropped) so
        // `decode_and_validate` can still refuse it.
        assert_eq!(resolve("", "../../x.css"), "../../x.css");
        assert_eq!(resolve("a", "../../x.css"), "../x.css");
    }

    #[test]
    fn decode_and_validate_refuses_unsafe_paths() {
        assert_eq!(
            decode_and_validate("Text/a%20b.xhtml").as_deref(),
            Some("Text/a b.xhtml")
        );
        assert_eq!(decode_and_validate(""), None);
        assert_eq!(decode_and_validate("%2Fetc/passwd"), None);
        assert_eq!(decode_and_validate("a%5cb"), None);
        assert_eq!(decode_and_validate("a%00b"), None);
        assert_eq!(decode_and_validate("a/%2e%2e/b"), None);
        // `..` inside a name is not a traversal segment.
        assert_eq!(decode_and_validate("a..b/c").as_deref(), Some("a..b/c"));
    }

    #[test]
    fn strip_target_drops_fragment_and_query() {
        assert_eq!(strip_target("ch1.xhtml#p3"), "ch1.xhtml");
        assert_eq!(strip_target("ch1.xhtml?x=1#y"), "ch1.xhtml");
        assert_eq!(strip_target("ch1.xhtml"), "ch1.xhtml");
    }

    #[test]
    fn media_type_fallback_is_case_insensitive_and_covers_known_extensions() {
        let cases = [
            ("a.XHTML", "application/xhtml+xml"),
            ("a.htm", "text/html"),
            ("a.html", "text/html"),
            ("a.css", "text/css"),
            ("a.JPEG", "image/jpeg"),
            ("a.jpg", "image/jpeg"),
            ("a.png", "image/png"),
            ("a.gif", "image/gif"),
            ("a.svg", "image/svg+xml"),
            ("content.opf", "application/oebps-package+xml"),
            ("toc.ncx", "application/x-dtbncx+xml"),
            ("a.js", "application/javascript"),
            ("f.woff", "font/woff"),
            ("f.woff2", "font/woff2"),
            ("f.ttf", "font/ttf"),
            ("f.otf", "font/otf"),
        ];
        for (path, expected) in cases {
            assert_eq!(media_type_for_extension(path), Some(expected), "{path}");
        }
        assert_eq!(media_type_for_extension("archive.bin"), None);
        assert_eq!(media_type_for_extension("README"), None);
    }
}
