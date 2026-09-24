use livtet_types::DbId;
use serde::Serialize;
use specta::Type;

#[derive(Debug, Clone, Serialize, Type)]
pub struct CatalogError {
    pub code: String,
    pub message: String,
}

impl CatalogError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into() }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct DeleteSkip {
    pub edition_id: DbId,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct DeleteOutcome {
    pub deleted: i32,
    pub files_removed: i32,
    pub covers_removed: i32,
    pub skipped: Vec<DeleteSkip>,
}

impl DeleteOutcome {
    pub fn empty() -> Self {
        Self { deleted: 0, files_removed: 0, covers_removed: 0, skipped: Vec::new() }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct ExportOutcome {
    pub path: String,
    pub rows: i32,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct TagMutationOutcome {
    pub tag: crate::commands::search::FilterOption,
    pub changed: i32,
}

/// RFC 4180-style field escaping: wrap in quotes when the value contains a
/// comma, quote, CR, or LF, doubling any embedded quotes.
pub fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

pub fn csv_row(fields: &[&str]) -> String {
    fields.iter().map(|f| csv_escape(f)).collect::<Vec<_>>().join(",")
}

#[cfg(test)]
mod tests {
    use super::{csv_escape, csv_row, DeleteOutcome, DeleteSkip};
    use livtet_types::DbId;

    #[test]
    fn csv_escape_quotes_when_needed() {
        assert_eq!(csv_escape("plain"), "plain");
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
        assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_escape("line\nbreak"), "\"line\nbreak\"");
    }

    #[test]
    fn csv_row_joins_with_commas() {
        assert_eq!(csv_row(&["a", "b,c", "d"]), "a,\"b,c\",d");
    }

    #[test]
    fn empty_outcome_is_all_zero() {
        let outcome = DeleteOutcome::empty();
        assert_eq!((outcome.deleted, outcome.files_removed, outcome.covers_removed), (0, 0, 0));
        assert!(outcome.skipped.is_empty());
        let _ = DeleteSkip { edition_id: DbId::new(), reason: "x".into() };
    }
}
