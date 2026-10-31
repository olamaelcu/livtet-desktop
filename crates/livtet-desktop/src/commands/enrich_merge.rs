//! The pure heart of enrichment: what a source contributes, and how it folds
//! into an edition under each mode.
//!
//! No host, no database, no network. `combine` folds several plugins' answers
//! into one; `merge` classifies that answer against the edition's current values
//! and resolves the set `apply` writes. Preview and apply call the same `merge`,
//! so the diff a reader confirms cannot drift from what lands (ADR-0046).

use std::collections::HashSet;

use livtet_importer::{
    BookIdentity, Enrichment, Identifier, ImporterCover, ImporterPublicationDate,
};

/// Fields of an edition that enrichment can touch.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EditionFields {
    pub title: Option<String>,
    pub title_sort: Option<String>,
    pub contributors: Vec<String>,
    pub publisher: Option<String>,
    /// Language code as stored (`en`, `eng`), not the display name.
    pub language: Option<String>,
    pub published: Option<ImporterPublicationDate>,
    pub description: Option<String>,
    pub subjects: Vec<String>,
    /// Whether the edition already has a cover on disk. The bytes are not read:
    /// a merge only needs to know whether the slot is taken.
    pub has_cover: bool,
}

/// Which values a merge may replace, serialized as `"fill" | "union" | "overwrite"`.
///
/// `fill` takes only blanks; `union` additionally appends list members;
/// `overwrite` lets the source replace a differing scalar. Lists are never
/// shrunk in any mode — a source does not know which local subjects or
/// contributors to remove.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum EnrichMode {
    Fill,
    Union,
    Overwrite,
}

/// One scalar the merge would add.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct FieldChange {
    pub field: String,
    pub value: String,
}

/// One list field the merge would extend.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct FieldAddition {
    pub field: String,
    pub values: Vec<String>,
}

/// A scalar where the edition and the source disagree.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct FieldConflict {
    pub field: String,
    pub current: String,
    pub incoming: String,
}

/// What the source offers for an edition that has no cover.
///
/// `storable` is false when the edition has no `digital_inventory` row, which is
/// the only place a cover path can live; the preview says so instead of quietly
/// dropping it (ADR-0046).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct CoverOffer {
    pub mime: String,
    pub storable: bool,
}

/// One identifier the source resolved, in the shape the drawer already renders
/// (`{ kind, value }`); `livtet_importer::Identifier` carries no `specta::Type`,
/// so the IPC boundary keeps its own mirror rather than pushing a TypeScript
/// generator into the importer crate.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct FoundIdentifier {
    pub kind: String,
    pub value: String,
}

impl From<&Identifier> for FoundIdentifier {
    fn from(identifier: &Identifier) -> Self {
        Self {
            kind: identifier.kind.clone(),
            value: identifier.value.clone(),
        }
    }
}

/// Everything the review screen shows.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct EnrichmentPreview {
    /// Contributing plugins, in merge order.
    pub sources: Vec<String>,
    pub added: Vec<FieldChange>,
    pub unioned: Vec<FieldAddition>,
    pub conflicts: Vec<FieldConflict>,
    /// Identifiers the source resolved; persisted on apply regardless of mode.
    pub found: Vec<FoundIdentifier>,
    pub cover: Option<CoverOffer>,
}

/// What applying a preview did.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct EnrichmentSummary {
    pub edition_id: String,
    pub sources: Vec<String>,
    pub mode: EnrichMode,
    // `u32`, not `usize`: Specta refuses to export BigInt-style types to
    // TypeScript, and an edition's field count never approaches this bound.
    pub fields_added: u32,
    pub fields_overwritten: u32,
    pub fields_unioned: u32,
    pub identifiers_persisted: u32,
    pub cover_stored: bool,
}

/// The resolved field set `apply` writes.
///
/// Lists are the final membership, not just the additions, so the writer links
/// every member it must keep rather than trying to reconstruct intent.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AppliedFields {
    pub title: Option<String>,
    pub title_sort: Option<String>,
    pub publisher: Option<String>,
    pub description: Option<String>,
    pub language: Option<String>,
    pub published: Option<ImporterPublicationDate>,
    pub cover: Option<ImporterCover>,
    pub contributors: Vec<String>,
    pub subjects: Vec<String>,
    pub identifiers: Vec<Identifier>,
}

