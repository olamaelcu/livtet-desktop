//! Integration tests: build in-memory EPUBs and assert extraction.

#[path = "../src/test_support.rs"]
mod test_support;

use std::path::Path;

use livtet_epub::{Contributor, Cover, EpubError, EpubMetadata, Role, read_metadata};
use test_support::{EpubBuilder, package, package3};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfake-png-bytes";
const JPEG: &[u8] = b"\xFF\xD8\xFF\xE0fake-jpeg-bytes";

type ErrorCheck = fn(&EpubError) -> bool;

fn meta_for(metadata_body: &str) -> EpubMetadata {
    try_body(metadata_body).expect("valid EPUB")
}

fn try_body(metadata_body: &str) -> livtet_epub::Result<EpubMetadata> {
    read_metadata(
        EpubBuilder::new(package3(metadata_body, "", ""))
            .tempfile()
            .path(),
    )
}

fn assert_roles(creators: &[Contributor], expected: &[Role]) {
    let roles: Vec<&Role> = creators.iter().map(|c| &c.role).collect();
    assert_eq!(roles, expected.iter().collect::<Vec<_>>());
}

fn cover_for(metadata_body: &str, manifest_extra: &str, guide: &str, file: &[u8]) -> Option<Cover> {
    let epub = EpubBuilder::new(package3(metadata_body, manifest_extra, guide))
        .file("OEBPS/cover.jpg", file.to_vec())
        .file("OEBPS/cover.png", file.to_vec());
    read_metadata(epub.tempfile().path()).unwrap().cover
}

#[test]
fn extracts_full_record_mirroring_real_book() {
    let metadata = r##"
    <dc:identifier id="bookid">9780063211841</dc:identifier>
    <dc:source id="isbn">urn:isbn:9780063212077</dc:source>
    <dc:title id="en_title" xml:lang="en">Positive Obsession</dc:title>
    <meta refines="#en_title" property="title-type">main</meta>
    <dc:creator id="creator01">Susana M. Morris</dc:creator>
    <meta refines="#creator01" property="role" scheme="marc:relators">aut</meta>
    <meta refines="#creator01" property="file-as">Morris, Susana M.</meta>
    <dc:contributor>Susana M. Morris</dc:contributor>
    <dc:publisher>HarperCollins</dc:publisher>
    <dc:language>en</dc:language>
    <dc:date>2025</dc:date>
    <dc:description>&lt;p&gt;A biography of Octavia E. Butler.&lt;/p&gt;</dc:description>
    <dc:subject>BIOGRAPHY, Cultural</dc:subject>
    "##;
    let manifest = r#"<item id="cover-image" href="cover.jpg" media-type="image/jpeg" properties="cover-image"/>"#;

    let epub =
        EpubBuilder::new(package3(metadata, manifest, "")).file("OEBPS/cover.jpg", JPEG.to_vec());
    let m = read_metadata(epub.tempfile().path()).unwrap();

    assert_eq!(m.title.0, "Positive Obsession");
    assert_eq!(m.title_sort, None);
    assert_eq!(m.creators.len(), 1);
    assert_eq!(m.creators[0].name, "Susana M. Morris");
    assert_eq!(m.creators[0].role, Role::Author);
    assert_eq!(m.creators[0].file_as.as_deref(), Some("Morris, Susana M."));
    assert_eq!(m.isbns.len(), 2);
    assert_eq!(m.isbns[0].as_str(), "9780063211841");
    assert_eq!(m.isbns[1].as_str(), "9780063212077");
    assert_eq!(
        m.publisher.as_ref().map(|p| p.0.as_str()),
        Some("HarperCollins")
    );
    assert_eq!(m.language.as_ref().map(|l| l.0.as_str()), Some("en"));
    let date = m.published.as_ref().unwrap();
    assert_eq!((date.year, date.month, date.day), (2025, None, None));
    assert!(
        m.description
            .as_ref()
            .unwrap()
            .0
            .contains("Octavia E. Butler")
    );
    assert_eq!(
        m.subjects.iter().map(|s| s.0.as_str()).collect::<Vec<_>>(),
        vec!["BIOGRAPHY", "Cultural"]
    );
    let cover = m.cover.expect("cover extracted");
    assert_eq!(cover.mime, "image/jpeg");
    assert_eq!(cover.data, JPEG);
}

