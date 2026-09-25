use std::io::Write;

use base64::{Engine, engine::general_purpose::STANDARD};
use livtet_importer::{Importer, ImporterPublicationDate, MobiImporter};

const HEADER_LENGTH: u32 = 232;
const CODEPAGE_UTF8: u32 = 65001;

fn exth_item(id: u32, data: &[u8]) -> Vec<u8> {
    let mut item = Vec::with_capacity(8 + data.len());
    item.extend_from_slice(&id.to_be_bytes());
    item.extend_from_slice(&((data.len() + 8) as u32).to_be_bytes());
    item.extend_from_slice(data);
    item
}

fn cover_bytes() -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0];
    bytes.extend_from_slice(b"importer-bytes");
    bytes
}

fn record0() -> Vec<u8> {
    let exth_start = 16 + HEADER_LENGTH as usize;
    let mut record = vec![0u8; exth_start.max(132)];
    record[0..2].copy_from_slice(&2u16.to_be_bytes());
    record[16..20].copy_from_slice(b"MOBI");
    record[20..24].copy_from_slice(&HEADER_LENGTH.to_be_bytes());
    record[28..32].copy_from_slice(&CODEPAGE_UTF8.to_be_bytes());
    record[36..40].copy_from_slice(&8u32.to_be_bytes());
    record[92..96].copy_from_slice(&9u32.to_be_bytes());
    record[108..112].copy_from_slice(&1u32.to_be_bytes());
    record[128..132].copy_from_slice(&0x40u32.to_be_bytes());

    let mut exth = Vec::new();
    exth.extend_from_slice(b"EXTH");
    exth.extend_from_slice(&[0u8; 4]);
    let items = [
        exth_item(503, b"Positive Obsession"),
        exth_item(100, b"Morris, Susana M."),
        exth_item(101, b"Amistad"),
        exth_item(103, b"A biography of Octavia E. Butler."),
        exth_item(104, b"9781784780609"),
        exth_item(105, b"Fiction; Science Fiction"),
        exth_item(106, b"2020-02-11"),
        exth_item(113, b"B08XYZ1234"),
        exth_item(524, b"en"),
        exth_item(201, &0u32.to_be_bytes()),
    ];
    let count = items.len() as u32;
    exth.extend_from_slice(&count.to_be_bytes());
    for item in &items {
        exth.extend_from_slice(item);
    }
    let declared = exth.len() as u32;
    exth[4..8].copy_from_slice(&declared.to_be_bytes());

    record.truncate(exth_start);
    record.extend_from_slice(&exth);

    let full_name = b"Positive Obsession";
    let full_name_offset = record.len() as u32;
    record.extend_from_slice(full_name);
    record[84..88].copy_from_slice(&full_name_offset.to_be_bytes());
    record[88..92].copy_from_slice(&(full_name.len() as u32).to_be_bytes());

    record
}

fn fixture_mobi() -> (tempfile::NamedTempFile, Vec<u8>) {
    let header = record0();
    let cover = cover_bytes();
    let records: Vec<&[u8]> = vec![&header, &cover];
    let nrec = records.len();

    let mut out = vec![0u8; 78];
    out[..8].copy_from_slice(b"TestBook");
    out[60..64].copy_from_slice(b"BOOK");
    out[64..68].copy_from_slice(b"MOBI");
    out[76..78].copy_from_slice(&(nrec as u16).to_be_bytes());

    let mut offset = 78 + 8 * nrec;
    for record in &records {
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&[0u8; 4]);
        offset += record.len();
    }
    for record in records {
        out.extend_from_slice(record);
    }

    let mut file = tempfile::Builder::new().suffix(".azw3").tempfile().unwrap();
    file.write_all(&out).unwrap();
    file.flush().unwrap();
    (file, cover)
}

#[test]
fn mobi_importer_preserves_native_catalog_fields() {
    let (mobi, expected_cover) = fixture_mobi();
    let importer = MobiImporter;
    let record = importer
        .read_metadata(mobi.path().to_string_lossy().into_owned())
        .expect("fixture MOBI has complete importer metadata");

    assert_eq!(
        importer.extensions().unwrap(),
        vec!["azw3".to_string(), "azw".to_string()]
    );
    assert_eq!(record.title, "Positive Obsession");
    assert_eq!(record.contributors.len(), 1);
    assert_eq!(record.contributors[0].name, "Susana M. Morris");
    assert_eq!(record.contributors[0].role.as_deref(), Some("aut"));
    assert_eq!(
        record.contributors[0].file_as.as_deref(),
        Some("Morris, Susana M.")
    );
    assert_eq!(record.isbns, vec!["9781784780609".to_string()]);
    assert_eq!(record.publisher.as_deref(), Some("Amistad"));
    assert_eq!(record.language.as_deref(), Some("en"));
    assert_eq!(
        record.other_identifiers,
        vec!["B08XYZ1234".to_string()],
        "non-ISBN EXTH identifiers survive"
    );
    assert_eq!(
        record.published,
        Some(ImporterPublicationDate {
            year: 2020,
            month: Some(2),
            day: Some(11),
        })
    );
    assert_eq!(
        record.description.as_deref(),
        Some("A biography of Octavia E. Butler.")
    );
    assert_eq!(
        record.subjects,
        vec!["Fiction".to_string(), "Science Fiction".to_string()],
        "EXTH subjects split on ';'"
    );
    assert!(
        record.title_sort.is_none(),
        "MOBI carries no title-sort field"
    );

    let cover = record.cover.expect("cover is preserved");
    assert_eq!(cover.mime, "image/jpeg");
    assert_eq!(STANDARD.decode(&cover.data_base64).unwrap(), expected_cover);
}

#[test]
fn mobi_importer_rejects_garbage_bytes() {
    let mut file = tempfile::Builder::new().suffix(".azw3").tempfile().unwrap();
    file.write_all(&[0x58u8; 100]).unwrap();
    file.flush().unwrap();

    let error = MobiImporter
        .read_metadata(file.path().to_string_lossy().into_owned())
        .expect_err("garbage bytes are not a MOBI-family file");
    assert!(
        error.to_string().contains("not a MOBI-family file"),
        "error identifies the non-MOBI container, not an I/O failure: {error}"
    );
}
