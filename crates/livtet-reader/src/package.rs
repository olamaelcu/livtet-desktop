//! OPF package document parsing into the data the streamer exposes.

use std::collections::{BTreeMap, BTreeSet};

use livtet_epub::xml::{self, Element};
use livtet_epub::{Archive, ocf};

use crate::error::ReaderError;
use crate::path::{decode_and_validate, resolve, strip_target};

#[derive(Clone, Debug)]
pub(crate) struct ManifestItem {
    pub id: String,
    pub href: String,
    pub media_type: String,
    pub properties: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct TocEntry {
    pub href: String,
    pub title: String,
}

#[derive(Debug)]
pub(crate) struct Package {
    pub title: String,
    pub authors: Vec<String>,
    pub language: Vec<String>,
    pub reading_progression: String,
    pub reading_order: Vec<ManifestItem>,
    pub resources: Vec<ManifestItem>,
    pub media_types: BTreeMap<String, String>,
    pub toc: Vec<TocEntry>,
}

/// Parse the OPF at `opf_path` into the manifest, reading-order, resource, and
/// table-of-contents data the streamer needs. `file_name` (no extension) is the
/// title fallback when the package declares none.
pub(crate) fn parse(
    archive: &mut Archive,
    opf_path: &str,
    file_name: &str,
) -> Result<Package, ReaderError> {
    let opf_bytes = archive
        .read(opf_path)
        .ok_or_else(|| ReaderError::Package(format!("OPF document {opf_path} is missing")))?;
    let root = xml::parse(&opf_bytes)?;
    let base_dir = ocf::base_dir(opf_path);
    let metadata = root.find("metadata").unwrap_or(&root);

    let items: Vec<ManifestItem> = root
        .find("manifest")
        .map(|manifest| {
            manifest
                .find_all("item")
                .into_iter()
                .filter_map(|item| manifest_item(item, &base_dir))
                .collect()
        })
        .unwrap_or_default();

    let spine = root.find("spine");
    let mut spine_ids: BTreeSet<String> = BTreeSet::new();
    let mut reading_order: Vec<ManifestItem> = Vec::new();
    if let Some(spine) = spine {
        for itemref in spine.find_all("itemref") {
            let Some(idref) = itemref.attr("idref") else {
                continue;
            };
            let Some(item) = items
                .iter()
                .find(|item| !item.id.is_empty() && item.id == idref)
            else {
                continue;
            };
            spine_ids.insert(item.id.clone());
            reading_order.push(item.clone());
        }
    }
    if reading_order.is_empty() {
        return Err(ReaderError::Unsupported(
            "EPUB declares no spine reading order".to_string(),
        ));
    }

    let reading_progression = match spine.and_then(|s| s.attr("page-progression-direction")) {
        Some(direction) if direction.eq_ignore_ascii_case("rtl") => "rtl",
        _ => "ltr",
    }
    .to_string();

    let resources: Vec<ManifestItem> = items
        .iter()
        .filter(|item| !spine_ids.contains(&item.id))
        .filter(|item| !is_nav(item) && !is_ncx(item))
        .cloned()
        .collect();

    // Keyed by the decoded archive path so it matches the candidate
    // `Reader::read` resolves from an (encoded) request href.
    let media_types = items
        .iter()
        .filter_map(|item| {
            decode_and_validate(&item.href).map(|path| (path, item.media_type.clone()))
        })
        .collect();

    let title = first_text(metadata, "title").unwrap_or_else(|| file_name.to_string());
    let mut authors = all_text(metadata, "creator");
    if authors.is_empty() {
        authors = all_text(metadata, "contributor");
    }
    let language = all_text(metadata, "language");
    let toc = extract_toc(archive, &items);

    Ok(Package {
        title,
        authors,
        language,
        reading_progression,
        reading_order,
        resources,
        media_types,
        toc,
    })
}

fn manifest_item(item: &Element, base_dir: &str) -> Option<ManifestItem> {
    let href = item.attr("href")?;
    Some(ManifestItem {
        id: item.attr("id").unwrap_or_default().to_string(),
        href: resolve(base_dir, strip_target(href)),
        media_type: item
            .attr("media-type")
            .unwrap_or("application/octet-stream")
            .to_string(),
        properties: item
            .attr("properties")
            .map(|properties| properties.split_whitespace().map(str::to_string).collect())
            .unwrap_or_default(),
    })
}

fn is_nav(item: &ManifestItem) -> bool {
    item.properties
        .iter()
        .any(|property| property.eq_ignore_ascii_case("nav"))
}

fn is_ncx(item: &ManifestItem) -> bool {
    item.media_type
        .eq_ignore_ascii_case("application/x-dtbncx+xml")
}

/// Table of contents from an EPUB3 navigation document, falling back to an
/// EPUB2 NCX. An empty result is allowed; the manifest simply omits `toc`.
fn extract_toc(archive: &mut Archive, items: &[ManifestItem]) -> Vec<TocEntry> {
    if let Some(nav) = items.iter().find(|item| is_nav(item)) {
        let entries = nav_toc(archive, nav);
        if !entries.is_empty() {
            return entries;
        }
    }
    if let Some(ncx) = items.iter().find(|item| is_ncx(item)) {
        return ncx_toc(archive, ncx);
    }
    Vec::new()
}

fn nav_toc(archive: &mut Archive, nav: &ManifestItem) -> Vec<TocEntry> {
    let Some(bytes) = archive.read(&nav.href) else {
        return Vec::new();
    };
    let Ok(root) = xml::parse(&bytes) else {
        return Vec::new();
    };
    let dir = document_dir(&nav.href);
    let navs = root.find_all("nav");
    let scope = navs
        .iter()
        .find(|element| {
            element
                .attr("type")
                .is_some_and(|kind| kind.eq_ignore_ascii_case("toc"))
        })
        .or_else(|| navs.first())
        .copied();
    let Some(scope) = scope else {
        return Vec::new();
    };
    scope
        .find_all("a")
        .into_iter()
        .filter_map(|anchor| {
            let href = resolve(&dir, strip_target(anchor.attr("href")?));
            let title = collapse(&anchor.text());
            (!href.is_empty() && !title.is_empty()).then_some(TocEntry { href, title })
        })
        .collect()
}

fn ncx_toc(archive: &mut Archive, ncx: &ManifestItem) -> Vec<TocEntry> {
    let Some(bytes) = archive.read(&ncx.href) else {
        return Vec::new();
    };
    let Ok(root) = xml::parse(&bytes) else {
        return Vec::new();
    };
    let dir = document_dir(&ncx.href);
    root.find_all("navPoint")
        .into_iter()
        .filter_map(|nav_point| {
            let title = collapse(&nav_point.find("navLabel")?.find("text")?.text());
            let href = resolve(&dir, strip_target(nav_point.find("content")?.attr("src")?));
            (!href.is_empty() && !title.is_empty()).then_some(TocEntry { href, title })
        })
        .collect()
}

fn document_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(index) => path[..index].to_string(),
        None => String::new(),
    }
}

fn first_text(metadata: &Element, local: &str) -> Option<String> {
    all_text(metadata, local).into_iter().next()
}

fn all_text(metadata: &Element, local: &str) -> Vec<String> {
    metadata
        .find_all(local)
        .into_iter()
        .map(|element| collapse(&element.text()))
        .filter(|text| !text.is_empty())
        .collect()
}

fn collapse(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
