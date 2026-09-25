//! Front-cover resolution: an embedded image from page 0, or a render of page 0.
//!
//! Cover extraction is best-effort. Any failure — an encrypted document, an
//! undecodable image, a page with no content, a renderer that refuses the
//! file — yields [`None`] rather than an error. Ciphertext is never returned
//! as image bytes.

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{RenderCache, RenderSettings, render};
use livtet_importer_types::Cover;
use pdf_oxide::PdfDocument;
use pdf_oxide::extractors::images::PdfFilter;

/// Target width for a rendered cover, in pixels.
const RENDER_WIDTH: f32 = 600.0;

/// Resolve the cover: the largest embedded image on page 0, else a render of
/// page 0. `bytes` are the raw file contents, reused for the render fallback.
pub(crate) fn extract(doc: &PdfDocument, bytes: &[u8]) -> Option<Cover> {
    embedded(doc).or_else(|| rendered(bytes))
}

/// Largest image painted on page 0, passed through when it is a JPEG and
/// re-encoded to PNG otherwise.
fn embedded(doc: &PdfDocument) -> Option<Cover> {
    let handles = doc.page_image_handles(0).ok()?;
    let handle = handles
        .iter()
        .max_by_key(|handle| u64::from(handle.width) * u64::from(handle.height))?;
    if handle.filter_chain == [PdfFilter::DCTDecode] {
        let data = handle.raw_compressed_bytes().ok()?;
        Some(Cover {
            data,
            mime: "image/jpeg".to_string(),
        })
    } else {
        let data = handle.decode().ok()?.to_png_bytes().ok()?;
        Some(Cover {
            data,
            mime: "image/png".to_string(),
        })
    }
}

/// Render page 0 to a white-backed PNG, scaled to roughly [`RENDER_WIDTH`].
///
/// A `Pdf::new` decryption failure (an encrypted document) is just "no cover"
/// here; it must never become a hard error.
fn rendered(bytes: &[u8]) -> Option<Cover> {
    let pdf = Pdf::new(bytes.to_vec()).ok()?;
    let page = pdf.pages().iter().next()?;
    let (page_width, _page_height) = page.render_dimensions();
    if !(page_width.is_finite() && page_width > 0.0) {
        return None;
    }
    let scale = RENDER_WIDTH / page_width;
    let cache = RenderCache::new();
    let interpreter = InterpreterSettings::default();
    let settings = RenderSettings {
        x_scale: scale,
        y_scale: scale,
        bg_color: WHITE,
        ..RenderSettings::default()
    };
    let pixmap = render(page, &cache, &interpreter, &settings);
    let data = pixmap.into_png().ok()?;
    Some(Cover {
        data,
        mime: "image/png".to_string(),
    })
}
