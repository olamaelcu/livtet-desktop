//! EPUB to Readium Web Publication streamer.
//!
//! [`Reader`] turns an EPUB file into the three pieces a Readium navigator
//! needs: a Readium Web Publication Manifest, a flat list of positions, and
//! resource bytes. It is the desktop app's own Streamer, so no third-party
//! Readium component parses the EPUB.

mod error;
mod package;
mod path;

pub use error::ReaderError;

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use livtet_epub::{Archive, ocf};

use package::{ManifestItem, Package};

/// A single reading position: one per reading-order resource.
#[derive(serde::Serialize, Clone, Debug)]
pub struct Location {
    pub href: String,
    #[serde(rename = "type")]
    pub media_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub locations: LocationDetail,
}

/// The location details Readium expects inside a [`Location`].
#[derive(serde::Serialize, Clone, Debug)]
pub struct LocationDetail {
    pub progression: f64,
    pub position: u32,
    #[serde(rename = "totalProgression")]
    pub total_progression: f64,
}

/// An open EPUB, parsed into a Readium Web Publication.
pub struct Reader {
    archive: Mutex<Archive>,
    base_dir: String,
    package: Package,
}

impl Reader {
    /// Open and parse an EPUB. Fails closed on an unreadable archive or a
    /// package with no reading order.
    pub fn open(path: &Path) -> Result<Self, ReaderError> {
        let bytes = std::fs::read(path)?;
        let mut archive = Archive::open(bytes)?;
        let opf_path = ocf::find_opf_path(&mut archive)?;
        let base_dir = ocf::base_dir(&opf_path);
        let file_name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("Untitled");
        let package = package::parse(&mut archive, &opf_path, file_name)?;
        Ok(Self {
            archive: Mutex::new(archive),
            base_dir,
            package,
        })
    }

    /// The Readium Web Publication Manifest, with the `rel=self` link pointing
    /// at `{base_url}manifest.json`.
    pub fn manifest(&self, base_url: &str) -> serde_json::Value {
        let package = &self.package;
        let mut metadata = serde_json::json!({
            "title": &package.title,
            "readingProgression": &package.reading_progression,
            "conformsTo": ["https://readium.org/webpub-manifest/profiles/epub"],
        });
        if !package.authors.is_empty() {
            metadata["author"] = serde_json::json!(&package.authors);
        }
        if !package.language.is_empty() {
            metadata["language"] = serde_json::json!(&package.language);
        }

        let reading_order: Vec<serde_json::Value> =
            package.reading_order.iter().map(link).collect();
        let resources: Vec<serde_json::Value> = package.resources.iter().map(link).collect();

        let mut manifest = serde_json::json!({
            "@context": ["https://readium.org/webpub-manifest/context.jsonld"],
            "metadata": metadata,
            "readingOrder": reading_order,
            "resources": resources,
            "links": [{"rel": ["self"], "href": format!("{base_url}manifest.json")}],
        });
        if !package.toc.is_empty() {
            let toc: Vec<serde_json::Value> = package
                .toc
                .iter()
                .map(|entry| serde_json::json!({ "href": &entry.href, "title": &entry.title }))
                .collect();
            manifest["toc"] = serde_json::json!(toc);
        }
        manifest
    }

    /// One [`Location`] per reading-order resource, with `progression` at the
    /// start of each resource.
    pub fn positions(&self) -> Vec<Location> {
        let order = &self.package.reading_order;
        let total = order.len();
        order
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let position = index as u32 + 1;
                Location {
                    href: item.href.clone(),
                    media_type: item.media_type.clone(),
                    title: self
                        .package
                        .toc
                        .iter()
                        .find(|entry| entry.href == item.href)
                        .map(|entry| entry.title.clone()),
                    locations: LocationDetail {
                        progression: 0.0,
                        position,
                        total_progression: index as f64 / total as f64,
                    },
                }
            })
            .collect()
    }

    /// Resolve an OCF-relative href to its media type and bytes.
    ///
    /// The href is percent-decoded first (archive names are decoded), then a
    /// traversal, absolute, backslash, or NUL-bearing result is rejected; an
    /// href that is not archive-root-relative is resolved against the OPF
    /// directory. `None` is returned for anything missing, encrypted, or
    /// refused by the archive.
    pub fn read(&self, href: &str) -> Option<(String, Vec<u8>)> {
        let candidate = self.candidate(href)?;
        let media_type = self
            .package
            .media_types
            .get(&candidate)
            .cloned()
            .or_else(|| path::media_type_for_extension(&candidate).map(str::to_string))
            .unwrap_or_else(|| "application/octet-stream".to_string());
        let bytes = self.lock_archive().read(&candidate)?;
        Some((media_type, bytes))
    }

    /// Whether the href exists in the archive, either as given or resolved
    /// against the OPF directory. The href is percent-decoded like [`read`].
    ///
    /// [`read`]: Reader::read
    pub fn contains(&self, href: &str) -> bool {
        self.candidate(href)
            .is_some_and(|candidate| self.exists_in_archive(&candidate))
    }

    /// The archive path for an href: percent-decoded and validated, then
    /// prefixed with the OPF directory when it is not already
    /// archive-root-relative.
    fn candidate(&self, href: &str) -> Option<String> {
        let decoded = path::decode_and_validate(href)?;
        Some(if self.exists_in_archive(&decoded) {
            decoded
        } else {
            path::resolve(&self.base_dir, &decoded)
        })
    }

    fn exists_in_archive(&self, name: &str) -> bool {
        self.lock_archive().contains(name)
    }

    /// A poisoned lock means a previous holder panicked; the archive itself is
    /// still intact, so recover it rather than reporting every later read as
    /// missing.
    fn lock_archive(&self) -> MutexGuard<'_, Archive> {
        self.archive
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn link(item: &ManifestItem) -> serde_json::Value {
    serde_json::json!({ "href": &item.href, "type": &item.media_type })
}

const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Reader>();
};
