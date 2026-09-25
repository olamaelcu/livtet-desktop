//! Integration tests: build an in-memory EPUB and stream it as a Readium
//! Web Publication.

#[path = "../../livtet-epub/src/test_support.rs"]
mod test_support;

use livtet_reader::Reader;
use serde_json::json;
use test_support::EpubBuilder;

const OPF: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Reader Test</dc:title>
    <dc:creator>Jane Doe</dc:creator>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="ch1" href="ch01.xhtml" media-type="application/xhtml+xml"/>
    <item id="ch2" href="ch02.xhtml" media-type="application/xhtml+xml"/>
    <item id="style" href="style.css" media-type="text/css"/>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
    <itemref idref="ch2"/>
  </spine>
</package>"#;

const NAV: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<body>
<nav epub:type="toc"><ol>
<li><a href="ch01.xhtml">Chapter One</a></li>
<li><a href="ch02.xhtml">Chapter Two</a></li>
</ol></nav>
</body></html>"#;

fn fixture() -> tempfile::NamedTempFile {
    EpubBuilder::new(OPF)
        .file(
            "OEBPS/ch01.xhtml",
            b"<html><body>One</body></html>".to_vec(),
        )
        .file(
            "OEBPS/ch02.xhtml",
            b"<html><body>Two</body></html>".to_vec(),
        )
        .file("OEBPS/style.css", b"body{color:red}".to_vec())
        .file("OEBPS/nav.xhtml", NAV.as_bytes().to_vec())
        .tempfile()
}

#[test]
fn manifest_has_self_link_reading_order_and_title() {
    let reader = Reader::open(fixture().path()).unwrap();
    let manifest = reader.manifest("reader://localhost/abc/");

    assert_eq!(manifest["metadata"]["title"], "Reader Test");
    assert_eq!(manifest["metadata"]["author"], json!(["Jane Doe"]));
    assert_eq!(manifest["metadata"]["language"], json!(["en"]));

    let links = manifest["links"].as_array().unwrap();
    assert!(links.iter().any(|link| {
        link["rel"] == json!(["self"]) && link["href"] == "reader://localhost/abc/manifest.json"
    }));

    let reading_order = manifest["readingOrder"].as_array().unwrap();
    assert_eq!(reading_order.len(), 2);
    assert_eq!(reading_order[0]["href"], "OEBPS/ch01.xhtml");
    assert_eq!(reading_order[0]["type"], "application/xhtml+xml");

    let toc = manifest["toc"].as_array().unwrap();
    assert_eq!(toc[0]["title"], "Chapter One");
    assert_eq!(toc[0]["href"], "OEBPS/ch01.xhtml");
}

#[test]
fn positions_follow_reading_order() {
    let reader = Reader::open(fixture().path()).unwrap();
    let positions = reader.positions();

    assert_eq!(positions.len(), 2);
    assert_eq!(positions[0].locations.position, 1);
    assert_eq!(positions[0].locations.progression, 0.0);
    assert_eq!(positions[1].locations.total_progression, 1.0);
    assert_eq!(positions[0].title.as_deref(), Some("Chapter One"));

    let value = serde_json::to_value(&positions[0]).unwrap();
    assert!(value["locations"].get("totalProgression").is_some());
    assert!(value["locations"].get("total_progression").is_none());
}

#[test]
fn reads_spine_item_and_css() {
    let reader = Reader::open(fixture().path()).unwrap();

    let (mime, bytes) = reader.read("OEBPS/ch01.xhtml").unwrap();
    assert_eq!(mime, "application/xhtml+xml");
    assert_eq!(bytes, b"<html><body>One</body></html>".to_vec());

    let (mime, bytes) = reader.read("OEBPS/style.css").unwrap();
    assert_eq!(mime, "text/css");
    assert_eq!(bytes, b"body{color:red}".to_vec());

    let (mime, _) = reader.read("ch02.xhtml").unwrap();
    assert_eq!(mime, "application/xhtml+xml");
}

#[test]
fn rejects_traversal_and_unknown_hrefs() {
    let reader = Reader::open(fixture().path()).unwrap();

    assert!(reader.read("../../etc/passwd").is_none());
    assert!(reader.read("/etc/passwd").is_none());
    assert!(reader.read("..\\..\\etc\\passwd").is_none());
    assert!(reader.read("OEBPS/missing.xhtml").is_none());

    assert!(!reader.contains("../../etc/passwd"));
    assert!(!reader.contains("OEBPS/missing.xhtml"));
    assert!(reader.contains("OEBPS/ch01.xhtml"));
}

#[test]
fn open_rejects_non_zip_files() {
    use std::io::Write;

    let mut file = tempfile::Builder::new().suffix(".epub").tempfile().unwrap();
    file.write_all(b"this is not a zip archive").unwrap();
    file.flush().unwrap();

    assert!(Reader::open(file.path()).is_err());
}
