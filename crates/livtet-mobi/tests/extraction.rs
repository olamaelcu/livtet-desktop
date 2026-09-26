//! Integration tests: build in-memory MOBI files and assert extraction.

#[path = "../src/test_support.rs"]
mod test_support;

use livtet_mobi::{MobiError, read_metadata};
use test_support::{MobiBuilder, tempfile_with};

const JPEG: &[u8] = b"\xFF\xD8\xFF\xE0fake-jpeg-bytes";
const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfake-png-bytes";
const GIF: &[u8] = b"GIF89afake-gif-bytes";
const FONT: &[u8] = b"FONTfake-font-bytes";
const TEXT: &[u8] = b"chapter text, compressed or not";

fn minimal() -> MobiBuilder {
    MobiBuilder::new()
        .exth(503, "The Title")
        .exth(100, "Jane Doe")
}

fn read(builder: &MobiBuilder) -> livtet_mobi::SourceMetadata {
    read_metadata(builder.tempfile().path()).expect("valid MOBI")
}

fn read_err(builder: &MobiBuilder) -> MobiError {
    read_metadata(builder.tempfile().path()).expect_err("must fail")
}

fn isbns(meta: &livtet_mobi::SourceMetadata) -> Vec<&str> {
    meta.isbns.iter().map(|isbn| isbn.as_str()).collect()
}

fn others(meta: &livtet_mobi::SourceMetadata) -> Vec<&str> {
    meta.other_identifiers
        .iter()
        .map(|id| id.0.as_str())
        .collect()
}

#[test]
fn full_kf8_record_extracts_everything() {
    let meta = read(
        &MobiBuilder::new()
            .header_length(264)
            .file_version(8)
            .codepage(65001)
            .exth(503, "The Great Book")
            .exth(100, "Doe, John")
            .exth(101, "Tor Books")
            .exth(103, "<p>A grand adventure.</p>")
            .exth(104, "9780061120084")
            .exth(105, "Fantasy; Adventure")
            .exth(106, "2021-05-17")
            .exth(524, "en-US")
            .exth(201, 0u32.to_be_bytes())
            .first_image_index(1)
            .push_record(JPEG),
    );
    assert_eq!(meta.title.0, "The Great Book");
    assert_eq!(meta.creators.len(), 1);
    assert_eq!(meta.creators[0].name, "John Doe");
    assert_eq!(meta.publisher.as_ref().unwrap().0, "Tor Books");
    assert_eq!(
        meta.description.as_ref().unwrap().0,
        "<p>A grand adventure.</p>"
    );
    assert_eq!(isbns(&meta), vec!["9780061120084"]);
    assert_eq!(meta.published.as_ref().unwrap().year, 2021);
    assert_eq!(meta.language.as_ref().unwrap().0, "en");
    assert_eq!(
        meta.subjects
            .iter()
            .map(|s| s.0.as_str())
            .collect::<Vec<_>>(),
        vec!["Fantasy", "Adventure"]
    );
    let cover = meta.cover.expect("cover");
    assert_eq!(cover.mime, "image/jpeg");
    assert_eq!(cover.data, JPEG);
}

#[test]
fn mobi6_record_with_slug_full_name_and_503_title() {
    let meta = read(
        &minimal()
            .file_version(6)
            .header_length(232)
            .full_name("the-great-book-asin-B0123"),
    );
    assert_eq!(meta.title.0, "The Title");
}

#[test]
fn exth_503_title_wins_over_full_name() {
    let meta = read(&minimal().full_name("Fallback Slug"));
    assert_eq!(meta.title.0, "The Title");
}

#[test]
fn full_name_fallback_decodes_entities() {
    let meta = read(
        &MobiBuilder::new()
            .exth(100, "Jane Doe")
            .full_name("Fish &#38; Chips"),
    );
    assert_eq!(meta.title.0, "Fish & Chips");
}

