use std::fs::File;
use std::io::BufReader;

use epub::doc::EpubDoc;

/// Front-cover image bytes and media type as declared in the manifest.
#[derive(Debug, Clone)]
pub struct Cover {
    pub data: Vec<u8>,
    pub mime: String,
}

pub(crate) fn extract(doc: &mut EpubDoc<BufReader<File>>) -> Option<Cover> {
    let (data, mime) = doc.get_cover()?;
    Some(Cover { data, mime })
}