#[test]
fn subjects_are_deduped_preserving_first_seen_order() {
    let repeated = meta_for(
        r##"
    <dc:identifier id="bookid">9780063211841</dc:identifier>
    <dc:title>X</dc:title>
    <dc:creator>Y</dc:creator>
    <dc:subject>Fiction, Fiction</dc:subject>
    "##,
    );
    let subjects: Vec<&str> = repeated.subjects.iter().map(|s| s.0.as_str()).collect();
    assert_eq!(subjects, vec!["Fiction"]);

    let split = meta_for(
        r##"
    <dc:identifier id="bookid">9780063211841</dc:identifier>
    <dc:title>X</dc:title>
    <dc:creator>Y</dc:creator>
    <dc:subject>Fiction</dc:subject>
    <dc:subject>Fiction, General</dc:subject>
    "##,
    );
    let subjects: Vec<&str> = split.subjects.iter().map(|s| s.0.as_str()).collect();
    assert_eq!(subjects, vec!["Fiction", "General"]);
}

#[test]
fn role_less_contributor_duplicating_creator_is_dropped() {
    let m = meta_for(
        r##"
    <dc:identifier id="bookid">9780063211841</dc:identifier>
    <dc:title>Positive Obsession</dc:title>
    <dc:creator id="creator01">Susana M. Morris</dc:creator>
    <meta refines="#creator01" property="role">aut</meta>
    <meta refines="#creator01" property="file-as">Morris, Susana M.</meta>
    <dc:contributor>Susana M. Morris</dc:contributor>
    "##,
    );
    assert_eq!(m.creators.len(), 1);
    assert_eq!(m.creators[0].role, Role::Author);
    assert_eq!(m.creators[0].file_as.as_deref(), Some("Morris, Susana M."));
}

#[test]
fn role_less_contributor_with_new_name_is_kept_as_ctb() {
    let m = meta_for(
        r##"
    <dc:identifier id="bookid">9780063211841</dc:identifier>
    <dc:title>Some Book</dc:title>
    <dc:creator>Susana M. Morris</dc:creator>
    <dc:contributor>Alice Sheldon</dc:contributor>
    "##,
    );
    assert_eq!(m.creators.len(), 2);
    assert_eq!(m.creators[1].name, "Alice Sheldon");
    assert_eq!(m.creators[1].role, Role::Other("ctb".to_string()));
}

#[test]
fn distinct_roles_for_same_name_are_both_kept() {
    let m = meta_for(
        r##"
    <dc:identifier id="bookid">9780063211841</dc:identifier>
    <dc:title>Some Book</dc:title>
    <dc:creator id="a">Susana M. Morris</dc:creator>
    <meta refines="#a" property="role">aut</meta>
    <dc:contributor id="b">Susana M. Morris</dc:contributor>
    <meta refines="#b" property="role">ill</meta>
    "##,
    );
    assert_roles(&m.creators, &[Role::Author, Role::Illustrator]);
}

#[test]
fn appends_subtitle_to_main_title() {
    let m = meta_for(
        r##"
    <dc:identifier id="bookid">9780063211841</dc:identifier>
    <dc:title id="t1">Main Title</dc:title>
    <meta refines="#t1" property="title-type">main</meta>
    <dc:title id="t2">A Subtitle</dc:title>
    <meta refines="#t2" property="title-type">subtitle</meta>
    <dc:creator>Someone</dc:creator>
    "##,
    );
    assert_eq!(m.title.0, "Main Title: A Subtitle");
}

#[test]
fn title_sort_prefers_title_file_as_then_calibre_meta() {
    let cases: &[(&str, &str)] = &[
        (
            "main title file-as",
            r##"
    <dc:identifier id="bookid">9780063211841</dc:identifier>
    <dc:title id="t1">The Book</dc:title>
    <meta refines="#t1" property="title-type">main</meta>
    <meta refines="#t1" property="file-as">Book, The</meta>
    <dc:creator>Someone</dc:creator>
    "##,
        ),
        (
            "calibre title_sort fallback",
            r##"
    <dc:identifier id="bookid">9780063211841</dc:identifier>
    <dc:title>The Book</dc:title>
    <meta name="calibre:title_sort" content="Book, The"/>
    <dc:creator>Someone</dc:creator>
    "##,
        ),
    ];

    for (name, body) in cases {
        let m = meta_for(body);
        assert_eq!(m.title_sort.as_deref(), Some("Book, The"), "{name}");
    }
}

