//! Front-cover resolution from the OPF manifest, `<meta name="cover">`, and the
//! guide, with a media-type guard and an encryption check.

use crate::encryption::Encryption;
use crate::xml::Element;
use crate::zip::Archive;

/// Front-cover image bytes and media type as declared in the manifest.
#[derive(Debug, Clone)]
pub struct Cover {
    pub data: Vec<u8>,
    pub mime: String,
}

/// Resolve the cover, if one can be found and safely read.
///
/// Resolution order: EPUB3 `cover-image` manifest property, then the
/// `<meta name="cover">` hint (resolved as a manifest id, then an href), then a
/// `<guide>` `type="cover"` reference. Any selected item must declare a
/// non-XHTML media type; if it is encrypted with a real cipher the cover is
/// skipped rather than returned as ciphertext.
pub(crate) fn extract(
    archive: &mut Archive,
    package: &Element,
    opf_dir: &str,
    encryption: &Encryption,
) -> Option<Cover> {
    let items: Vec<&Element> = package
        .find("manifest")
        .map(|manifest| manifest.find_all("item"))
        .unwrap_or_default();

    if let Some(item) = items.iter().find(|item| has_cover_image_property(item)) {
        return from_item(item, opf_dir, archive, encryption);
    }

    if let Some(content) = meta_cover_content(package) {
        let item = items
            .iter()
            .find(|item| item.attr("id") == Some(content))
            .or_else(|| {
                items.iter().find(|item| {
                    item.attr("href")
                        .is_some_and(|href| same_path(href, content))
                })
            });
        if let Some(item) = item {
            return from_item(item, opf_dir, archive, encryption);
        }
    }

    if let Some(href) = guide_cover_href(package) {
        let item = items
            .iter()
            .find(|item| item.attr("href").is_some_and(|h| same_path(h, href)));
        if let Some(item) = item {
            return from_item(item, opf_dir, archive, encryption);
        }
    }

    None
}

fn from_item(
    item: &Element,
    opf_dir: &str,
    archive: &mut Archive,
    encryption: &Encryption,
) -> Option<Cover> {
    let media_type = item.attr("media-type")?;
    let lower = media_type.to_ascii_lowercase();
    if lower.contains("xml") || lower.contains("html") {
        return None;
    }
    let href = item.attr("href")?;
    let path = resolve_path(opf_dir, href);
    if encryption.is_encrypted(&path).is_some() || encryption.is_encrypted(href).is_some() {
        return None;
    }
    let data = archive.read(&path)?;
    Some(Cover {
        data,
        mime: media_type.to_string(),
    })
}

fn has_cover_image_property(item: &Element) -> bool {
    item.attr("properties").is_some_and(|properties| {
        properties
            .split_whitespace()
            .any(|token| token.eq_ignore_ascii_case("cover-image"))
    })
}

fn meta_cover_content(package: &Element) -> Option<&str> {
    package
        .find_all("meta")
        .into_iter()
        .find(|meta| {
            meta.attr("name")
                .is_some_and(|name| name.eq_ignore_ascii_case("cover"))
        })
        .and_then(|meta| meta.attr("content"))
        .map(str::trim)
        .filter(|content| !content.is_empty())
}

fn guide_cover_href(package: &Element) -> Option<&str> {
    let guide = package.find("guide")?;
    guide
        .find_all("reference")
        .into_iter()
        .find(|reference| {
            reference
                .attr("type")
                .is_some_and(|kind| kind.eq_ignore_ascii_case("cover"))
        })
        .and_then(|reference| reference.attr("href"))
}

/// Resolve an href relative to the OPF directory and percent-decode it.
pub(crate) fn resolve_path(opf_dir: &str, href: &str) -> String {
    let decoded = percent_decode(href);
    let joined = if let Some(stripped) = decoded.strip_prefix('/') {
        stripped.to_string()
    } else if opf_dir.is_empty() {
        decoded
    } else {
        format!("{opf_dir}/{decoded}")
    };
    joined.replace('\\', "/")
}

fn same_path(a: &str, b: &str) -> bool {
    a.replace('\\', "/") == b.replace('\\', "/")
}

pub(crate) fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let (Some(high), Some(low)) = (
                hex_value(bytes.get(index + 1)),
                hex_value(bytes.get(index + 2)),
            )
        {
            out.push(high * 16 + low);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(byte: Option<&u8>) -> Option<u8> {
    byte.and_then(|b| (*b as char).to_digit(16))
        .map(|d| d as u8)
}