#[test]
fn last_first_author_sets_file_as() {
    let meta = read(&MobiBuilder::new().exth(503, "T").exth(100, "Doe, John"));
    assert_eq!(meta.creators.len(), 1);
    assert_eq!(meta.creators[0].name, "John Doe");
    assert_eq!(meta.creators[0].file_as.as_deref(), Some("Doe, John"));
    assert_eq!(meta.creators[0].role, livtet_mobi::Role::Author);
}

#[test]
fn author_without_comma_stays_verbatim() {
    let meta = read(&MobiBuilder::new().exth(503, "T").exth(100, "Plato"));
    assert_eq!(meta.creators[0].name, "Plato");
    assert_eq!(meta.creators[0].file_as, None);
}

#[test]
fn parenthesised_author_without_comma_stays_verbatim() {
    let meta = read(
        &MobiBuilder::new()
            .exth(503, "T")
            .exth(100, "John (Jack) Smith"),
    );
    assert_eq!(meta.creators[0].name, "John (Jack) Smith");
    assert_eq!(meta.creators[0].file_as, None);
}

#[test]
fn accented_last_first_author_preserved_verbatim() {
    let meta = read(
        &MobiBuilder::new()
            .exth(503, "T")
            .exth(100, "García Márquez, Gabriel"),
    );
    assert_eq!(meta.creators[0].name, "Gabriel García Márquez");
    assert_eq!(
        meta.creators[0].file_as.as_deref(),
        Some("García Márquez, Gabriel")
    );
}

#[test]
fn author_with_two_commas_stays_verbatim() {
    let meta = read(
        &MobiBuilder::new()
            .exth(503, "T")
            .exth(100, "Smith, John, Jr."),
    );
    assert_eq!(meta.creators[0].name, "Smith, John, Jr.");
    assert_eq!(meta.creators[0].file_as, None);
}

#[test]
fn repeated_exth_100_authors_accumulate_in_order() {
    let meta = read(
        &MobiBuilder::new()
            .exth(503, "T")
            .exth(100, "B Author")
            .exth(100, "A Author"),
    );
    let names: Vec<&str> = meta.creators.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["B Author", "A Author"]);
}

#[test]
fn and_joined_author_splits_with_per_name_file_as() {
    let meta = read(
        &MobiBuilder::new()
            .exth(503, "T")
            .exth(100, "Doe, John and Smith, Jane"),
    );
    assert_eq!(meta.creators.len(), 2);
    assert_eq!(meta.creators[0].name, "John Doe");
    assert_eq!(meta.creators[0].file_as.as_deref(), Some("Doe, John"));
    assert_eq!(meta.creators[1].name, "Jane Smith");
    assert_eq!(meta.creators[1].file_as.as_deref(), Some("Smith, Jane"));
}

#[test]
fn semicolon_joined_author_splits() {
    let meta = read(
        &MobiBuilder::new()
            .exth(503, "T")
            .exth(100, "Plato; Aristotle"),
    );
    let names: Vec<&str> = meta.creators.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["Plato", "Aristotle"]);
}

#[test]
fn and_inside_author_word_does_not_split() {
    let meta = read(
        &MobiBuilder::new()
            .exth(503, "T")
            .exth(100, "Alexander Anderson"),
    );
    assert_eq!(meta.creators.len(), 1);
    assert_eq!(meta.creators[0].name, "Alexander Anderson");
}

#[test]
fn exth_108_contributor_is_entity_decoded_ctb() {
    let meta = read(&minimal().exth(108, "Edited by John &amp; Jane"));
    assert_eq!(meta.creators.len(), 2);
    let ctb = &meta.creators[1];
    assert_eq!(ctb.name, "Edited by John & Jane");
    assert_eq!(ctb.role, livtet_mobi::Role::Other("ctb".to_string()));
    assert_eq!(ctb.file_as, None);
}

#[test]
fn blank_publisher_is_none() {
    let meta = read(&minimal().exth(101, "   "));
    assert_eq!(meta.publisher, None);
}

