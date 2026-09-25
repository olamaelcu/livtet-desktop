use base64::{Engine, engine::general_purpose::STANDARD};
use livtet_importer::{Importer, PdfImporter};
use pdf_oxide::editor::{DocumentEditor, DocumentInfo, EditableDocument};
use pdf_oxide::writer::{DocumentBuilder, PageSize};

fn fixture_pdf() -> tempfile::NamedTempFile {
    let mut builder = DocumentBuilder::new();
    {
        let page = builder.page(PageSize::A4);
        page.done();
    }
    let bytes = builder.build().unwrap();
    let file = tempfile::Builder::new().suffix(".pdf").tempfile().unwrap();
    std::fs::write(file.path(), bytes).unwrap();

    let mut info = DocumentInfo::new()
        .title("Positive Obsession")
        .author("Susana M. Morris")
        .keywords("9781784780609, publisher-123, Biography");
    info.creation_date = Some("D:20250819235959Z".to_string());
    let mut editor = DocumentEditor::open(file.path()).unwrap();
    editor.set_info(info).unwrap();
    editor.save(file.path()).unwrap();

    file
}

#[test]
fn pdf_importer_preserves_native_catalog_fields() {
    let pdf = fixture_pdf();
    let importer = PdfImporter;
    let record = importer
        .read_metadata(pdf.path().to_string_lossy().into_owned())
        .expect("fixture PDF has complete importer metadata");

    assert_eq!(importer.extensions().unwrap(), vec!["pdf".to_string()]);
    assert_eq!(record.title, "Positive Obsession");
    assert_eq!(record.title_sort, None);
    assert_eq!(record.contributors.len(), 1);
    assert_eq!(record.contributors[0].name, "Susana M. Morris");
    assert_eq!(record.contributors[0].role.as_deref(), Some("aut"));
    assert_eq!(record.contributors[0].file_as, None);
    assert_eq!(record.isbns, vec!["9781784780609".to_string()]);
    assert_eq!(
        record.other_identifiers,
        vec!["publisher-123".to_string(), "Biography".to_string()]
    );
    assert_eq!(record.publisher, None);
    assert_eq!(record.language, None);
    let published = record.published.expect("publication date is preserved");
    assert_eq!(
        (published.year, published.month, published.day),
        (2025, Some(8), Some(19))
    );
    assert!(record.subjects.contains(&"Biography".to_string()));

    let cover = record.cover.expect("cover is preserved");
    assert_eq!(cover.mime, "image/png");
    assert!(
        STANDARD
            .decode(&cover.data_base64)
            .unwrap()
            .starts_with(b"\x89PNG\r\n\x1a\n")
    );
}

#[test]
fn pdf_importer_without_title_fails() {
    let mut builder = DocumentBuilder::new();
    {
        let page = builder.page(PageSize::A4);
        page.done();
    }
    let bytes = builder.build().unwrap();
    let file = tempfile::Builder::new().suffix(".pdf").tempfile().unwrap();
    std::fs::write(file.path(), bytes).unwrap();

    let mut editor = DocumentEditor::open(file.path()).unwrap();
    editor
        .set_info(DocumentInfo::new().author("Susana M. Morris"))
        .unwrap();
    editor.save(file.path()).unwrap();

    let error = PdfImporter
        .read_metadata(file.path().to_string_lossy().into_owned())
        .expect_err("a PDF with no title must not import");
    assert!(
        error.to_string().contains("title"),
        "unexpected error: {error}"
    );
}