/// Facts about the target edition the merge needs but cannot infer.
#[derive(Debug, Clone, Default)]
pub struct MergeContext {
    /// Plugins that contributed, in merge order.
    pub sources: Vec<String>,
    /// Whether the edition has a `digital_inventory` row to hang a cover on.
    pub cover_storable: bool,
}

/// One merge's classification and the field set it resolves to.
#[derive(Debug, Clone, PartialEq)]
pub struct Merge {
    pub preview: EnrichmentPreview,
    pub applied: AppliedFields,
}

/// Fold several plugins' answers into one, in plugin order.
///
/// The first non-`None` scalar wins; lists concatenate then dedupe preserving
/// order; `found` dedupes by `(kind, value)`. Returns the contributing plugin
/// names alongside, so the preview can say who answered.
pub fn combine(answers: Vec<(String, Enrichment)>) -> (Vec<String>, Enrichment) {
    let mut sources = Vec::with_capacity(answers.len());
    let mut combined = Enrichment {
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
    };

    for (plugin, enrichment) in answers {
        sources.push(plugin);
        combined.title = combined.title.or(enrichment.title);
        combined.title_sort = combined.title_sort.or(enrichment.title_sort);
        combined.publisher = combined.publisher.or(enrichment.publisher);
        combined.language = combined.language.or(enrichment.language);
        combined.published = combined.published.or(enrichment.published);
        combined.description = combined.description.or(enrichment.description);
        combined.cover = combined.cover.or(enrichment.cover);
        combined.contributors.extend(enrichment.contributors);
        combined.subjects.extend(enrichment.subjects);
        combined.found.extend(enrichment.found);
    }

    let mut seen_contributors: HashSet<(String, String)> = HashSet::new();
    combined.contributors.retain(|contributor| {
        seen_contributors.insert((
            contributor.name.clone(),
            contributor.role.clone().unwrap_or_default(),
        ))
    });
    let mut seen_found: HashSet<(String, String)> = HashSet::new();
    combined.found.retain(|identifier| {
        seen_found.insert((identifier.kind.clone(), identifier.value.clone()))
    });
    let mut seen_subjects: HashSet<String> = HashSet::new();
    combined
        .subjects
        .retain(|subject| seen_subjects.insert(subject.clone()));

    (sources, combined)
}