#[test]
fn reads_bare_and_opf_role_attributes() {
    let metadata = r##"
    <dc:identifier id="bookid">9780063211841</dc:identifier>
    <dc:title>Some Book</dc:title>
    <dc:creator opf:role="aut">Author One</dc:creator>
    <dc:creator role="ILL">Illustrator Two</dc:creator>
    "##;
    let opf = package("2.0", metadata, "", "");
    let m = read_metadata(EpubBuilder::new(opf).tempfile().path()).unwrap();
    assert_roles(&m.creators, &[Role::Author, Role::Illustrator]);
}

#[test]
fn upgrades_isbn10_and_strips_uppercase_urn() {
    let m = meta_for(
        r##"
    <dc:identifier id="bookid">URN:ISBN:0-306-40615-2</dc:identifier>
    <dc:title>X</dc:title>
    <dc:creator>Y</dc:creator>
    "##,
    );
    assert_eq!(m.isbns.len(), 1);
    assert_eq!(m.isbns[0].as_str(), "9780306406157");
}

#[test]
fn recovers_sigil_prefixed_identifier() {
    let m = meta_for(
        r##"
    <dc:identifier id="bookid">a9781784780609</dc:identifier>
    <dc:title>X</dc:title>
    <dc:creator>Y</dc:creator>
    "##,
    );
    assert_eq!(m.isbns.len(), 1);
    assert_eq!(m.isbns[0].as_str(), "9781784780609");
}

#[test]
fn preserves_non_isbn_identifiers() {
    let m = meta_for(
        r##"
    <dc:identifier id="bookid">urn:uuid:45f50eae-2b3c-48c5</dc:identifier>
    <dc:identifier>example.com/books/1</dc:identifier>
    <dc:title>X</dc:title>
    <dc:creator>Y</dc:creator>
    <dc:source>urn:isbn:9780063211841</dc:source>
    "##,
    );
    assert_eq!(m.isbns.len(), 1);
    assert_eq!(m.isbns[0].as_str(), "9780063211841");
    let others: Vec<&str> = m.other_identifiers.iter().map(|i| i.0.as_str()).collect();
    assert_eq!(
        others,
        vec!["urn:uuid:45f50eae-2b3c-48c5", "example.com/books/1"]
    );
}

#[test]
fn read_metadata_fails_closed() {
    let cases: &[(&str, &str, ErrorCheck)] = &[
        (
            "no isbn",
            r##"<dc:identifier id="bookid">not-an-isbn</dc:identifier>
                <dc:title>T</dc:title><dc:creator>C</dc:creator>"##,
            |error| matches!(error, EpubError::MissingIsbn),
        ),
        (
            "bad isbn checksum",
            r##"<dc:identifier id="bookid">urn:isbn:9780063211842</dc:identifier>
                <dc:title>T</dc:title><dc:creator>C</dc:creator>"##,
            |error| matches!(error, EpubError::MissingIsbn),
        ),
        (
            "no title",
            r##"<dc:identifier id="bookid">urn:isbn:9780063211841</dc:identifier>
                <dc:creator>C</dc:creator>"##,
            |error| matches!(error, EpubError::MissingRequired("title")),
        ),
        (
            "no creator",
            r##"<dc:identifier id="bookid">urn:isbn:9780063211841</dc:identifier>
                <dc:title>T</dc:title>"##,
            |error| matches!(error, EpubError::MissingRequired("creator")),
        ),
    ];

    for (name, metadata, expected) in cases {
        let error = try_body(metadata).expect_err(name);
        assert!(expected(&error), "case {name} produced {error:?}");
    }
}

#[test]
fn resolves_cover_from_epub3_cover_image_property() {
    let cover = cover_for(
        minimal_metadata(),
        r#"<item id="cover-image" href="cover.jpg" media-type="image/jpeg" properties="cover-image"/>"#,
        "",
        JPEG,
    )
    .unwrap();
    assert_eq!(cover.mime, "image/jpeg");
    assert_eq!(cover.data, JPEG);
}

