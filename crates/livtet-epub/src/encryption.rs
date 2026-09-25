//! `META-INF/encryption.xml` parsing.
//!
//! OCF uses encryption.xml both for real content encryption and for the benign
//! "obfuscation" schemes publishers apply to fonts and images. We only need to
//! distinguish the two so a cover is never emitted as ciphertext.

use std::collections::HashMap;

use crate::cover::percent_decode;
use crate::xml::{self, Element};
use crate::zip::{Archive, normalize};

pub(crate) const ENCRYPTION_PATH: &str = "META-INF/encryption.xml";

/// Allowlisted *benign* obfuscation algorithms (IDPF fonts and Adobe PDF
/// assets). Compared exactly, case-insensitively, after trimming a trailing
/// `/`; anything else — including an unknown or malformed algorithm — is
/// treated as real encryption.
const OBFUSCATION_ALGORITHMS: [&str; 2] = [
    "http://www.idpf.org/2008/embedding",
    "http://ns.adobe.com/pdf/enc#rc",
];

/// Sentinel returned by [`Encryption::is_encrypted`] when the document exists
/// but could not be understood.
const UNKNOWN_ENCRYPTION: &str = "<unknown>";

/// Map of archive path → encryption algorithm.
#[derive(Debug, Default)]
pub(crate) struct Encryption {
    algorithms: HashMap<String, String>,
    /// Set when `encryption.xml` exists but could not be fully understood, so
    /// callers must assume an affected resource might be encrypted.
    unknown: bool,
}

impl Encryption {
    /// Load encryption metadata. A missing file means "nothing encrypted"; a
    /// present-but-unparseable file yields an [`Encryption`] that reports every
    /// path as encrypted (fail-closed) rather than silently returning none.
    pub fn load(archive: &mut Archive) -> Self {
        let Some(bytes) = archive.read(ENCRYPTION_PATH) else {
            return Self::default();
        };
        match xml::parse(&bytes) {
            Ok(root) => Self::from_root(&root),
            Err(_) => Self {
                algorithms: HashMap::new(),
                unknown: true,
            },
        }
    }

    fn from_root(root: &Element) -> Self {
        let mut algorithms = HashMap::new();
        let mut unknown = !root.local.eq_ignore_ascii_case("encryption");
        for data in root.find_all("EncryptedData") {
            let algorithm = data
                .find("EncryptionMethod")
                .and_then(|method| method.attr("Algorithm"));
            let uri = data
                .find("CipherReference")
                .and_then(|reference| reference.attr("URI"));
            match (algorithm, uri) {
                (Some(algorithm), Some(uri)) if !algorithm.trim().is_empty() => {
                    algorithms.insert(canonical_reference(uri), algorithm.trim().to_string());
                }
                // An EncryptedData we cannot attribute to a path might cover the
                // cover, so fail closed for the whole document.
                _ => unknown = true,
            }
        }
        Self {
            algorithms,
            unknown,
        }
    }

    /// The raw declared algorithm for a path, if any.
    pub fn algorithm(&self, path: &str) -> Option<&str> {
        self.algorithms
            .get(&canonical_reference(path))
            .map(String::as_str)
    }

    /// The algorithm when the path is *really* encrypted (anything that is not
    /// a known obfuscation scheme), or a sentinel when the document was opaque.
    pub fn is_encrypted(&self, path: &str) -> Option<&str> {
        if self.unknown {
            return Some(UNKNOWN_ENCRYPTION);
        }
        match self.algorithm(path) {
            Some(algorithm) if !is_obfuscation_algorithm(algorithm) => Some(algorithm),
            _ => None,
        }
    }
}

fn is_obfuscation_algorithm(algorithm: &str) -> bool {
    let normalized = algorithm.trim().trim_end_matches('/').to_ascii_lowercase();
    OBFUSCATION_ALGORITHMS.contains(&normalized.as_str())
}

/// Canonicalize a `CipherReference` URI so it matches the OPF-relative path the
/// cover resolver produces: percent-decode, normalize backslashes, strip a
/// leading `./` (or `/`).
fn canonical_reference(uri: &str) -> String {
    let decoded = percent_decode(uri);
    let slashed = decoded.replace('\\', "/");
    let trimmed = slashed.strip_prefix("./").unwrap_or(&slashed);
    normalize(trimmed.trim_start_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_algorithms(xml_doc: &str) -> Encryption {
        Encryption::from_root(&xml::parse(xml_doc.as_bytes()).unwrap())
    }

    /// An encryption document declaring `algorithm` for a font (not the cover).
    fn font_encryption(algorithm: &str) -> Encryption {
        parse_algorithms(&format!(
            r#"<encryption><EncryptedData>
  <EncryptionMethod Algorithm="{algorithm}"/>
  <CipherData><CipherReference URI="OEBPS/font.otf"/></CipherData>
</EncryptedData></encryption>"#
        ))
    }

    #[test]
    fn maps_uris_to_algorithms() {
        let encryption = parse_algorithms(
            r#"<encryption xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <EncryptedData xmlns="http://www.w3.org/2001/04/xmlenc#">
    <EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#aes128-cbc"/>
    <CipherData><CipherReference URI="OEBPS/cover.jpg"/></CipherData>
  </EncryptedData>
</encryption>"#,
        );
        assert_eq!(
            encryption.algorithm("OEBPS/cover.jpg"),
            Some("http://www.w3.org/2001/04/xmlenc#aes128-cbc")
        );
        assert!(encryption.is_encrypted("OEBPS/cover.jpg").is_some());
    }

    #[test]
    fn treats_known_obfuscation_algorithms_as_benign() {
        for algorithm in [
            "http://www.idpf.org/2008/embedding",
            "http://ns.adobe.com/pdf/enc#RC",
            "http://ns.adobe.com/pdf/enc#RC/", // trailing slash is trimmed
        ] {
            assert!(
                font_encryption(algorithm)
                    .is_encrypted("OEBPS/font.otf")
                    .is_none(),
                "{algorithm} should be treated as benign obfuscation"
            );
        }
    }

    #[test]
    fn treats_near_miss_algorithm_as_encrypted() {
        // The old substring check accepted any URI containing the Adobe prefix;
        // the exact allowlist must not.
        assert!(
            font_encryption("http://evil.example/ns.adobe.com/pdf/enc")
                .is_encrypted("OEBPS/font.otf")
                .is_some()
        );
    }

    #[test]
    fn normalizes_reference_paths_and_encoding() {
        let encryption = parse_algorithms(
            r#"<encryption><EncryptedData>
  <EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#aes128-cbc"/>
  <CipherData><CipherReference URI="./OEBPS/cover.jpg"/></CipherData>
</EncryptedData>
<EncryptedData>
  <EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#aes128-cbc"/>
  <CipherData><CipherReference URI="OEBPS/cover%2Ejpg"/></CipherData>
</EncryptedData></encryption>"#,
        );
        assert!(encryption.is_encrypted("OEBPS/cover.jpg").is_some());
    }

    #[test]
    fn unattributable_encrypted_data_marks_document_unknown() {
        let encryption = parse_algorithms("<encryption><EncryptedData>");
        assert!(encryption.algorithm("anything").is_none());
        assert!(encryption.is_encrypted("anything").is_some());
    }

    #[test]
    fn missing_file_is_empty() {
        let mut archive =
            Archive::open(crate::test_support::EpubBuilder::new("<package/>").bytes()).unwrap();
        let encryption = Encryption::load(&mut archive);
        assert!(encryption.algorithm("OEBPS/cover.jpg").is_none());
        assert!(encryption.is_encrypted("OEBPS/cover.jpg").is_none());
    }
}
