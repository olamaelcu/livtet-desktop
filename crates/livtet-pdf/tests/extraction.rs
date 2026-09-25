//! Integration tests: synthesize PDFs with `pdf_oxide`'s writer, stamp an Info
//! dictionary onto them, and assert extraction.

use livtet_pdf::{PdfError, PublicationDate, Role};
use pdf_oxide::editor::{DocumentEditor, DocumentInfo, EditableDocument};
use pdf_oxide::writer::{DocumentBuilder, PageSize};

/// Build a one-page PDF carrying `info` and write it to a temp file.
fn fixture(info: DocumentInfo) -> tempfile::NamedTempFile {
    let mut builder = DocumentBuilder::new();
    {
        let page = builder.page(PageSize::A4);
        page.done();
    }
    let bytes = builder.build().expect("fixture PDF builds");
    let file = tempfile::Builder::new().suffix(".pdf").tempfile().unwrap();
    std::fs::write(file.path(), bytes).unwrap();

    let mut editor = DocumentEditor::open(file.path()).expect("fixture opens for editing");
    editor
        .set_info(info)
        .expect("fixture accepts an Info dictionary");
    editor.save(file.path()).expect("fixture saves");

    file
}

fn names(parsed: &livtet_pdf::PdfMetadata) -> Vec<&str> {
    parsed
        .creators
        .iter()
        .map(|creator| creator.name.as_str())
        .collect()
}

#[test]
fn extracts_info_metadata_with_optional_isbn() {
    let mut info = DocumentInfo::new()
        .title("Positive Obsession")
        .author("Susana M. Morris")
        .keywords("9781784780609, publisher-123, Biography");
    info.creation_date = Some("D:20250819235959Z".to_string());

    let parsed = livtet_pdf::read_metadata(fixture(info).path()).expect("valid fixture");

    assert_eq!(parsed.title.0, "Positive Obsession");
    assert_eq!(names(&parsed), vec!["Susana M. Morris"]);
    assert_eq!(parsed.creators[0].role, Role::Author);
    assert_eq!(parsed.publisher, None);
    assert_eq!(
        parsed.published,
        Some(PublicationDate {
            year: 2025,
            month: Some(8),
            day: Some(19),
        })
    );

    let isbns: Vec<&str> = parsed.isbns.iter().map(livtet_pdf::Isbn::as_str).collect();
    assert_eq!(isbns, vec!["9781784780609"]);

    let identifiers: Vec<&str> = parsed
        .other_identifiers
        .iter()
        .map(|identifier| identifier.0.as_str())
        .collect();
    assert!(identifiers.contains(&"publisher-123"), "{identifiers:?}");

    let subjects: Vec<&str> = parsed
        .subjects
        .iter()
        .map(|subject| subject.0.as_str())
        .collect();
    assert!(subjects.contains(&"Biography"), "{subjects:?}");
}

#[test]
fn splits_info_author_into_contributors() {
    let info = DocumentInfo::new()
        .title("Collaborative Work")
        .author("Ada Lovelace and Charles Babbage && Sons");
    let parsed = livtet_pdf::read_metadata(fixture(info).path()).expect("valid fixture");

    assert_eq!(
        names(&parsed),
        vec!["Ada Lovelace", "Charles Babbage & Sons"]
    );
}

#[test]
fn missing_title_fails_closed() {
    let info = DocumentInfo::new().author("Susana M. Morris");
    let error = livtet_pdf::read_metadata(fixture(info).path())
        .expect_err("a PDF with no title must not import");

    assert!(matches!(error, PdfError::MissingRequired("title")));
}

#[test]
fn missing_creator_fails_closed() {
    let info = DocumentInfo::new().title("Positive Obsession");
    let error = livtet_pdf::read_metadata(fixture(info).path())
        .expect_err("a PDF with no creator must not import");

    assert!(matches!(error, PdfError::MissingRequired("creator")));
}

#[test]
fn renders_page_zero_as_cover_fallback() {
    let info = DocumentInfo::new().title("Coverless").author("Someone");
    let parsed = livtet_pdf::read_metadata(fixture(info).path()).expect("valid fixture");

    let cover = parsed.cover.expect("a blank page renders to a PNG cover");
    assert_eq!(cover.mime, "image/png");
    assert!(cover.data.starts_with(b"\x89PNG\r\n\x1a\n"));
}
