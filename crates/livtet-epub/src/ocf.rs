//! OCF container parsing: locating the OPF package document.

use crate::error::EpubError;
use crate::xml;
use crate::zip::{Archive, normalize};

pub(crate) const CONTAINER_PATH: &str = "META-INF/container.xml";

/// Resolve the archive path of the first `<rootfile>` that actually exists in
/// the archive. Missing entries are skipped (a known Kobo quirk), but if no
/// rootfile can be found the container is rejected rather than guessed at.
pub fn find_opf_path(archive: &mut Archive) -> Result<String, EpubError> {
    let bytes = archive
        .read(CONTAINER_PATH)
        .ok_or_else(|| EpubError::Container(format!("missing {CONTAINER_PATH}")))?;
    let root = xml::parse(&bytes)
        .map_err(|error| EpubError::Container(format!("container.xml: {error}")))?;

    let rootfiles = root
        .find("rootfiles")
        .ok_or_else(|| EpubError::Container("container.xml has no <rootfiles>".to_string()))?;

    let candidates: Vec<String> = rootfiles
        .find_all("rootfile")
        .into_iter()
        .filter_map(|rootfile| rootfile.attr("full-path"))
        .map(normalize)
        .filter(|path| !path.is_empty())
        .collect();

    if candidates.is_empty() {
        return Err(EpubError::Container(
            "container.xml declares no rootfile".to_string(),
        ));
    }

    for path in candidates {
        if archive.contains(&path) {
            return Ok(path);
        }
    }
    Err(EpubError::Container(
        "declared rootfile is not present in the archive".to_string(),
    ))
}

/// Directory portion of an OPF path (empty when the OPF is at the archive root).
pub fn base_dir(opf_path: &str) -> String {
    match opf_path.rfind('/') {
        Some(index) => opf_path[..index].to_string(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{EpubBuilder, default_container};

    fn archive_with(container: &str) -> Archive {
        Archive::open(EpubBuilder::new("<package/>").container(container).bytes()).unwrap()
    }

    #[test]
    fn finds_existing_rootfile() {
        let mut archive = archive_with(&default_container("OEBPS/content.opf"));
        assert_eq!(find_opf_path(&mut archive).unwrap(), "OEBPS/content.opf");
    }

    #[test]
    fn skips_rootfiles_missing_from_archive() {
        let container = r#"<?xml version="1.0"?>
<container version="1.0"><rootfiles>
  <rootfile full-path="missing/content.opf" media-type="application/oebps-package+xml"/>
  <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
</rootfiles></container>"#;
        let mut archive = archive_with(container);
        assert_eq!(find_opf_path(&mut archive).unwrap(), "OEBPS/content.opf");
    }

    #[test]
    fn errors_when_no_rootfile_exists() {
        let container = r#"<?xml version="1.0"?>
<container version="1.0"><rootfiles>
  <rootfile full-path="missing/content.opf" media-type="application/oebps-package+xml"/>
</rootfiles></container>"#;
        let mut archive = archive_with(container);
        assert!(matches!(
            find_opf_path(&mut archive),
            Err(EpubError::Container(_))
        ));
    }

    #[test]
    fn derives_base_directory() {
        assert_eq!(base_dir("OEBPS/content.opf"), "OEBPS");
        assert_eq!(base_dir("content.opf"), "");
        assert_eq!(base_dir("a/b/c.opf"), "a/b");
    }
}
