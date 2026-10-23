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
    let root = xml::parse(&opf_bytes)
        .map_err(|error| ReaderError::Package(format!("OPF XML: {error}")))?;
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use std::io::{Cursor, Write};

    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    use super::*;

    fn archive(files: &[(&str, &[u8])]) -> Archive {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in files {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(bytes).unwrap();
        }
        Archive::open(zip.finish().unwrap().into_inner()).unwrap()
    }

    fn opf(metadata: &str, manifest: &str, spine: &str) -> String {
        format!(
            r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="3.0">
  <metadata>{metadata}</metadata>
  <manifest>{manifest}</manifest>
  {spine}
</package>"#
        )
    }

    const CHAPTER: &str = r#"<item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"/>"#;
    const NAV_ITEM: &str =
        r#"<item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>"#;
    const NCX_ITEM: &str =
        r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>"#;
    const NCX: &str = r#"<ncx><navMap>
  <navPoint><navLabel><text>  Chapter
     One </text></navLabel><content src="ch1.xhtml#start"/></navPoint>
  <navPoint><navLabel><text>No target</text></navLabel></navPoint>
  <navPoint><navLabel><text>   </text></navLabel><content src="ch1.xhtml"/></navPoint>
</navMap></ncx>"#;

    fn toc(package: &Package) -> Vec<(&str, &str)> {
        package
            .toc
            .iter()
            .map(|entry| (entry.href.as_str(), entry.title.as_str()))
            .collect()
    }

    #[test]
    fn spine_skips_itemrefs_without_or_with_unknown_idrefs() {
        let doc = opf(
            "<dc:title>T</dc:title>",
            CHAPTER,
            r#"<spine page-progression-direction="RTL"><itemref/><itemref idref="ghost"/><itemref idref="ch1"/></spine>"#,
        );
        let mut archive = archive(&[("content.opf", doc.as_bytes())]);
        let package = parse(&mut archive, "content.opf", "file").unwrap();

        let ids: Vec<&str> = package
            .reading_order
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        assert_eq!(ids, ["ch1"]);
        assert_eq!(
            package.reading_progression, "rtl",
            "direction is case-insensitive"
        );
    }

    #[test]
    fn spine_of_only_dangling_itemrefs_is_unsupported() {
        let doc = opf(
            "<dc:title>T</dc:title>",
            CHAPTER,
            r#"<spine><itemref idref="ghost"/></spine>"#,
        );
        let mut archive = archive(&[("content.opf", doc.as_bytes())]);
        let error = parse(&mut archive, "content.opf", "file").unwrap_err();
        assert!(matches!(error, ReaderError::Unsupported(_)), "{error:?}");
    }

    #[test]
    fn missing_title_and_creator_fall_back_to_file_name_and_contributors() {
        let doc = opf(
            "<dc:contributor> Ed   Itor </dc:contributor><dc:title>   </dc:title>",
            CHAPTER,
            r#"<spine><itemref idref="ch1"/></spine>"#,
        );
        let mut archive = archive(&[("content.opf", doc.as_bytes())]);
        let package = parse(&mut archive, "content.opf", "My Book").unwrap();

        assert_eq!(package.title, "My Book");
        assert_eq!(package.authors, ["Ed Itor"]);
        assert_eq!(package.reading_progression, "ltr");
    }

    #[test]
    fn toc_falls_back_to_ncx_when_nav_has_no_usable_entries() {
        // The only nav anchor has an empty title, so the NCX supplies the TOC.
        let nav =
            br#"<html><body><nav><ol><li><a href="ch1.xhtml"> </a></li></ol></nav></body></html>"#;
        let doc = opf(
            "<dc:title>T</dc:title>",
            &format!("{CHAPTER}{NAV_ITEM}{NCX_ITEM}"),
            r#"<spine><itemref idref="ch1"/></spine>"#,
        );
        let mut archive = archive(&[
            ("content.opf", doc.as_bytes()),
            ("nav.xhtml", nav.as_slice()),
            ("toc.ncx", NCX.as_bytes()),
        ]);
        let package = parse(&mut archive, "content.opf", "file").unwrap();

        assert_eq!(toc(&package), [("ch1.xhtml", "Chapter One")]);
        assert!(
            package.resources.is_empty(),
            "nav and NCX are not exposed as resources: {:?}",
            package.resources
        );
    }

    #[test]
    fn ncx_hrefs_resolve_against_the_ncx_directory() {
        let doc = opf(
            "<dc:title>T</dc:title>",
            r#"<item id="ch1" href="Text/ch1.xhtml" media-type="application/xhtml+xml"/><item id="ncx" href="Misc/toc.ncx" media-type="application/x-dtbncx+xml"/>"#,
            r#"<spine><itemref idref="ch1"/></spine>"#,
        );
        let ncx = br#"<ncx><navPoint><navLabel><text>One</text></navLabel><content src="../Text/ch1.xhtml"/></navPoint></ncx>"#;
        let mut archive = archive(&[
            ("OEBPS/content.opf", doc.as_bytes()),
            ("OEBPS/Misc/toc.ncx", ncx.as_slice()),
        ]);
        let package = parse(&mut archive, "OEBPS/content.opf", "file").unwrap();

        assert_eq!(toc(&package), [("OEBPS/Text/ch1.xhtml", "One")]);
    }

    #[test]
    fn toc_is_empty_when_nav_and_ncx_are_missing_or_unparseable() {
        let doc = opf(
            "<dc:title>T</dc:title>",
            &format!("{CHAPTER}{NAV_ITEM}{NCX_ITEM}"),
            r#"<spine><itemref idref="ch1"/></spine>"#,
        );

        // Neither document is in the archive.
        let mut missing = archive(&[("content.opf", doc.as_bytes())]);
        let package = parse(&mut missing, "content.opf", "file").unwrap();
        assert!(package.toc.is_empty());

        // Both documents exist but contain no XML element.
        let mut garbage = archive(&[
            ("content.opf", doc.as_bytes()),
            ("nav.xhtml", b"plain text".as_slice()),
            ("toc.ncx", b"plain text".as_slice()),
        ]);
        let package = parse(&mut garbage, "content.opf", "file").unwrap();
        assert!(package.toc.is_empty());
    }

    #[test]
    fn nav_document_without_a_nav_element_falls_back_to_ncx() {
        let doc = opf(
            "<dc:title>T</dc:title>",
            &format!("{CHAPTER}{NAV_ITEM}{NCX_ITEM}"),
            r#"<spine><itemref idref="ch1"/></spine>"#,
        );
        let mut archive = archive(&[
            ("content.opf", doc.as_bytes()),
            (
                "nav.xhtml",
                br#"<html><body><a href="ch1.xhtml">Stray</a></body></html>"#.as_slice(),
            ),
            ("toc.ncx", NCX.as_bytes()),
        ]);
        let package = parse(&mut archive, "content.opf", "file").unwrap();

        assert_eq!(toc(&package), [("ch1.xhtml", "Chapter One")]);
    }

    #[test]
    fn nav_prefers_the_toc_typed_nav_over_earlier_navs() {
        let nav = br#"<html xmlns:epub="http://www.idpf.org/2007/ops"><body>
<nav epub:type="landmarks"><a href="ch1.xhtml">Landmark</a></nav>
<nav epub:type="TOC"><a href="ch1.xhtml#p">Real Entry</a></nav>
</body></html>"#;
        let doc = opf(
            "<dc:title>T</dc:title>",
            &format!("{CHAPTER}{NAV_ITEM}"),
            r#"<spine><itemref idref="ch1"/></spine>"#,
        );
        let mut archive = archive(&[
            ("content.opf", doc.as_bytes()),
            ("nav.xhtml", nav.as_slice()),
        ]);
        let package = parse(&mut archive, "content.opf", "file").unwrap();

        assert_eq!(toc(&package), [("ch1.xhtml", "Real Entry")]);
    }
}
