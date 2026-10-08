use std::collections::{BTreeMap, HashSet};

use crate::error::PluginError;
use crate::types::AppState;
use livtet_importer::{
    Enrichment, Identifier, ImporterContributor, ImporterCover, ImporterPublicationDate,
};
use tauri::State;

/// Fields we track for enrichment
#[derive(Debug, Clone, serde::Serialize, Default)]
pub struct EditionFields {
    pub title: Option<String>,
    pub title_sort: Option<String>,
    pub contributors: Vec<String>,
    pub publisher: Option<String>,
    pub language: Option<String>,
    pub published: Option<ImporterPublicationDate>,
    pub description: Option<String>,
    pub subjects: Vec<String>,
    pub cover: Option<ImporterCover>,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct EnrichmentPreview {
    pub added: BTreeMap<String, String>,
    pub unioned: BTreeMap<String, Vec<String>>,
    pub conflicts: BTreeMap<String, String>,
    pub found: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct EnrichmentSummary {
    pub edition_id: String,
    pub enricher: String,
    pub mode: String,
    pub fields_added: usize,
    pub fields_unioned: usize,
    pub identifiers_persisted: usize,
}

fn format_date(date: &ImporterPublicationDate) -> String {
    match (date.year, date.month, date.day) {
        (y, Some(m), Some(d)) => format!("{y:04}-{m:02}-{d:02}"),
        (y, Some(m), None) => format!("{y:04}-{m:02}"),
        (y, None, _) => format!("{y:04}"),
    }
}

fn compute_merge(
    current: &EditionFields,
    enrichment: &Enrichment,
    _mode: &str,
) -> EnrichmentPreview {
    let mut added = BTreeMap::new();
    let mut unioned: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut conflicts = BTreeMap::new();
    let found: Vec<String> = enrichment.found.iter().map(|id| id.value.clone()).collect();

    if let (None, Some(src)) = (&current.title_sort, &enrichment.title_sort) {
        added.insert("title_sort".to_string(), src.clone());
    }

    match (&current.publisher, &enrichment.publisher) {
        (None, Some(src)) => {
            added.insert("publisher".to_string(), src.clone());
        }
        (Some(cur), Some(src)) if cur != src => {
            conflicts.insert(
                "publisher".to_string(),
                format!("current: \"{}\", source: \"{}\"", cur, src),
            );
        }
        _ => {}
    }

    match (&current.description, &enrichment.description) {
        (None, Some(src)) => {
            added.insert("description".to_string(), src.clone());
        }
        (Some(cur), Some(src)) if cur != src => {
            conflicts.insert(
                "description".to_string(),
                format!("current: \"{}\", source: \"{}\"", cur, src),
            );
        }
        _ => {}
    }

    match (&current.language, &enrichment.language) {
        (None, Some(src)) => {
            added.insert("language".to_string(), src.clone());
        }
        (Some(cur), Some(src)) if cur != src => {
            conflicts.insert(
                "language".to_string(),
                format!("current: \"{}\", source: \"{}\"", cur, src),
            );
        }
        _ => {}
    }

    match (&current.published, &enrichment.published) {
        (None, Some(src)) => {
            added.insert("published".to_string(), format_date(src));
        }
        (Some(cur), Some(src)) => {
            let (cur_str, src_str) = (format_date(cur), format_date(src));
            if cur_str != src_str {
                conflicts.insert(
                    "published".to_string(),
                    format!("current: \"{cur_str}\", source: \"{src_str}\""),
                );
            }
        }
        _ => {}
    }

    if current.cover.is_none() && enrichment.cover.is_some() {
        added.insert("cover".to_string(), "present".to_string());
    }

    let mut existing_contributors: HashSet<String> = current.contributors.iter().cloned().collect();
    for contrib in &enrichment.contributors {
        if !existing_contributors.contains(&contrib.name) {
            unioned
                .entry("contributors".to_string())
                .or_default()
                .push(contrib.name.clone());
            existing_contributors.insert(contrib.name.clone());
        }
    }

    let mut existing_subjects: HashSet<String> = current.subjects.iter().cloned().collect();
    for subj in &enrichment.subjects {
        if !existing_subjects.contains(subj) {
            unioned
                .entry("subjects".to_string())
                .or_default()
                .push(subj.clone());
            existing_subjects.insert(subj.clone());
        }
    }

    EnrichmentPreview {
        added,
        unioned,
        conflicts,
        found,
    }
}

#[tauri::command]
#[specta::specta]
pub async fn enrich_edition(
    _state: State<'_, AppState>,
    _edition_id: String,
    plugin_name: Option<String>,
    mode: String,
) -> Result<EnrichmentPreview, PluginError> {
    if plugin_name.is_none() {
        return Err(PluginError::invalid(
            "no enricher plugin selected; open /plugins to add one",
        ));
    }

    let current_edition = EditionFields {
        title: Some("Mock Title".to_string()),
        title_sort: None,
        contributors: Vec::new(),
        publisher: Some("Mock Publisher".to_string()),
        language: Some("en".to_string()),
        published: Some(ImporterPublicationDate {
            year: 2020,
            month: Some(1),
            day: Some(15),
        }),
        description: Some("Existing description".to_string()),
        subjects: vec!["Science Fiction".to_string()],
        cover: Some(ImporterCover {
            mime: "image/jpeg".to_string(),
            data_base64: "existing".to_string(),
        }),
    };

    let enrichment = Enrichment {
        title: Some("Dune".to_string()),
        title_sort: Some("Dune".to_string()),
        contributors: vec![ImporterContributor {
            name: "Frank Herbert".to_string(),
            role: Some("author".to_string()),
            file_as: None,
        }],
        publisher: Some("Ace Books".to_string()),
        language: Some("eng".to_string()),
        published: Some(ImporterPublicationDate {
            year: 1965,
            month: None,
            day: None,
        }),
        description: Some("A science fiction epic set on the desert planet Arrakis.".to_string()),
        subjects: vec!["Science Fiction".to_string(), "Dune Universe".to_string()],
        cover: Some(ImporterCover {
            mime: "image/jpeg".to_string(),
            data_base64: "...".to_string(),
        }),
        found: vec![Identifier {
            kind: "olid".to_string(),
            value: "OL12345W".to_string(),
        }],
    };

    Ok(compute_merge(&current_edition, &enrichment, &mode))
}

#[tauri::command]
#[specta::specta]
pub async fn enrich_library(
    _state: State<'_, AppState>,
    _filter: String,
    _plugin_name: Option<String>,
    _mode: String,
) -> Result<Vec<EnrichmentSummary>, PluginError> {
    Err(PluginError::unavailable(
        "bulk enrich library is planned for a future release",
    ))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    /// `Enrichment` has no `Default`, so tests start from an empty one and set
    /// only the field under test.
    fn empty() -> Enrichment {
        Enrichment {
            title: None,
            title_sort: None,
            contributors: Vec::new(),
            publisher: None,
            language: None,
            published: None,
            description: None,
            subjects: Vec::new(),
            cover: None,
            found: Vec::new(),
        }
    }

    fn date(year: i32, month: Option<u8>, day: Option<u8>) -> ImporterPublicationDate {
        ImporterPublicationDate { year, month, day }
    }

    #[test]
    fn format_date_renders_year_month_day() {
        assert_eq!(format_date(&date(2024, Some(3), Some(7))), "2024-03-07");
    }

    #[test]
    fn format_date_omits_absent_day() {
        assert_eq!(format_date(&date(2024, Some(3), None)), "2024-03");
    }

    #[test]
    fn format_date_renders_year_alone() {
        assert_eq!(format_date(&date(2024, None, None)), "2024");
    }

    #[test]
    fn merge_adds_a_field_absent_from_current() {
        let enrichment = Enrichment {
            publisher: Some("Verso".to_string()),
            ..empty()
        };

        let preview = compute_merge(&EditionFields::default(), &enrichment, "union");

        assert_eq!(
            preview.added.get("publisher").map(String::as_str),
            Some("Verso")
        );
        assert!(preview.conflicts.is_empty());
    }

    #[test]
    fn merge_reports_a_conflict_when_both_sides_differ() {
        let current = EditionFields {
            publisher: Some("Verso".to_string()),
            ..Default::default()
        };
        let enrichment = Enrichment {
            publisher: Some("Pluto".to_string()),
            ..empty()
        };

        let preview = compute_merge(&current, &enrichment, "union");

        assert!(preview.added.is_empty());
        assert_eq!(
            preview.conflicts.get("publisher").map(String::as_str),
            Some("current: \"Verso\", source: \"Pluto\"")
        );
    }

    #[test]
    fn merge_unions_list_valued_fields_without_duplicating() {
        let current = EditionFields {
            subjects: vec!["politics".to_string()],
            ..Default::default()
        };
        let enrichment = Enrichment {
            subjects: vec!["politics".to_string(), "history".to_string()],
            ..empty()
        };

        let preview = compute_merge(&current, &enrichment, "union");

        assert_eq!(
            preview.unioned.get("subjects"),
            Some(&vec!["history".to_string()])
        );
    }

    #[test]
    fn merge_collects_found_identifiers() {
        let enrichment = Enrichment {
            found: vec![Identifier {
                kind: "olid".to_string(),
                value: "OL123M".to_string(),
            }],
            ..empty()
        };

        let preview = compute_merge(&EditionFields::default(), &enrichment, "union");

        assert_eq!(preview.found, vec!["OL123M".to_string()]);
    }

    /// The `mode` parameter is bound as `_mode` and never read, so every mode
    /// produces the same preview. Pinned so the dead parameter is visible
    /// rather than assumed to work.
    #[test]
    fn merge_ignores_its_mode_argument() {
        let current = EditionFields {
            publisher: Some("Verso".to_string()),
            ..Default::default()
        };
        let enrichment = Enrichment {
            publisher: Some("Pluto".to_string()),
            subjects: vec!["history".to_string()],
            ..empty()
        };

        let union = compute_merge(&current, &enrichment, "union");
        let replace = compute_merge(&current, &enrichment, "replace");

        assert_eq!(union.added, replace.added);
        assert_eq!(union.unioned, replace.unioned);
        assert_eq!(union.conflicts, replace.conflicts);
    }

    /// Unlike publisher, description and language, `title_sort` has only an
    /// "added" branch: a differing title_sort is silently dropped.
    #[test]
    fn merge_never_reports_a_title_sort_conflict() {
        let current = EditionFields {
            title_sort: Some("Capital".to_string()),
            ..Default::default()
        };
        let enrichment = Enrichment {
            title_sort: Some("Kapital, Das".to_string()),
            ..empty()
        };

        let preview = compute_merge(&current, &enrichment, "union");

        assert!(preview.added.is_empty());
        assert!(preview.conflicts.is_empty());
    }
}