#[test]
fn unknown_publisher_is_none() {
    let meta = read(&minimal().exth(101, "Unknown"));
    assert_eq!(meta.publisher, None);
}

#[test]
fn bare_isbn_goes_to_isbns() {
    let meta = read(&minimal().exth(104, "9780061120084"));
    assert_eq!(isbns(&meta), vec!["9780061120084"]);
    assert!(others(&meta).is_empty());
}

#[test]
fn dashed_isbn_is_canonicalized() {
    let meta = read(&minimal().exth(104, "978-0-06-112008-4"));
    assert_eq!(isbns(&meta), vec!["9780061120084"]);
}

#[test]
fn urn_isbn_source_id_goes_to_isbns() {
    let meta = read(&minimal().exth(112, "urn:isbn:9780061120084"));
    assert_eq!(isbns(&meta), vec!["9780061120084"]);
    assert!(others(&meta).is_empty());
}

#[test]
fn urn_isbn_prefix_is_case_insensitive() {
    let meta = read(&minimal().exth(112, "URN:ISBN:9780061120084"));
    assert_eq!(isbns(&meta), vec!["9780061120084"]);
}

#[test]
fn ebook_isbn_source_id_goes_to_isbns() {
    let meta = read(&minimal().exth(112, "eBook ISBN: 0-306-40615-2"));
    assert_eq!(isbns(&meta), vec!["9780306406157"]);
}

#[test]
fn invalid_isbn_falls_back_to_other_identifiers() {
    let meta = read(&minimal().exth(104, "not-an-isbn"));
    assert!(isbns(&meta).is_empty());
    assert_eq!(others(&meta), vec!["not-an-isbn"]);
}

#[test]
fn calibre_source_id_is_ignored() {
    let meta = read(&minimal().exth(112, "calibre:6a7b8c9d"));
    assert!(isbns(&meta).is_empty());
    assert!(others(&meta).is_empty());
}

#[test]
fn plain_source_id_is_preserved_verbatim() {
    let meta = read(&minimal().exth(112, "some-publisher-id"));
    assert_eq!(others(&meta), vec!["some-publisher-id"]);
}

#[test]
fn asin_identifier_is_preserved() {
    let meta = read(&minimal().exth(113, "B012345678"));
    assert_eq!(others(&meta), vec!["B012345678"]);
}

#[test]
fn semicolon_subjects_are_split_deduped_in_order() {
    let meta = read(&minimal().exth(105, "Fantasy; Science Fiction;Fantasy ; ;Epic"));
    let subjects: Vec<&str> = meta.subjects.iter().map(|s| s.0.as_str()).collect();
    assert_eq!(subjects, vec!["Fantasy", "Science Fiction", "Epic"]);
}

#[test]
fn iso_publication_date_parses() {
    let meta = read(&minimal().exth(106, "2021-05-17"));
    let published = meta.published.expect("date");
    assert_eq!(published.year, 2021);
    assert_eq!(published.month, Some(5));
    assert_eq!(published.day, Some(17));
}

#[test]
fn unparsable_publication_date_is_none() {
    let meta = read(&minimal().exth(106, "not-a-date"));
    assert_eq!(meta.published, None);
}

#[test]
fn uppercase_language_subtag_is_lowercased() {
    let meta = read(&minimal().exth(524, "En"));
    assert_eq!(meta.language.unwrap().0, "en");
}

#[test]
fn language_region_subtag_is_stripped() {
    let meta = read(&minimal().exth(524, "en-US"));
    assert_eq!(meta.language.unwrap().0, "en");
}

#[test]
fn locale_english_falls_back_to_en() {
    let meta = read(&minimal().locale(1033));
    assert_eq!(meta.language.unwrap().0, "en");
}

#[test]
fn locale_9_falls_back_to_en() {
    let meta = read(&minimal().locale(9));
    assert_eq!(meta.language.unwrap().0, "en");
}

