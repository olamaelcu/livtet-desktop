//! Map an `mp4ameta` tag onto the shared parser-side metadata model.

use mp4ameta::{ImgFmt, Tag};

use crate::{AudioError, Result};
use livtet_importer_types::{
    Contributor, Cover, Description, Identifier, PublicationDate, Publisher, Role, SourceMetadata,
    Subject, Title,
};

/// Build [`SourceMetadata`] from an already-read MP4 tag.
///
/// `title_fallback` (usually the file stem) is used only when the tag carries
/// neither a title nor an album. The composer (`©wrt`) maps to the narrator:
/// in audiobook MP4s that atom conventionally carries the narrator, and this
/// importer only claims audiobook extensions. The purchase comment (`©cmt`)
/// is never a description. Chapters prefer the chapter track and fall back to
/// the `chpl` list; quirks in chapter bounds are clamped, never fatal.
pub(crate) fn from_tag(tag: &Tag, title_fallback: Option<&str>) -> Result<SourceMetadata> {
    let title = tag
        .title()
        .or_else(|| tag.album())
        .or(title_fallback)
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .ok_or(AudioError::MissingRequired("title"))?;

    let mut creators: Vec<Contributor> = Vec::new();
    for artist in tag.artists() {
        push_contributor(&mut creators, artist, Role::Author);
    }
    for composer in tag.composers() {
        push_contributor(&mut creators, composer, Role::Narrator);
    }
    if creators.is_empty() {
        return Err(AudioError::MissingRequired("creator"));
    }

    let duration_seconds = i32::try_from(tag.duration().as_secs()).map_err(|_| {
        AudioError::Invalid("audiobook duration exceeds the supported range".to_string())
    })?;

    Ok(SourceMetadata {
        title: Title(title.to_string()),
        title_sort: tag.title_sort_order().map(str::to_string),
        creators,
        isbns: Vec::new(),
        other_identifiers: asins(tag),
        publisher: tag.label().map(|label| Publisher(label.to_string())),
        language: None,
        published: tag.year().and_then(PublicationDate::parse),
        description: tag
            .description()
            .map(|description| Description(description.to_string())),
        subjects: subjects(tag),
        cover: cover(tag),
        format_metadata: Some(format_metadata(tag, duration_seconds)?),
    })
}

/// Append a trimmed, non-empty, not-yet-seen `(name, role)` contributor.
fn push_contributor(creators: &mut Vec<Contributor>, name: &str, role: Role) {
    let name = name.trim();
    if name.is_empty()
        || creators
            .iter()
            .any(|creator| creator.name == name && creator.role == role)
    {
        return;
    }
    creators.push(Contributor {
        name: name.to_string(),
        role,
        file_as: None,
    });
}

/// ASIN-style identifiers from the iTunes freeform atom, preserved verbatim.
fn asins(tag: &Tag) -> Vec<Identifier> {
    let ident = mp4ameta::FreeformIdent::new_static("com.apple.iTunes", "ASIN");
    tag.strings_of(&ident)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| Identifier(value.to_string()))
        .collect()
}

/// Genre atoms as subjects, custom (`©gen`) first, deduplicated in order.
fn subjects(tag: &Tag) -> Vec<Subject> {
    let mut subjects: Vec<Subject> = Vec::new();
    for genre in tag.custom_genres().chain(tag.genres()) {
        let genre = genre.trim();
        if !genre.is_empty() && !subjects.iter().any(|subject| subject.0 == genre) {
            subjects.push(Subject(genre.to_string()));
        }
    }
    subjects
}

/// First attached artwork, with the MIME type sniffed from its format.
fn cover(tag: &Tag) -> Option<Cover> {
    let image = tag.artwork()?;
    let mime = match image.fmt {
        ImgFmt::Jpeg => "image/jpeg",
        ImgFmt::Png => "image/png",
        ImgFmt::Bmp => "image/bmp",
    };
    Some(Cover {
        data: image.data.to_vec(),
        mime: mime.to_string(),
    })
}

