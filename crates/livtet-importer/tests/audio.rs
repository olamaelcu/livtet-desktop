use livtet_importer::{AudiobookImporter, Importer};

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
