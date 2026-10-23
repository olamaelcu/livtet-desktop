#![allow(clippy::unwrap_used, clippy::expect_used)]

use livtet_importer::{AudiobookImporter, Importer, ImporterContributor, ImporterPublicationDate};

#[test]
fn audiobook_importer_claims_m4b_and_m4a() {
    let extensions = AudiobookImporter
        .extensions()
        .expect("extensions are known");

    assert_eq!(extensions, vec!["m4b".to_string(), "m4a".to_string()]);
}

#[test]
fn audiobook_importer_fails_closed_on_unreadable_files() {
    let error = AudiobookImporter
        .read_metadata("/nonexistent/audiobook.m4b".to_string())
        .expect_err("an unreadable file is an error, never a partial record");

    assert!(!error.to_string().is_empty(), "the error carries a reason");
}

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn maps_a_tagged_audiobook_into_importer_metadata() {
    let meta = AudiobookImporter
        .read_metadata(fixture("tagged.m4b"))
        .expect("tagged fixture imports");

    assert_eq!(meta.title, "The Quiet Shore");
    assert_eq!(
        meta.contributors,
        vec![
            ImporterContributor {
                name: "Ada Lantern".to_string(),
                role: Some("aut".to_string()),
                file_as: None,
            },
            ImporterContributor {
                name: "Ben Reader".to_string(),
                role: Some("nrt".to_string()),
                file_as: None,
            },
        ]
    );
    assert_eq!(
        meta.published,
        Some(ImporterPublicationDate {
            year: 2021,
            month: Some(5),
            day: Some(4),
        })
    );
    assert_eq!(meta.description.as_deref(), Some("A short tale."));
    assert_eq!(meta.subjects, vec!["Fiction".to_string()]);
    assert!(meta.isbns.is_empty(), "MP4 tags carry no ISBN");
    assert_eq!(meta.language, None);

    let cover = meta.cover.expect("attached artwork becomes the cover");
    assert_eq!(cover.mime, "image/jpeg");
    // Base64 of a JPEG SOI/APP0 marker (FF D8 FF E0): the bytes are encoded,
    // not passed through raw or re-encoded.
    assert!(
        cover.data_base64.starts_with("/9j/4"),
        "cover is base64 JPEG, got {}",
        &cover.data_base64[..8.min(cover.data_base64.len())]
    );

    let format = meta.format_metadata.expect("audiobook format metadata");
    assert_eq!(format["duration_seconds"], serde_json::json!(3));
}

#[test]
fn untitled_audiobook_falls_back_to_the_file_stem() {
    let meta = AudiobookImporter
        .read_metadata(fixture("Stem Title.m4a"))
        .expect("artist-only fixture imports");

    assert_eq!(meta.title, "Stem Title");
    assert_eq!(meta.contributors.len(), 1);
    assert_eq!(meta.contributors[0].name, "Solo Voice");
    assert_eq!(meta.cover, None, "no artwork means no cover");
    assert_eq!(meta.published, None);
}

#[test]
fn audiobook_without_creators_fails_closed() {
    AudiobookImporter
        .read_metadata(fixture("no-creator.m4b"))
        .expect_err("a missing creator is an error, never a partial record");
}
