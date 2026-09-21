use livtet_importer::{ImporterContributor, ImporterCover, ImporterMeta, ImporterPublicationDate};
use serde_json::{Value, json};

fn complete_record() -> ImporterMeta {
    ImporterMeta {
        title: "Positive Obsession".to_string(),
        contributors: vec![ImporterContributor {
            name: "Susana M. Morris".to_string(),
            role: Some("aut".to_string()),
            file_as: Some("Morris, Susana M.".to_string()),
        }],
        isbns: vec!["9781784780609".to_string()],
        other_identifiers: vec!["publisher-123".to_string()],
        publisher: Some("Amistad".to_string()),
        language: Some("en".to_string()),
        published: Some(ImporterPublicationDate {
            year: 2025,
            month: Some(8),
            day: Some(19),
        }),
        description: Some("A biography of Octavia E. Butler.".to_string()),
        subjects: vec!["Biography".to_string()],
        cover: Some(ImporterCover {
            mime: "image/png".to_string(),
            data_base64: "iVBORw0KGgo=".to_string(),
        }),
    }
}

#[test]
fn importer_metadata_preserves_contract_fields() {
    let record = complete_record();
    let encoded = serde_json::to_value(&record).expect("importer metadata is JSON");
    let decoded: ImporterMeta =
        serde_json::from_value(encoded.clone()).expect("importer metadata round-trips");

    assert_eq!(decoded, record);
    assert_eq!(
        encoded,
        json!({
            "title": "Positive Obsession",
            "contributors": [{
                "name": "Susana M. Morris",
                "role": "aut",
                "file_as": "Morris, Susana M."
            }],
            "isbns": ["9781784780609"],
            "other_identifiers": ["publisher-123"],
            "publisher": "Amistad",
            "language": "en",
            "published": {"year": 2025, "month": 8, "day": 19},
            "description": "A biography of Octavia E. Butler.",
            "subjects": ["Biography"],
            "cover": {"mime": "image/png", "data_base64": "iVBORw0KGgo="}
        })
    );
    assert!(matches!(encoded, Value::Object(_)));
}

#[test]
fn lua_empty_lists_decode_as_empty() {
    let wire = json!({
        "title": "Remote Title",
        "contributors": [],
        "isbns": ["9781784780609"],
        "other_identifiers": {},
        "publisher": null,
        "language": null,
        "published": null,
        "description": null,
        "subjects": [],
        "cover": null
    });

    let decoded =
        ImporterMeta::from_wire_json(wire).expect("empty Lua tables decode as empty lists");
    assert!(decoded.other_identifiers.is_empty());
}

#[test]
fn lua_empty_optional_objects_decode_as_missing() {
    let wire = json!({
        "title": "Remote Title",
        "contributors": [{"name": "Remote Author", "role": {}, "file_as": {}}],
        "isbns": ["9781784780609"],
        "other_identifiers": [],
        "publisher": {},
        "language": {},
        "published": {},
        "description": {},
        "subjects": [],
        "cover": {}
    });

    let decoded =
        ImporterMeta::from_wire_json(wire).expect("empty Lua tables decode as missing optionals");
    assert_eq!(decoded.publisher, None);
    assert_eq!(decoded.language, None);
    assert_eq!(decoded.published, None);
    assert_eq!(decoded.description, None);
    assert_eq!(decoded.cover, None);
    assert_eq!(decoded.contributors[0].role, None);
    assert_eq!(decoded.contributors[0].file_as, None);
}