/// Classify `incoming` against `current` and resolve what `apply` writes.
pub fn merge(
    current: &EditionFields,
    incoming: &Enrichment,
    mode: EnrichMode,
    context: &MergeContext,
) -> Merge {
    let mut added: Vec<FieldChange> = Vec::new();
    let mut unioned: Vec<FieldAddition> = Vec::new();
    let mut conflicts: Vec<FieldConflict> = Vec::new();
    let mut applied = AppliedFields {
        contributors: current.contributors.clone(),
        subjects: current.subjects.clone(),
        ..Default::default()
    };

    // Scalars: a blank is filled in every mode; a disagreement is a conflict,
    // and the mode decides which side `applied` carries.
    macro_rules! scalar {
        ($field:literal, $current:expr, $incoming:expr, $slot:ident) => {
            match ($current.as_deref(), $incoming.as_deref()) {
                (None, Some(source)) => {
                    added.push(FieldChange {
                        field: $field.to_string(),
                        value: source.to_string(),
                    });
                    applied.$slot = Some(source.to_string());
                }
                (Some(kept), Some(source)) if kept != source => {
                    conflicts.push(FieldConflict {
                        field: $field.to_string(),
                        current: kept.to_string(),
                        incoming: source.to_string(),
                    });
                    applied.$slot = Some(match mode {
                        EnrichMode::Overwrite => source.to_string(),
                        EnrichMode::Fill | EnrichMode::Union => kept.to_string(),
                    });
                }
                _ => {}
            }
        };
    }

    scalar!("title", current.title, incoming.title, title);
    scalar!(
        "title_sort",
        current.title_sort,
        incoming.title_sort,
        title_sort
    );
    scalar!(
        "publisher",
        current.publisher,
        incoming.publisher,
        publisher
    );
    scalar!(
        "description",
        current.description,
        incoming.description,
        description
    );
    scalar!("language", current.language, incoming.language, language);

    match (&current.published, &incoming.published) {
        (None, Some(source)) => {
            added.push(FieldChange {
                field: String::from("published"),
                value: format_date(source),
            });
            applied.published = Some(source.clone());
        }
        (Some(kept), Some(source)) => {
            let (kept_str, source_str) = (format_date(kept), format_date(source));
            if kept_str != source_str {
                conflicts.push(FieldConflict {
                    field: String::from("published"),
                    current: kept_str,
                    incoming: source_str,
                });
                applied.published = Some(match mode {
                    EnrichMode::Overwrite => source.clone(),
                    EnrichMode::Fill | EnrichMode::Union => kept.clone(),
                });
            }
        }
        _ => {}
    }

    // Lists: the additions are always *reported*, so the preview describes what
    // the source offers regardless of mode; the mode only decides whether
    // `applied` takes them. `fill` leaves a populated list alone; `union` and
    // `overwrite` append the missing members. No mode removes one.
    let new_contributors: Vec<String> = dedupe(
        incoming
            .contributors
            .iter()
            .map(|contributor| contributor.name.clone()),
    )
    .into_iter()
    .filter(|name| !current.contributors.contains(name))
    .collect();
    if !new_contributors.is_empty() {
        unioned.push(FieldAddition {
            field: String::from("contributors"),
            values: new_contributors.clone(),
        });
        if mode != EnrichMode::Fill || current.contributors.is_empty() {
            applied.contributors.extend(new_contributors);
        }
    }

    let new_subjects: Vec<String> = dedupe(incoming.subjects.iter().cloned())
        .into_iter()
        .filter(|subject| !current.subjects.contains(subject))
        .collect();
    if !new_subjects.is_empty() {
        unioned.push(FieldAddition {
            field: String::from("subjects"),
            values: new_subjects.clone(),
        });
        if mode != EnrichMode::Fill || current.subjects.is_empty() {
            applied.subjects.extend(new_subjects);
        }
    }

    // A cover is only ever added to an edition that has none, and only stored
    // when there is an inventory row to hang the path on.
    let cover = match (&incoming.cover, current.has_cover) {
        (Some(offer), false) => Some(CoverOffer {
            mime: offer.mime.clone(),
            storable: context.cover_storable,
        }),
        _ => None,
    };
    if let Some(offer) = &cover
        && offer.storable
    {
        applied.cover = incoming.cover.clone();
    }

    let identifiers = incoming.found.clone();
    applied.identifiers = identifiers.clone();
    if !identifiers.is_empty() {
        unioned.push(FieldAddition {
            field: String::from("identifiers"),
            values: identifiers
                .iter()
                .map(|identifier| format!("{}:{}", identifier.kind, identifier.value))
                .collect(),
        });
    }

    // Field order, not insertion order: the review screen and its tests read the
    // same list regardless of which branch pushed first.
    added.sort_by(|left, right| left.field.cmp(&right.field));
    unioned.sort_by(|left, right| left.field.cmp(&right.field));
    conflicts.sort_by(|left, right| left.field.cmp(&right.field));

    Merge {
        preview: EnrichmentPreview {
            sources: context.sources.clone(),
            added,
            unioned,
            conflicts,
            found: incoming.found.iter().map(FoundIdentifier::from).collect(),
            cover,
        },
        applied,
    }
}

/// The identity an enricher keys on, taken from the edition's own record.
pub fn identity_from(
    title: Option<String>,
    authors: Vec<String>,
    identifiers: Vec<Identifier>,
) -> BookIdentity {
    BookIdentity {
        identifiers,
        title,
        authors,
    }
}