#[test]
fn resolves_cover_from_meta_and_guide_references() {
    let cases: &[(&str, &str, &str)] = &[
        (
            "meta cover as manifest id",
            r#"<item id="coverimg" href="cover.jpg" media-type="image/jpeg"/>
           <meta name="cover" content="coverimg"/>"#,
            "",
        ),
        (
            "meta cover as href",
            r#"<item id="coverimg" href="cover.jpg" media-type="image/jpeg"/>
           <meta name="cover" content="cover.jpg"/>"#,
            "",
        ),
        (
            "guide reference",
            r#"<item id="coverimg" href="cover.jpg" media-type="image/jpeg"/>"#,
            r#"<guide><reference type="cover" href="cover.jpg"/></guide>"#,
        ),
    ];

    for (name, manifest_extra, guide) in cases {
        let cover = cover_for(minimal_metadata(), manifest_extra, guide, JPEG)
            .unwrap_or_else(|| panic!("{name} should resolve a cover"));
        assert_eq!(cover.mime, "image/jpeg", "{name}");
    }
}

#[test]
fn rejects_cover_with_xhtml_media_type() {
    let cover = cover_for(
        minimal_metadata(),
        r#"<item id="cover-page" href="cover.png" media-type="application/xhtml+xml" properties="cover-image"/>"#,
        "",
        PNG,
    );
    assert!(cover.is_none());
}

#[test]
fn skips_really_encrypted_cover() {
    assert!(encrypted_cover(encryption_for(AES128_CBC, "OEBPS/cover.jpg")).is_none());
}

#[test]
fn allows_idpf_obfuscated_cover() {
    let cover = encrypted_cover(encryption_for(IDPF_EMBEDDING, "OEBPS/cover.jpg"))
        .expect("idpf obfuscation is not real encryption");
    assert_eq!(cover.mime, "image/jpeg");
}

#[test]
fn non_ascii_identifiers_do_not_panic() {
    let m = meta_for(
        r##"
    <dc:identifier id="bookid">9780063211841</dc:identifier>
    <dc:identifier>€x</dc:identifier>
    <dc:identifier>日本語</dc:identifier>
    <dc:identifier>€é</dc:identifier>
    <dc:title>X</dc:title>
    <dc:creator>Y</dc:creator>
    "##,
    );
    let others: Vec<&str> = m.other_identifiers.iter().map(|i| i.0.as_str()).collect();
    assert_eq!(others, vec!["€x", "日本語", "€é"]);
}

#[test]
fn malformed_encryption_xml_skips_cover_but_keeps_metadata() {
    let m = metadata_with_encryption("this is not xml at all");
    assert_eq!(m.title.0, "Some Book");
    assert_eq!(m.isbns.len(), 1);
    assert!(m.cover.is_none());
}

#[test]
fn skips_cover_referenced_by_dot_slash_encryption_uri() {
    assert!(cover_with_encryption_uri("./OEBPS/cover.jpg").is_none());
}

#[test]
fn skips_cover_referenced_by_percent_encoded_encryption_uri() {
    assert!(cover_with_encryption_uri("OEBPS/cover%2Ejpg").is_none());
}

#[test]
fn deeply_nested_opf_fails_without_crashing() {
    let mut opf = String::from("<?xml version=\"1.0\"?><package>");
    for _ in 0..100_000 {
        opf.push_str("<a>");
    }
    let error = read_metadata(EpubBuilder::new(opf).tempfile().path()).unwrap_err();
    assert!(matches!(error, EpubError::Xml(_)), "got {error:?}");
}

#[test]
fn reads_utf16_opf() {
    let epub = EpubBuilder::new(package3(realish_metadata(), "", "")).opf_utf16();
    let m = read_metadata(epub.tempfile().path()).unwrap();
    assert_eq!(m.title.0, "Positive Obsession");
    assert_eq!(m.creators[0].name, "Susana M. Morris");
}

