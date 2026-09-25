use std::path::Path;

use base64::{Engine, engine::general_purpose::STANDARD};
use stanchion::mlua;

use crate::{Importer, ImporterContributor, ImporterCover, ImporterMeta, ImporterPublicationDate};

/// Native PDF importer behind the shared contract.
pub struct PdfImporter;

impl Importer for PdfImporter {
    fn extensions(&self) -> mlua::Result<Vec<String>> {
        Ok(vec!["pdf".to_string()])
    }

    fn read_metadata(&self, path: String) -> mlua::Result<ImporterMeta> {
        let parsed = livtet_pdf::read_metadata(Path::new(&path)).map_err(mlua::Error::external)?;

        Ok(ImporterMeta {
            title: parsed.title.0,
            title_sort: parsed.title_sort,
            contributors: parsed
                .creators
                .into_iter()
                .map(|creator| ImporterContributor {
                    name: creator.name,
                    role: Some(crate::role_string(&creator.role)),
                    file_as: creator.file_as,
                })
                .collect(),
            isbns: parsed
                .isbns
                .iter()
                .map(|isbn| isbn.as_str().to_string())
                .collect(),
            other_identifiers: parsed
                .other_identifiers
                .into_iter()
                .map(|identifier| identifier.0)
                .collect(),
            publisher: parsed.publisher.map(|publisher| publisher.0),
            language: parsed.language.map(|language| language.0),
            published: parsed.published.map(|published| ImporterPublicationDate {
                year: published.year,
                month: published.month,
                day: published.day,
            }),
            description: parsed.description.map(|description| description.0),
            subjects: parsed
                .subjects
                .into_iter()
                .map(|subject| subject.0)
                .collect(),
            cover: parsed.cover.map(|cover| ImporterCover {
                mime: cover.mime,
                data_base64: STANDARD.encode(cover.data),
            }),
            format_metadata: parsed.format_metadata,
        })
    }
}
