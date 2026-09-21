use std::io::Write;

use base64::{Engine, engine::general_purpose::STANDARD};
use livtet_importer::{EpubImporter, Importer};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

fn fixture_epub() -> tempfile::NamedTempFile {
    let file = tempfile::Builder::new().suffix(".epub").tempfile().unwrap();
    let mut zip = ZipWriter::new(file.reopen().unwrap());
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    zip.start_file("mimetype", stored).unwrap();
    zip.write_all(b"application/epub+zip").unwrap();
    zip.start_file("META-INF/container.xml", deflated).unwrap();
    zip.write_all(
        br##"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"##,
    )
    .unwrap();
    zip.start_file("OEBPS/cover.png", deflated).unwrap();
    zip.write_all(b"\x89PNG\r\n\x1a\nimporter-bytes").unwrap();
    zip.start_file("OEBPS/content.opf", deflated).unwrap();
    zip.write_all(
        br##"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="pub-id" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="pub-id">9781784780609</dc:identifier>
    <dc:identifier>publisher-123</dc:identifier>
    <dc:title>Positive Obsession</dc:title>
    <dc:creator id="aut">Susana M. Morris</dc:creator>
    <meta refines="#aut" property="role" scheme="marc:relators">aut</meta>
    <meta refines="#aut" property="file-as">Morris, Susana M.</meta>
    <dc:publisher>Amistad</dc:publisher>
    <dc:language>en</dc:language>
    <dc:date>2025-08-19</dc:date>
    <dc:description>A biography of Octavia E. Butler.</dc:description>
    <dc:subject>Biography</dc:subject>
  </metadata>
  <manifest>
    <item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/>
    <item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/>
  </manifest>
  <spine><itemref idref="chapter"/></spine>
</package>"##,
    )
    .unwrap();
    zip.start_file("OEBPS/chapter.xhtml", deflated).unwrap();
    zip.write_all(
        br##"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>x</title></head>
<body><p>content</p></body></html>"##,
    )
    .unwrap();

    let mut out = zip.finish().unwrap();
    out.flush().unwrap();
    file
}

#[test]
fn epub_importer_preserves_native_catalog_fields() {
    let epub = fixture_epub();
    let importer = EpubImporter;
    let record = importer
        .read_metadata(epub.path().to_string_lossy().into_owned())
        .expect("fixture EPUB has complete importer metadata");

    assert_eq!(importer.extensions().unwrap(), vec!["epub".to_string()]);
    assert_eq!(record.title, "Positive Obsession");
    assert_eq!(record.contributors.len(), 1);
    assert_eq!(record.contributors[0].name, "Susana M. Morris");
    assert_eq!(record.contributors[0].role.as_deref(), Some("aut"));
    assert_eq!(
        record.contributors[0].file_as.as_deref(),
        Some("Morris, Susana M.")
    );
    assert_eq!(record.isbns, vec!["9781784780609".to_string()]);
    assert_eq!(record.other_identifiers, vec!["publisher-123".to_string()]);
    assert_eq!(record.publisher.as_deref(), Some("Amistad"));
    assert_eq!(record.language.as_deref(), Some("en"));
    let published = record.published.expect("publication date is preserved");
    assert_eq!(
        (published.year, published.month, published.day),
        (2025, Some(8), Some(19))
    );
    assert_eq!(
        record.description.as_deref(),
        Some("A biography of Octavia E. Butler.")
    );
    assert_eq!(record.subjects, vec!["Biography".to_string()]);

    let cover = record.cover.expect("cover is preserved");
    assert_eq!(cover.mime, "image/png");
    assert_eq!(
        STANDARD.decode(&cover.data_base64).unwrap(),
        b"\x89PNG\r\n\x1a\nimporter-bytes"
    );
}
