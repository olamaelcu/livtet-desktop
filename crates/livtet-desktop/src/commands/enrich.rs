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