/// The `FormatMetadataSchema::Audiobook` value for this tag: total duration
/// plus one entry per chapter, each ending where the next begins (the last
/// ends at the track duration). Bounds are clamped into `[0, duration]`; a
/// chapter that would otherwise invert keeps a zero length.
fn format_metadata(tag: &Tag, duration_seconds: i32) -> Result<serde_json::Value> {
    let source = if tag.chapter_track().is_empty() {
        tag.chapter_list()
    } else {
        tag.chapter_track()
    };
    let duration = u64::try_from(duration_seconds).unwrap_or(0);
    let mut chapters = Vec::with_capacity(source.len());
    for (index, chapter) in source.iter().enumerate() {
        let start = chapter.start.as_secs().min(duration);
        let end = source
            .get(index + 1)
            .map(|next| next.start.as_secs())
            .unwrap_or(duration)
            .clamp(start, duration);
        let (start, end) = (
            i32::try_from(start).map_err(|_| {
                AudioError::Invalid(
                    "audiobook chapter start exceeds the supported range".to_string(),
                )
            })?,
            i32::try_from(end).map_err(|_| {
                AudioError::Invalid("audiobook chapter end exceeds the supported range".to_string())
            })?,
        );
        chapters.push(serde_json::json!({
            "name": chapter.title,
            "audio_start": start,
            "audio_end": end,
        }));
    }
    Ok(serde_json::json!({
        "duration_seconds": duration_seconds,
        "chapters": chapters,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use livtet_importer_types::Role;
    use mp4ameta::{AudioInfo, Chapter, Img, ImgFmt, Userdata};
    use std::time::Duration;

    fn blank_tag() -> Tag {
        Tag {
            ftyp: String::new(),
            info: AudioInfo::default(),
            userdata: Userdata::default(),
        }
    }

    fn tagged() -> Tag {
        let mut tag = blank_tag();
        tag.set_title("I'm Glad My Mom Died");
        tag.set_artist("Jennette McCurdy");
        tag.set_composer("Jennette McCurdy");
        tag.set_year("2022");
        tag.set_title_sort_order("Glad My Mom Died, I'm");
        tag.set_description("A memoir.");
        tag.set_custom_genre("Biography");
        tag
    }

    #[test]
    fn maps_title_author_narrator_date_and_sort() {
        let meta = from_tag(&tagged(), None).expect("tagged audiobook maps");

        assert_eq!(meta.title.0, "I'm Glad My Mom Died");
        assert_eq!(meta.title_sort.as_deref(), Some("Glad My Mom Died, I'm"));
        let creators: Vec<(&str, &Role)> = meta
            .creators
            .iter()
            .map(|creator| (creator.name.as_str(), &creator.role))
            .collect();
        assert!(creators.contains(&("Jennette McCurdy", &Role::Author)));
        assert!(creators.contains(&("Jennette McCurdy", &Role::Narrator)));
        let published = meta.published.expect("year maps");
        assert_eq!(
            (published.year, published.month, published.day),
            (2022, None, None)
        );
        assert_eq!(
            meta.description
                .as_ref()
                .map(|description| description.0.as_str()),
            Some("A memoir.")
        );
        assert_eq!(
            meta.subjects
                .iter()
                .map(|subject| subject.0.as_str())
                .collect::<Vec<_>>(),
            vec!["Biography"]
        );
    }

    #[test]
    fn falls_back_to_album_then_filename_for_a_missing_title() {
        let mut tag = tagged();
        tag.remove_title();
        tag.set_album("Album Title");

        let meta = from_tag(&tag, Some("file-stem")).expect("album title maps");
        assert_eq!(meta.title.0, "Album Title");

        tag.remove_album();
        let meta = from_tag(&tag, Some("file-stem")).expect("file stem maps");
        assert_eq!(meta.title.0, "file-stem");
    }

    #[test]
    fn rejects_a_missing_title() {
        let tag = blank_tag();

        let error = from_tag(&tag, None).expect_err("a title is required");
        assert!(
            matches!(error, AudioError::MissingRequired("title")),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn requires_a_creator() {
        let mut tag = blank_tag();
        tag.set_title("Title Only");

        let error = from_tag(&tag, None).expect_err("a creator is required");
        assert!(
            matches!(error, AudioError::MissingRequired("creator")),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn maps_chapters_with_end_times() {
        let mut tag = tagged();
        tag.info.duration = Duration::from_secs(900);
        tag.chapter_list_mut().extend([
            Chapter::new(Duration::ZERO, "Part One"),
            Chapter::new(Duration::from_secs(300), "Part Two"),
        ]);

        let meta = from_tag(&tag, None).expect("chapters map");
        let format = meta.format_metadata.expect("format metadata is set");
        assert_eq!(
            format,
            serde_json::json!({
                "duration_seconds": 900,
                "chapters": [
                    {"name": "Part One", "audio_start": 0, "audio_end": 300},
                    {"name": "Part Two", "audio_start": 300, "audio_end": 900},
                ],
            })
        );
    }

    #[test]
    fn prefers_the_chapter_track_over_the_chapter_list() {
        let mut tag = tagged();
        tag.info.duration = Duration::from_secs(600);
        tag.chapter_list_mut()
            .push(Chapter::new(Duration::ZERO, "List Chapter"));
        tag.chapter_track_mut()
            .push(Chapter::new(Duration::ZERO, "Track Chapter"));

        let meta = from_tag(&tag, None).expect("chapter track wins");
        let format = meta.format_metadata.expect("format metadata is set");
        assert_eq!(format["chapters"][0]["name"], "Track Chapter");
        assert_eq!(format["chapters"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn maps_cover_artwork() {
        let mut tag = tagged();
        tag.set_artwork(Img::new(ImgFmt::Jpeg, vec![1, 2, 3, 4]));

        let meta = from_tag(&tag, None).expect("cover maps");
        let cover = meta.cover.expect("cover is set");
        assert_eq!(cover.mime, "image/jpeg");
        assert_eq!(cover.data, vec![1, 2, 3, 4]);
    }

    #[test]
    fn purchase_comments_are_not_descriptions() {
        let mut tag = tagged();
        tag.set_comment("Jennette McCurdy Purchased from Libro.fm.");
        tag.remove_descriptions();

        let meta = from_tag(&tag, None).expect("comment is not a description");
        assert!(meta.description.is_none());
    }

    #[test]
    fn format_metadata_validates_against_the_audiobook_schema() {
        let mut tag = tagged();
        tag.info.duration = Duration::from_secs(7_200);
        tag.chapter_list_mut()
            .push(Chapter::new(Duration::ZERO, "Intro"));

        let meta = from_tag(&tag, None).expect("metadata maps");
        let format = meta.format_metadata.expect("format metadata is set");
        livtet_types::FormatMetadataSchema::Audiobook
            .validate(&format)
            .expect("audiobook format metadata validates");
    }
}
