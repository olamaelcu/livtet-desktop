//! Reference-file extraction, ignored by default: it needs a local audiobook.
//!
//! Run with e.g.
//! `LIVTET_AUDIOBOOK_FIXTURE=~/Documents/Books/book.m4b cargo test -p livtet-audio -- --ignored`.
//! Assertions are structural so any tagged `.m4b`/`.m4a` works as the fixture.

use std::path::PathBuf;

#[test]
#[ignore = "requires LIVTET_AUDIOBOOK_FIXTURE pointing at a local .m4b/.m4a file"]
fn extracts_the_reference_audiobook() {
    let path = PathBuf::from(
        std::env::var("LIVTET_AUDIOBOOK_FIXTURE")
            .expect("set LIVTET_AUDIOBOOK_FIXTURE to a local audiobook file"),
    );
    let meta = livtet_audio::read_metadata(&path).expect("reference audiobook extracts");

    assert!(!meta.title.0.trim().is_empty(), "title is present");
    assert!(!meta.creators.is_empty(), "at least one creator is present");
    assert!(
        meta.creators
            .iter()
            .any(|creator| !creator.name.trim().is_empty()),
        "creator names are non-empty"
    );

    let format = meta
        .format_metadata
        .expect("audiobook format metadata is set");
    let duration = format["duration_seconds"]
        .as_i64()
        .expect("duration_seconds is an integer");
    assert!(duration > 0, "duration is positive");
    let chapters = format["chapters"].as_array().expect("chapters is a list");
    assert!(!chapters.is_empty(), "at least one chapter is present");
    let mut cursor = 0;
    for chapter in chapters {
        let start = chapter["audio_start"]
            .as_i64()
            .expect("audio_start is an integer");
        let end = chapter["audio_end"]
            .as_i64()
            .expect("audio_end is an integer");
        assert!(
            0 <= start && start <= end && end <= duration,
            "chapter bounds are ordered"
        );
        assert!(start >= cursor, "chapters are in order");
        cursor = start;
    }

    livtet_types::FormatMetadataSchema::Audiobook
        .validate(&format)
        .expect("format metadata validates against the audiobook schema");
}