#[test]
fn non_english_locale_without_524_is_none() {
    let meta = read(&minimal().locale(1031));
    assert_eq!(meta.language, None);
}

#[test]
fn cover_via_201_offset_zero() {
    let meta = read(
        &minimal()
            .exth(201, 0u32.to_be_bytes())
            .first_image_index(1)
            .push_record(JPEG),
    );
    let cover = meta.cover.expect("cover");
    assert_eq!(cover.mime, "image/jpeg");
    assert_eq!(cover.data, JPEG);
}

#[test]
fn cover_via_201_with_nonzero_offset() {
    let meta = read(
        &minimal()
            .exth(201, 1u32.to_be_bytes())
            .first_image_index(1)
            .push_record(TEXT)
            .push_record(JPEG),
    );
    let cover = meta.cover.expect("cover");
    assert_eq!(cover.mime, "image/jpeg");
    assert_eq!(cover.data, JPEG);
}

#[test]
fn cover_falls_back_to_202_thumbnail_offset() {
    let meta = read(
        &minimal()
            .exth(202, 1u32.to_be_bytes())
            .first_image_index(1)
            .push_record(TEXT)
            .push_record(PNG),
    );
    let cover = meta.cover.expect("cover");
    assert_eq!(cover.mime, "image/png");
    assert_eq!(cover.data, PNG);
}

#[test]
fn cover_falls_back_to_first_image_scan() {
    let meta = read(
        &minimal()
            .first_image_index(1)
            .push_record(TEXT)
            .push_record(PNG),
    );
    let cover = meta.cover.expect("cover");
    assert_eq!(cover.mime, "image/png");
    assert_eq!(cover.data, PNG);
}

#[test]
fn non_image_record_is_skipped_during_scan() {
    let meta = read(
        &minimal()
            .first_image_index(1)
            .push_record(FONT)
            .push_record(GIF),
    );
    let cover = meta.cover.expect("cover");
    assert_eq!(cover.mime, "image/gif");
    assert_eq!(cover.data, GIF);
}

#[test]
fn no_images_means_no_cover() {
    let meta = read(&minimal().push_record(TEXT));
    assert_eq!(meta.cover, None);
}

#[test]
fn drm_file_still_yields_metadata_and_cover() {
    let meta = read(
        &minimal()
            .encryption(2)
            .exth(201, 0u32.to_be_bytes())
            .first_image_index(1)
            .push_record(JPEG),
    );
    assert_eq!(meta.title.0, "The Title");
    assert_eq!(meta.creators[0].name, "Jane Doe");
    assert_eq!(meta.cover.expect("cover").mime, "image/jpeg");
}

#[test]
fn kfx_container_is_rejected() {
    let mut bytes = vec![0xEA, 0x44, 0x52, 0x4D, 0x49, 0x4F, 0x4E, 0xEE];
    bytes.extend_from_slice(&[0u8; 128]);
    let file = tempfile_with(&bytes);
    let err = read_metadata(file.path()).expect_err("KFX must be rejected");
    assert!(matches!(err, MobiError::UnsupportedContainer("kfx")));
}

#[test]
fn topaz_container_is_rejected() {
    let mut bytes = b"TPZ".to_vec();
    bytes.extend_from_slice(&[0u8; 128]);
    let file = tempfile_with(&bytes);
    let err = read_metadata(file.path()).expect_err("Topaz must be rejected");
    assert!(matches!(err, MobiError::UnsupportedContainer("topaz")));
}

#[test]
fn non_mobi_type_creator_is_rejected() {
    let builder = minimal().pdb_kind(*b"DATA", *b"XXXX");
    assert!(matches!(read_err(&builder), MobiError::NotMobi));
}

#[test]
fn textread_container_is_accepted() {
    let meta = read(&minimal().pdb_kind(*b"TEXT", *b"READ"));
    assert_eq!(meta.title.0, "The Title");
}

