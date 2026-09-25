//! Drawing a page and lifting its text: the two answers rendered in device
//! pixels and in the reader's own space.

use pdfium_render::prelude::{PdfBitmapFormat, PdfDocument, PdfRenderConfig};
use super::read::{page_of};
use crate::formats::pdf::protocol::FrameKey;
use crate::formats::pdf::text::{self, RawChar, Run};

/// Rasterise one page into the key's device-pixel box.
///
/// The page's own rotation is **not** applied here, deliberately, and that is
/// worth stating because applying it looks reasonable and is wrong: Pdfium's
/// `FPDF_GetPageWidthF`/`HeightF` already answer the rotated geometry, and
/// `FPDF_RenderPageBitmap` with `rotate = 0` draws the page upright in that
/// box. Passing the page's rotation through as well turns `/Rotate 90` into a
/// half turn — the fixture's third page and the ctypes verifier beside it exist
/// to keep that from coming back.
pub(super) fn frame(document: &PdfDocument<'_>, key: FrameKey) -> Result<(u32, u32, Vec<u8>), String> {
    let page = page_of(document, key.page)?;
    let config = PdfRenderConfig::new()
        // Both sides, not just the width: the raster is allocated at exactly
        // the box the host is about to draw, so no side is left for Pdfium to
        // choose and the aspect cannot drift from the rectangle it was
        // measured for.
        .set_target_size(key.width as i32, key.height as i32)
        // Spelled out rather than left to the default, because the whole
        // pipeline downstream assumes it — and because `as_rgba_bytes()` below
        // is only a pass-through when the format really is BGRA.
        .set_format(PdfBitmapFormat::BGRA);
    let bitmap = page
        .render_with_config(&config)
        .map_err(|error| error.to_string())?;
    // The bitmap's own size, because that is what the texture is allocated
    // with. It is the requested box in every case but a cap, and reporting the
    // truth is what keeps a capped raster from being drawn as a distorted page.
    let width = bitmap.width().max(0) as u32;
    let height = bitmap.height().max(0) as u32;
    Ok((width, height, bitmap.as_rgba_bytes()))
}

/// One page's text, grouped into runs in the reader's own space — see
/// [`super::text`] for the flip and the grouping rule.
pub(super) fn text_of(document: &PdfDocument<'_>, page: u32) -> Result<Vec<Run>, String> {
    let page = page_of(document, page)?;
    let page_height = f64::from(page.height().value);
    let text = page.text().map_err(|error| error.to_string())?;
    let chars = text.chars();
    // `len()` is already a `usize` (Pdfium's own character index type), so
    // this is a capacity hint and nothing else.
    let mut raw = Vec::with_capacity(chars.len());
    for ch in chars.iter() {
        // A character Pdfium cannot box is skipped rather than inserted at the
        // origin: one broken glyph would otherwise drag a run's rectangle
        // across the whole page.
        let Ok(bounds) = ch.loose_bounds() else {
            continue;
        };
        let (y, _) = text::flip(
            f64::from(bounds.top().value),
            f64::from(bounds.bottom().value),
            page_height,
        );
        raw.push(RawChar {
            ch: ch.unicode_char().unwrap_or(text::UNMAPPED),
            x: f64::from(bounds.left().value),
            y,
            w: f64::from(bounds.width().value),
            h: f64::from(bounds.height().value),
            size: f64::from(ch.scaled_font_size().value),
        });
    }
    Ok(text::runs(&raw))
}