#[test]
fn tolerates_stray_ampersand_in_opf() {
    let m = meta_for(
        r##"
    <dc:identifier id="bookid">urn:isbn:9780063211841</dc:identifier>
    <dc:title>Tom & Jerry</dc:title>
    <dc:creator>Mutts & Co.</dc:creator>
    "##,
    );
    assert_eq!(m.title.0, "Tom & Jerry");
    assert_eq!(m.creators[0].name, "Mutts & Co.");
}

#[test]
fn reads_archive_with_stripped_central_directory() {
    let epub = EpubBuilder::new(package3(realish_metadata(), "", "")).strip_central_directory();
    let m = read_metadata(epub.tempfile().path()).unwrap();
    assert_eq!(m.isbns.len(), 2);
    assert_eq!(m.creators.len(), 1);
}

#[test]
#[ignore = "requires LIVTET_REAL_EPUB pointing at a real EPUB file"]
fn reads_real_positive_obsession_when_available() {
    let Some(path) = std::env::var_os("LIVTET_REAL_EPUB") else {
        return;
    };
    let m = read_metadata(Path::new(&path)).unwrap();
    assert_eq!(m.creators.len(), 1);
    assert_eq!(m.creators[0].name, "Susana M. Morris");
    assert_eq!(m.creators[0].role, Role::Author);
    assert_eq!(m.creators[0].file_as.as_deref(), Some("Morris, Susana M."));
    let isbns: Vec<&str> = m.isbns.iter().map(|i| i.as_str()).collect();
    assert_eq!(isbns, vec!["9780063211841", "9780063212077"]);
}

/// Real-cipher algorithm URI; a cover encrypted with it must be skipped.
const AES128_CBC: &str = "http://www.w3.org/2001/04/xmlenc#aes128-cbc";
/// IDPF obfuscation URI; it is not real encryption, so the cover is kept.
const IDPF_EMBEDDING: &str = "http://www.idpf.org/2008/embedding";

/// The EPUB3 cover item shared by the encryption fixtures below.
const EPUB3_JPEG_COVER_ITEM: &str =
    r#"<item id="cover-image" href="cover.jpg" media-type="image/jpeg" properties="cover-image"/>"#;

/// An `encryption.xml` declaring `uri` as encrypted with `algorithm`.
fn encryption_for(algorithm: &str, uri: &str) -> String {
    format!(
        r#"<encryption xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <EncryptedData xmlns="http://www.w3.org/2001/04/xmlenc#">
    <EncryptionMethod Algorithm="{algorithm}"/>
    <CipherData><CipherReference URI="{uri}"/></CipherData>
  </EncryptedData>
</encryption>"#
    )
}

/// A minimal EPUB whose shared EPUB3 JPEG cover item carries `encryption`.
fn metadata_with_encryption(encryption: impl Into<String>) -> EpubMetadata {
    let epub = EpubBuilder::new(package3(minimal_metadata(), EPUB3_JPEG_COVER_ITEM, ""))
        .file("OEBPS/cover.jpg", JPEG.to_vec())
        .encryption(encryption);
    read_metadata(epub.tempfile().path()).unwrap()
}

/// The cover produced by [`metadata_with_encryption`].
fn encrypted_cover(encryption: impl Into<String>) -> Option<Cover> {
    metadata_with_encryption(encryption).cover
}

/// A JPEG cover item guarded by a real-cipher encryption entry for `uri`.
fn cover_with_encryption_uri(uri: &str) -> Option<Cover> {
    encrypted_cover(encryption_for(AES128_CBC, uri))
}

fn minimal_metadata() -> &'static str {
    r##"<dc:identifier id="bookid">9780063211841</dc:identifier>
    <dc:title>Some Book</dc:title>
    <dc:creator>Someone</dc:creator>"##
}

fn realish_metadata() -> &'static str {
    r##"<dc:identifier id="bookid">9780063211841</dc:identifier>
    <dc:source>urn:isbn:9780063212077</dc:source>
    <dc:title id="t">Positive Obsession</dc:title>
    <meta refines="#t" property="title-type">main</meta>
    <dc:creator id="c">Susana M. Morris</dc:creator>
    <meta refines="#c" property="role">aut</meta>
    <meta refines="#c" property="file-as">Morris, Susana M.</meta>
    <dc:contributor>Susana M. Morris</dc:contributor>"##
}
