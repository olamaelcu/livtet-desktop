//! Integration tests: build minimal in-memory EPUBs and assert extraction.

use livtet_epub::{read_metadata, EpubError, Role};

use std::io::{Seek, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

struct Fixture {
    opf_metadata: String,
    with_cover: bool,
}

/// Write a minimal but spec-valid EPUB to a temp file and return its path.
fn write_epub(f: &Fixture) -> tempfile::NamedTempFile {
    let file = tempfile::Builder::new().suffix(".epub").tempfile().unwrap();
    let mut zip = ZipWriter::new(file.reopen().unwrap());
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    zip.start_file("mimetype", stored).unwrap();
    zip.write_all(b"application/epub+zip").unwrap();

    zip.start_file("META-INF/container.xml", deflated).unwrap();
    zip.write_all(
        br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
    )
    .unwrap();

    if f.with_cover {
        zip.start_file("OEBPS/cover.png", deflated).unwrap();
        zip.write_all(b"\x89PNG\r\n\x1a\nfake-png-bytes").unwrap();
    }

    let cover_manifest = if f.with_cover {
        r#"<item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/>"#
    } else {
        ""
    };
    let opf = format!(
        r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="pub-id" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    {metadata}
  </metadata>
  <manifest>
    <item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/>
    {cover_manifest}
  </manifest>
  <spine><itemref idref="chapter"/></spine>
</package>"#,
        metadata = f.opf_metadata
    );
    zip.start_file("OEBPS/content.opf", deflated).unwrap();
    zip.write_all(opf.as_bytes()).unwrap();

    zip.start_file("OEBPS/chapter.xhtml", deflated).unwrap();
    zip.write_all(
        br#"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>x</title></head>
<body><p>content</p></body></html>"#,
    )
    .unwrap();

    let mut out = zip.finish().unwrap();
    out.flush().unwrap();
    file
}

#[test]
fn extracts_full_record() {
    let path = write_epub(&Fixture {
        opf_metadata: r#"
      <dc:identifier id="pub-id">a9781784780609</dc:identifier>
      <dc:source>urn:isbn:9780063211841</dc:source>
      <dc:title xml:lang="en">Positive Obsession</dc:title>
      <dc:creator id="aut">Susana M. Morris</dc:creator>
      <meta refines="#aut" property="role" scheme="marc:relators">aut</meta>
      <meta refines="#aut" property="file-as">Morris, Susana M.</meta>
      <dc:contributor id="ctb">Alice Sheldon</dc:contributor>
      <meta refines="#ctb" property="role" scheme="marc:relators">author</meta>
      <dc:publisher>Amistad</dc:publisher>
      <dc:language>en</dc:language>
      <dc:date>2025-08-19</dc:date>
      <dc:description>&lt;p&gt;A biography of Octavia E. Butler.&lt;/p&gt;</dc:description>
      <dc:subject>BIOGRAPHY &amp; AUTOBIOGRAPHY / Cultural, Ethnic &amp; Regional</dc:subject>
      <dc:subject>Butler, Octavia E.</dc:subject>
    "#.to_string(),
        with_cover: true,
    })
    .path()
    .to_path_buf();

    let m = read_metadata(&path).unwrap();
    assert_eq!(m.title.0, "Positive Obsession");
    assert_eq!(m.creators.len(), 2);
    assert_eq!(m.creators[0].name, "Susana M. Morris");
    assert_eq!(m.creators[0].role, Role::Author);
    assert_eq!(m.creators[0].file_as.as_deref(), Some("Morris, Susana M."));
    assert_eq!(m.creators[1].name, "Alice Sheldon");

    // Sigil-prefixed identifier and urn:isbn source both resolve; deduped.
    assert_eq!(m.isbns.len(), 2);
    assert_eq!(m.isbns[0].as_str(), "9781784780609");
    assert_eq!(m.isbns[1].as_str(), "9780063211841");

    assert_eq!(m.publisher.as_deref().map(|p| p.0.as_str()), Some("Amistad"));
    assert_eq!(m.language.as_deref().map(|l| l.0.as_str()), Some("en"));
    let d = m.published.as_ref().unwrap();
    assert_eq!((d.year, d.month, d.day), (2025, Some(8), Some(19)));
    assert!(m.description.as_deref().unwrap().0.contains("Octavia E. Butler"));
    assert_eq!(m.subjects.len(), 2);
    let cover = m.cover.expect("cover extracted");
    assert_eq!(cover.mime, "image/png");
    assert!(cover.data.starts_with(b"\x89PNG"));
}

#[test]
fn converts_isbn10_and_strips_uppercase_urn() {
    let path = write_epub(&Fixture {
        opf_metadata: r#"
      <dc:identifier id="pub-id">URN:ISBN:0-306-40615-2</dc:identifier>
      <dc:title>X</dc:title>
      <dc:creator>Y</dc:creator>
    "#.to_string(),
        with_cover: false,
    })
    .path()
    .to_path_buf();
    let m = read_metadata(&path).unwrap();
    assert_eq!(m.isbns.len(), 1);
    assert_eq!(m.isbns[0].as_str(), "9780306406157");
}

#[test]
fn fails_without_isbn() {
    let path = write_epub(&Fixture {
        opf_metadata: r#"
      <dc:identifier id="pub-id">lorem-not-an-isbn</dc:identifier>
      <dc:title>Some Book</dc:title>
      <dc:creator>Someone</dc:creator>
    "#.to_string(),
        with_cover: false,
    })
    .path()
    .to_path_buf();
    assert!(matches!(read_metadata(&path), Err(EpubError::MissingIsbn)));
}

#[test]
fn fails_with_bad_checksum() {
    let path = write_epub(&Fixture {
        opf_metadata: r#"
      <dc:identifier id="pub-id">urn:isbn:9780063211842</dc:identifier>
      <dc:title>Some Book</dc:title>
      <dc:creator>Someone</dc:creator>
    "#.to_string(),
        with_cover: false,
    })
    .path()
    .to_path_buf();
    assert!(matches!(read_metadata(&path), Err(EpubError::MissingIsbn)));
}

#[test]
fn fails_without_title() {
    let path = write_epub(&Fixture {
        opf_metadata: r#"
      <dc:identifier id="pub-id">urn:isbn:9780063211841</dc:identifier>
      <dc:creator>Someone</dc:creator>
    "#.to_string(),
        with_cover: false,
    })
    .path()
    .to_path_buf();
    assert!(matches!(
        read_metadata(&path),
        Err(EpubError::MissingRequired("title"))
    ));
}

#[test]
fn fails_without_creator() {
    let path = write_epub(&Fixture {
        opf_metadata: r#"
      <dc:identifier id="pub-id">urn:isbn:9780063211841</dc:identifier>
      <dc:title>Some Book</dc:title>
    "#.to_string(),
        with_cover: false,
    })
    .path()
    .to_path_buf();
    assert!(matches!(
        read_metadata(&path),
        Err(EpubError::MissingRequired("creator"))
    ));
}

#[test]
fn non_isbn_identifier_preserved_as_other() {
    let path = write_epub(&Fixture {
        opf_metadata: r#"
      <dc:identifier id="pub-id">urn:uuid:45f50eae-2b3c-48c5</dc:identifier>
      <dc:identifier>2009033798</dc:identifier>
      <dc:title>Some Book</dc:title>
      <dc:creator>Someone</dc:creator>
      <dc:source>urn:isbn:9780063211841</dc:source>
    "#.to_string(),
        with_cover: false,
    })
    .path()
    .to_path_buf();
    let m = read_metadata(&path).unwrap();
    assert_eq!(m.isbns.len(), 1);
    assert_eq!(m.other_identifiers.len(), 2);
    assert!(m
        .other_identifiers
        .iter()
        .any(|i| i.0 == "urn:uuid:45f50eae-2b3c-48c5"));
}
