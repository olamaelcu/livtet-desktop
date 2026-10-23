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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    /// Fake JPEG payload: passthrough must return these exact bytes.
    const JPEG: &[u8] = b"\xFF\xD8\xFF\xE0fake-jpeg-payload\xFF\xD9";

    /// A one-page PDF painting a DCT image (`jpeg_size` square) and an
    /// unfiltered DeviceRGB image (`raw_size` square).
    fn pdf_with_images(jpeg_size: u32, raw_size: u32) -> Vec<u8> {
        let raw_pixels = vec![0x80_u8; (raw_size * raw_size * 3) as usize];
        let content = b"q 100 0 0 100 0 0 cm /Im1 Do Q q 50 0 0 50 0 0 cm /Im2 Do Q".to_vec();
        let stream = |dict: String, data: &[u8]| {
            let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
            out.extend_from_slice(data);
            out.extend_from_slice(b"\nendstream");
            out
        };
        let objects: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /XObject << /Im1 5 0 R /Im2 6 0 R >> >> /Contents 4 0 R >>".to_vec(),
            stream(String::new(), &content),
            stream(
                format!("/Type /XObject /Subtype /Image /Width {jpeg_size} /Height {jpeg_size} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode"),
                JPEG,
            ),
            stream(
                format!("/Type /XObject /Subtype /Image /Width {raw_size} /Height {raw_size} /ColorSpace /DeviceRGB /BitsPerComponent 8"),
                &raw_pixels,
            ),
        ];

        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (index, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    #[test]
    fn largest_jpeg_image_passes_through_unchanged() {
        let bytes = pdf_with_images(40, 2);
        let doc = PdfDocument::from_bytes(bytes.clone()).unwrap();

        let cover = extract(&doc, &bytes).expect("embedded cover");
        assert_eq!(cover.mime, "image/jpeg");
        assert_eq!(cover.data, JPEG, "JPEG bytes are not re-encoded");
    }

    #[test]
    fn largest_non_jpeg_image_is_reencoded_to_png() {
        let bytes = pdf_with_images(1, 4);
        let doc = PdfDocument::from_bytes(bytes.clone()).unwrap();

        let cover = embedded(&doc).expect("embedded cover");
        assert_eq!(cover.mime, "image/png");
        assert!(cover.data.starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn render_fallback_yields_nothing_for_unparseable_bytes() {
        assert!(rendered(b"definitely not a pdf").is_none());
    }
}