/// Order-preserving dedupe, so a source that repeats a name contributes once.
fn dedupe(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

/// Render a publication date at the precision it carries (`1965`, `1965-08`,
/// `1965-08-01`), so a year-only source and a full date compare visibly.
pub fn format_date(date: &ImporterPublicationDate) -> String {
    match (date.year, date.month, date.day) {
        (year, Some(month), Some(day)) => format!("{year:04}-{month:02}-{day:02}"),
        (year, Some(month), None) => format!("{year:04}-{month:02}"),
        (year, None, _) => format!("{year:04}"),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use livtet_importer::ImporterContributor;

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

    fn contributor(name: &str) -> ImporterContributor {
        ImporterContributor {
            name: name.to_string(),
            role: Some(String::from("aut")),
            file_as: None,
        }
    }

    fn cover(data: &str) -> ImporterCover {
        ImporterCover {
            mime: String::from("image/jpeg"),
            data_base64: data.to_string(),
        }
    }

    fn merged(current: &EditionFields, incoming: &Enrichment, mode: EnrichMode) -> Merge {
        merge(current, incoming, mode, &MergeContext::default())
    }

    fn fields(preview: &EnrichmentPreview) -> Vec<String> {
        preview
            .added
            .iter()
            .map(|change| change.field.clone())
            .collect()
    }

    #[test]
    fn every_scalar_absent_from_current_is_added_in_every_mode() {
        let incoming = Enrichment {
            title: Some(String::from("Dune")),
            title_sort: Some(String::from("Dune")),
            publisher: Some(String::from("Chilton")),
            language: Some(String::from("en")),
            published: Some(date(1965, Some(8), Some(1))),
            description: Some(String::from("Desert planet.")),
            ..empty()
        };

        for mode in [EnrichMode::Fill, EnrichMode::Union, EnrichMode::Overwrite] {
            let merge = merged(&EditionFields::default(), &incoming, mode);
            assert_eq!(
                fields(&merge.preview),
                vec![
                    "description",
                    "language",
                    "published",
                    "publisher",
                    "title",
                    "title_sort"
                ],
                "{mode:?}"
            );
            assert!(merge.preview.conflicts.is_empty(), "{mode:?}");
            assert_eq!(merge.applied.title.as_deref(), Some("Dune"), "{mode:?}");
            assert_eq!(
                merge.applied.published,
                Some(date(1965, Some(8), Some(1))),
                "{mode:?}"
            );
        }
    }

    /// The mode is the only thing that decides who wins a disagreement.
    #[test]
    fn a_differing_scalar_is_a_conflict_resolved_by_the_mode() {
        let current = EditionFields {
            publisher: Some(String::from("Ace")),
            ..Default::default()
        };
        let incoming = Enrichment {
            publisher: Some(String::from("Chilton")),
            ..empty()
        };

        let fill = merged(&current, &incoming, EnrichMode::Fill);
        let union = merged(&current, &incoming, EnrichMode::Union);
        let overwrite = merged(&current, &incoming, EnrichMode::Overwrite);

        for merge in [&fill, &union, &overwrite] {
            assert_eq!(
                merge.preview.conflicts,
                vec![FieldConflict {
                    field: String::from("publisher"),
                    current: String::from("Ace"),
                    incoming: String::from("Chilton"),
                }]
            );
        }
        assert_eq!(fill.applied.publisher.as_deref(), Some("Ace"));
        assert_eq!(union.applied.publisher.as_deref(), Some("Ace"));
        assert_eq!(
            overwrite.applied.publisher.as_deref(),
            Some("Chilton"),
            "overwrite lets the source replace the value"
        );
    }

    #[test]
    fn agreeing_values_are_not_conflicts() {
        let current = EditionFields {
            publisher: Some(String::from("Ace")),
            published: Some(date(1965, Some(8), Some(1))),
            ..Default::default()
        };
        let incoming = Enrichment {
            publisher: Some(String::from("Ace")),
            published: Some(date(1965, Some(8), Some(1))),
            ..empty()
        };

        let merge = merged(&current, &incoming, EnrichMode::Overwrite);
        assert!(merge.preview.added.is_empty());
        assert!(merge.preview.conflicts.is_empty());
    }

    /// Dates compare at the precision they carry, so a year-only source date is
    /// a visible disagreement rather than an invisible one.
    #[test]
    fn published_dates_compare_at_their_rendered_precision() {
        let current = EditionFields {
            published: Some(date(1965, Some(8), Some(1))),
            ..Default::default()
        };
        let incoming = Enrichment {
            published: Some(date(1965, None, None)),
            ..empty()
        };

        let merge = merged(&current, &incoming, EnrichMode::Fill);
        assert_eq!(
            merge.preview.conflicts,
            vec![FieldConflict {
                field: String::from("published"),
                current: String::from("1965-08-01"),
                incoming: String::from("1965"),
            }]
        );
        assert_eq!(merge.applied.published, Some(date(1965, Some(8), Some(1))));
    }

    /// A differing `title_sort` used to vanish; it is now a conflict like any
    /// other scalar.
    #[test]
    fn a_differing_title_sort_is_a_conflict() {
        let current = EditionFields {
            title_sort: Some(String::from("Capital")),
            ..Default::default()
        };
        let incoming = Enrichment {
            title_sort: Some(String::from("Kapital, Das")),
            ..empty()
        };

        let merge = merged(&current, &incoming, EnrichMode::Overwrite);
        assert_eq!(
            merge.preview.conflicts[0].field, "title_sort",
            "{:?}",
            merge.preview.conflicts
        );
        assert_eq!(merge.applied.title_sort.as_deref(), Some("Kapital, Das"));
    }

    /// `fill` takes a list only when it is empty; `union`/`overwrite` append the
    /// missing members.
    #[test]
    fn fill_extends_a_list_only_when_it_is_empty() {
        let incoming = Enrichment {
            contributors: vec![contributor("Frank Herbert")],
            subjects: vec![String::from("Dune"), String::from("Desert")],
            ..empty()
        };

        let empty_current = merged(&EditionFields::default(), &incoming, EnrichMode::Fill);
        assert_eq!(
            empty_current.applied.subjects,
            vec![String::from("Dune"), String::from("Desert")]
        );

        let populated = EditionFields {
            contributors: vec![String::from("Someone Else")],
            subjects: vec![String::from("Fiction")],
            ..Default::default()
        };
        let kept = merged(&populated, &incoming, EnrichMode::Fill);
        // The preview still reports what the source offers: classification does
        // not depend on the mode, only the resolved write set does.
        assert_eq!(kept.preview.unioned.len(), 2, "{:?}", kept.preview.unioned);
        assert_eq!(kept.applied.subjects, vec![String::from("Fiction")]);
        assert_eq!(
            kept.applied.contributors,
            vec![String::from("Someone Else")]
        );
    }

    #[test]
    fn union_appends_missing_list_members_only() {
        let current = EditionFields {
            subjects: vec![String::from("Fiction")],
            contributors: vec![String::from("Frank Herbert")],
            ..Default::default()
        };
        let incoming = Enrichment {
            subjects: vec![
                String::from("Fiction"),
                String::from("Desert"),
                String::from("Desert"),
            ],
            contributors: vec![contributor("Frank Herbert"), contributor("Brian Herbert")],
            ..empty()
        };

        let merge = merged(&current, &incoming, EnrichMode::Union);
        assert_eq!(
            merge.applied.subjects,
            vec![String::from("Fiction"), String::from("Desert")]
        );
        assert_eq!(
            merge.applied.contributors,
            vec![String::from("Frank Herbert"), String::from("Brian Herbert")]
        );
        assert_eq!(
            merge.preview.unioned.len(),
            2,
            "{:?}",
            merge.preview.unioned
        );
    }

    /// No mode removes a list member: a source cannot know what to drop.
    #[test]
    fn overwrite_never_shrinks_a_list() {
        let current = EditionFields {
            subjects: vec![String::from("Local Tag"), String::from("Fiction")],
            contributors: vec![String::from("Curated Author")],
            ..Default::default()
        };
        let incoming = Enrichment {
            subjects: vec![String::from("Fiction")],
            contributors: vec![contributor("Source Author")],
            ..empty()
        };

        let merge = merged(&current, &incoming, EnrichMode::Overwrite);
        assert_eq!(
            merge.applied.subjects,
            vec![String::from("Local Tag"), String::from("Fiction")]
        );
        assert_eq!(
            merge.applied.contributors,
            vec![
                String::from("Curated Author"),
                String::from("Source Author")
            ]
        );
    }

    #[test]
    fn found_identifiers_are_reported_and_applied_in_every_mode() {
        let incoming = Enrichment {
            found: vec![Identifier {
                kind: String::from("olid"),
                value: String::from("OL123W"),
            }],
            ..empty()
        };

        for mode in [EnrichMode::Fill, EnrichMode::Union, EnrichMode::Overwrite] {
            let merge = merged(&EditionFields::default(), &incoming, mode);
            assert_eq!(merge.preview.found.len(), 1, "{mode:?}");
            assert_eq!(merge.applied.identifiers.len(), 1, "{mode:?}");
            assert_eq!(
                merge.preview.unioned[0].values,
                vec![String::from("olid:OL123W")],
                "{mode:?}"
            );
        }
    }

    #[test]
    fn a_cover_is_offered_only_when_the_edition_has_none() {
        let incoming = Enrichment {
            cover: Some(cover("new")),
            ..empty()
        };

        let missing = merge(
            &EditionFields::default(),
            &incoming,
            EnrichMode::Fill,
            &MergeContext {
                sources: vec![String::from("openlibrary")],
                cover_storable: true,
            },
        );
        assert_eq!(
            missing.preview.cover,
            Some(CoverOffer {
                mime: String::from("image/jpeg"),
                storable: true,
            })
        );
        assert!(missing.applied.cover.is_some());

        let present = merged(
            &EditionFields {
                has_cover: true,
                ..Default::default()
            },
            &incoming,
            EnrichMode::Overwrite,
        );
        assert_eq!(
            present.preview.cover, None,
            "an existing cover is never replaced"
        );
        assert!(present.applied.cover.is_none());
    }

    /// An edition with no inventory row cannot store a cover; the preview says
    /// so rather than dropping the offer silently.
    #[test]
    fn an_unstorable_cover_is_offered_but_not_applied() {
        let incoming = Enrichment {
            cover: Some(cover("new")),
            ..empty()
        };

        let merge = merge(
            &EditionFields::default(),
            &incoming,
            EnrichMode::Union,
            &MergeContext::default(),
        );
        assert_eq!(
            merge.preview.cover,
            Some(CoverOffer {
                mime: String::from("image/jpeg"),
                storable: false,
            })
        );
        assert!(merge.applied.cover.is_none());
    }

    #[test]
    fn nothing_to_change_produces_an_empty_preview() {
        let current = EditionFields {
            title: Some(String::from("Dune")),
            subjects: vec![String::from("Fiction")],
            has_cover: true,
            ..Default::default()
        };

        let merge = merged(&current, &empty(), EnrichMode::Overwrite);
        assert!(merge.preview.added.is_empty());
        assert!(merge.preview.unioned.is_empty());
        assert!(merge.preview.conflicts.is_empty());
        assert!(merge.preview.found.is_empty());
        assert_eq!(merge.preview.cover, None);
    }

    #[test]
    fn combine_prefers_the_first_scalar_and_merges_lists_in_order() {
        let first = Enrichment {
            title: Some(String::from("First")),
            publisher: None,
            subjects: vec![String::from("A"), String::from("B")],
            contributors: vec![contributor("Ann"), contributor("Ann")],
            found: vec![Identifier {
                kind: String::from("isbn"),
                value: String::from("1"),
            }],
            ..empty()
        };
        let second = Enrichment {
            title: Some(String::from("Second")),
            publisher: Some(String::from("Pluto")),
            subjects: vec![String::from("B"), String::from("C")],
            contributors: vec![contributor("Bob")],
            found: vec![
                Identifier {
                    kind: String::from("isbn"),
                    value: String::from("1"),
                },
                Identifier {
                    kind: String::from("olid"),
                    value: String::from("OL1W"),
                },
            ],
            ..empty()
        };

        let (sources, combined) = combine(vec![
            (String::from("first"), first),
            (String::from("second"), second),
        ]);

        assert_eq!(sources, vec![String::from("first"), String::from("second")]);
        assert_eq!(combined.title.as_deref(), Some("First"));
        assert_eq!(combined.publisher.as_deref(), Some("Pluto"));
        assert_eq!(
            combined.subjects,
            vec![String::from("A"), String::from("B"), String::from("C")]
        );
        let names: Vec<&str> = combined
            .contributors
            .iter()
            .map(|contributor| contributor.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["Ann", "Bob"],
            "same name and role collapses once"
        );
        assert_eq!(combined.found.len(), 2, "found dedupes by kind and value");
    }

    #[test]
    fn combine_of_nothing_is_an_empty_enrichment() {
        let (sources, combined) = combine(Vec::new());
        assert!(sources.is_empty());
        assert_eq!(combined, empty());
    }

    #[test]
    fn date_rendering_follows_the_precision_it_carries() {
        assert_eq!(format_date(&date(2024, Some(3), Some(7))), "2024-03-07");
        assert_eq!(format_date(&date(2024, Some(3), None)), "2024-03");
        assert_eq!(format_date(&date(2024, None, None)), "2024");
    }

    #[test]
    fn identity_carries_the_fields_an_enricher_keys_on() {
        let identity = identity_from(
            Some(String::from("Dune")),
            vec![String::from("Frank Herbert")],
            vec![Identifier {
                kind: String::from("isbn"),
                value: String::from("9780441013593"),
            }],
        );

        assert_eq!(identity.title.as_deref(), Some("Dune"));
        assert_eq!(identity.authors, vec![String::from("Frank Herbert")]);
        assert_eq!(identity.identifiers.len(), 1);
    }
}