#[test]
fn short_record0_is_malformed_not_panic() {
    // PDB header (78) + 1 entry (8) = 86; record 0 gets only 50 bytes.
    let builder = minimal().truncate(86 + 50);
    assert!(matches!(read_err(&builder), MobiError::Malformed(_)));
}

#[test]
fn truncated_record_table_is_malformed_not_panic() {
    let builder = minimal().truncate(80);
    assert!(matches!(read_err(&builder), MobiError::Malformed(_)));
}

#[test]
fn empty_file_is_malformed_not_panic() {
    let file = tempfile_with(&[]);
    let err = read_metadata(file.path()).expect_err("empty must fail");
    assert!(matches!(err, MobiError::Malformed(_)));
}

#[test]
fn padded_exth_length_and_wide_flags_are_tolerated() {
    let meta = read(
        &minimal()
            .exth_flags(0x1050)
            .exth_len_padding(32)
            .exth(101, "Padded Press"),
    );
    assert_eq!(meta.title.0, "The Title");
    assert_eq!(meta.publisher.unwrap().0, "Padded Press");
}

#[test]
fn missing_exth_flags_bit_ignores_exth_block() {
    // EXTH bytes are present but flags lack 0x40: `full_name` supplies the
    // title while the EXTH authors are ignored, leaving no creators.
    let builder = MobiBuilder::new()
        .exth_flags(0x00)
        .exth(503, "Ignored Title")
        .exth(100, "Ignored Author")
        .full_name("Real Title");
    assert!(matches!(
        read_err(&builder),
        MobiError::MissingRequired("creator")
    ));
}

#[test]
fn malformed_exth_item_is_skipped_later_items_survive() {
    let meta = read(
        &minimal()
            .exth(101, "Skipped Press")
            .exth(103, "<p>Kept description.</p>")
            .corrupt_exth_item(2),
    );
    assert_eq!(meta.publisher, None);
    assert_eq!(meta.description.unwrap().0, "<p>Kept description.</p>");
}

#[test]
fn exth_121_without_boundary_parses_normally() {
    let meta = read(&minimal().exth(121, "not-a-boundary-value"));
    assert_eq!(meta.title.0, "The Title");
    assert_eq!(meta.creators[0].name, "Jane Doe");
}

#[test]
fn missing_title_is_an_error() {
    let builder = MobiBuilder::new().exth(100, "Jane Doe");
    assert!(matches!(
        read_err(&builder),
        MobiError::MissingRequired("title")
    ));
}

#[test]
fn missing_creator_is_an_error() {
    let builder = MobiBuilder::new().exth(503, "The Title");
    assert!(matches!(
        read_err(&builder),
        MobiError::MissingRequired("creator")
    ));
}

#[test]
fn cp1252_title_with_80_to_9f_bytes() {
    // 0x93/0x94 = smart quotes, 0xE9 = é in Windows-1252.
    let meta = read(
        &MobiBuilder::new()
            .codepage(1252)
            .exth(503, b"\x93Caf\xE9\x94".to_vec())
            .exth(100, "Jane Doe"),
    );
    assert_eq!(meta.title.0, "\u{201C}Caf\u{00E9}\u{201D}");
}

#[test]
fn pdb_nrec_not_palmdoc_count_owns_slicing() {
    // PalmDOC claims 7 records; the PDB table really holds 3 (record 0 +
    // text + cover). The cover must still resolve.
    let meta = read(
        &minimal()
            .palmdoc_record_count(7)
            .exth(201, 1u32.to_be_bytes())
            .first_image_index(1)
            .push_record(TEXT)
            .push_record(JPEG),
    );
    let cover = meta.cover.expect("cover via PDB nrec");
    assert_eq!(cover.mime, "image/jpeg");
}

#[test]
fn unreadable_path_is_an_io_error() {
    let err = read_metadata(std::path::Path::new("/nonexistent-dir/missing.mobi"))
        .expect_err("must fail");
    assert!(matches!(err, MobiError::Io(_)));
}
