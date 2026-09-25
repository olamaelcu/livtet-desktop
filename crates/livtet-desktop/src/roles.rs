//! Display labels for `edition_authors.role` values.
//!
//! The role column holds three vocabularies depending on the writer:
//! canonical [`ContributorRole`] slugs (`"author"`), their URNs
//! (`"urn:livtet:contrib/600"`), and raw MARC relator codes (`"aut"`,
//! `"bkp"`) written by the file importers. [`role_label`] resolves all three
//! to a human-readable label and passes unknown values through unchanged so
//! future roles stay visible.

use livtet_types::ContributorRole;

/// Human-readable label for an `edition_authors.role` value.
pub fn role_label(role: &str) -> String {
    let trimmed = role.trim();
    let lower = trimmed.to_ascii_lowercase();
    let code = lower.strip_prefix("urn:livtet:contrib/").unwrap_or(&lower);
    if let Some(canonical) = canonical_role(code) {
        return canonical.name().to_string();
    }
    if let Some(label) = marc_relator_label(code) {
        return label.to_string();
    }
    trimmed.to_string()
}

/// Map a canonical slug or its URN discriminant to a [`ContributorRole`].
fn canonical_role(code: &str) -> Option<ContributorRole> {
    use ContributorRole::*;
    Some(match code {
        "author" | "aut" | "600" => Author,
        "translator" | "trl" | "601" => Translator,
        "editor" | "edt" | "602" => Editor,
        "illustrator" | "ill" | "603" => Illustrator,
        "narrator" | "nrt" | "604" => Narrator,
        "intro_author" | "aui" | "605" => IntroAuthor,
        "foreword_author" | "fwd" | "606" => ForewordAuthor,
        "epilogue_author" | "aft" | "607" => EpilogueAuthor,
        _ => return None,
    })
}

/// MARC relator codes that have no canonical [`ContributorRole`] equivalent.
fn marc_relator_label(code: &str) -> Option<&'static str> {
    Some(match code {
        "bkp" => "Book producer",
        "ctb" => "Contributor",
        "com" => "Compiler",
        "edc" => "Editor of compilation",
        "pbl" => "Publisher",
        "oth" => "Other",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_marc_relator_codes() {
        assert_eq!(role_label("aut"), "Author");
        assert_eq!(role_label("edt"), "Editor");
        assert_eq!(role_label("trl"), "Translator");
        assert_eq!(role_label("ill"), "Illustrator");
        assert_eq!(role_label("bkp"), "Book producer");
        assert_eq!(role_label("ctb"), "Contributor");
    }

    #[test]
    fn maps_canonical_slugs_and_urns() {
        assert_eq!(role_label("author"), "Author");
        assert_eq!(role_label("intro_author"), "Intro Author");
        assert_eq!(role_label("urn:livtet:contrib/600"), "Author");
        assert_eq!(role_label("URN:LIVTET:CONTRIB/601"), "Translator");
    }

    #[test]
    fn passes_unknown_roles_through() {
        assert_eq!(role_label("xyz"), "xyz");
        assert_eq!(role_label("  ctb  "), "Contributor");
        assert_eq!(role_label(""), "");
    }
}
