//! The reader's search model, the scan both pipelines run, and the maths the
//! search UI runs on.
//!
//! Both pipelines answer in this shape — the PDF side from the engine's
//! page-text index (`pdf_core::search`), a reflowable document from
//! `reflow_core::search` — and the results list, the cycling and the scroll
//! reveal are the same code for either.
//!
//! The SCAN lives here because a match carries an ordinal and the painter
//! counts occurrences again, independently, to find which of its boxes that
//! ordinal names. Two scanners are two chances to disagree about what an
//! occurrence is, so there is one ([`occurrence_spans`]), with the snippet
//! window ([`snippet`]) beside it.
//!
//! The engine returns ONE ENTRY PER OCCURRENCE in document order, not one per
//! page. A match's rect is in scale-1 CSS px relative to its page's top-left;
//! the UI multiplies by the current scale to place it.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// Which occurrence of the query, in which block, of a document with no fixed
/// page grid.
///
/// The two halves of a [`SearchMatch`] answer "where is this hit" for the two
/// kinds of document. A page of pixels has a fixed grid, so a box in page
/// space IS an identity (`x`/`y`/`w`/`h`). A document the reader lays out
/// itself has no grid — every typography knob re-cuts its pages and a stored
/// box would point at whatever moved underneath — so its answer is the block
/// and the occurrence inside it, the same identity its gloss marks keep. The
/// painter re-finds the query in the block row's rendered text and numbers
/// occurrences in reading order, so the pair names one box on screen without
/// geometry — the same deal the engine's text-layer painter makes with
/// `page` + `index`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct BlockHit {
    pub block: u32,
    /// Which occurrence of the query inside that block, counting from zero.
    pub occurrence: u32,
}

/// One occurrence of the query.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchMatch {
    /// 1-based page holding this occurrence.
    pub page: u32,
    /// Ordinal of this occurrence WITHIN its page, in reading order. The engine
    /// stamps the same number onto the highlight box it paints, so this pair
    /// names one box on screen without matching geometry.
    pub index: u32,
    /// Snippet of surrounding text, for the results list. Shared so a
    /// 500-hit query does not clone 500 independent `String`s of the same
    /// haystack windows.
    pub text: Arc<str>,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// Where the hit sits in a document the reader lays out itself; `None` for a
    /// fixed-grid one, whose rect above is the whole answer. `#[serde(default)]`
    /// because the engine's response has never carried it and never will.
    #[serde(default)]
    pub block_hit: Option<BlockHit>,
}

/// `{ok:true, query, total, matches:[…]}` — engine.search().
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResponse {
    pub query: String,
    pub total: u32,
    pub matches: Vec<SearchMatch>,
}

/// Characters of context on each side of a hit in a results-list snippet. One
/// number for both families, because one dropdown shows both: the row clips
/// at 80 characters anyway (components/search/result_list.rs), so a wider
/// window is text the reader never sees.
pub const SNIPPET_RADIUS: usize = 32;

/// Every occurrence of `needle` in `haystack`, as character spans in reading
/// order: the one scan, called by the PDF's page-text index, by a reflowable
/// document's blocks, and by the layer that paints hits over a block's
/// rendered text.
///
/// `folded` is `haystack.to_lowercase()`, passed in because the hot caller
/// already holds it: the PDF index folds each page once at build and rescans
/// on every keystroke.
///
/// Matching is case-insensitive and non-overlapping, advancing by the
/// needle's length — "aa" in "aaa" is one hit, at 0 — matching
/// `str::match_indices` and the engine's painter. An empty or whitespace-only
/// needle matches nothing.
///
/// Case folding can change LENGTH ('İ' lowercases to two characters), and a
/// span counted in the folded copy would then not be a span of the text the
/// reader sees: when folding changed the character count the scan runs over
/// the ORIGINAL, case-sensitively. A missed hit is a smaller lie than a box
/// over characters nobody searched for.
pub fn occurrence_spans(haystack: &str, folded: &str, needle: &str) -> Vec<(usize, usize)> {
    let needle = needle.trim();
    if needle.is_empty() {
        return Vec::new();
    }
    // ASCII is the common case and the cheap one: folding is one character for
    // one, so byte offsets are character offsets and neither side is copied.
    if haystack.is_ascii() && folded.is_ascii() && needle.is_ascii() {
        let needle = needle.to_ascii_lowercase();
        let mut out = Vec::new();
        let mut at = 0;
        while let Some(found) = folded[at..].find(&needle) {
            let start = at + found;
            out.push((start, start + needle.len()));
            at = start + needle.len();
        }
        return out;
    }
    let lowered = needle.to_lowercase();
    let (text, want) = if folded.chars().count() == haystack.chars().count() {
        (folded, lowered.as_str())
    } else {
        (haystack, needle)
    };
    let chars: Vec<char> = text.chars().collect();
    let want: Vec<char> = want.chars().collect();
    if want.len() > chars.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut at = 0;
    while at + want.len() <= chars.len() {
        if chars[at..at + want.len()] == want[..] {
            out.push((at, at + want.len()));
            at += want.len();
        } else {
            at += 1;
        }
    }
    out
}

/// The context window around a hit for one results-list row:
/// [`SNIPPET_RADIUS`] characters either side of `[start, end)`, elided at the
/// edges the window does not reach, newlines folded to spaces (a row is one
/// line). Offsets are CHARACTERS — the spans [`occurrence_spans`] reports —
/// so no byte-boundary walking, and Latin prose and emoji read the same.
/// Casing is the original's: the scan runs over a folded copy, the reader
/// reads this.
pub fn snippet(text: &str, start: usize, end: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let from = start.saturating_sub(SNIPPET_RADIUS).min(chars.len());
    let to = (end + SNIPPET_RADIUS).min(chars.len()).max(from);
    let mut out: String = chars[from..to]
        .iter()
        .map(|&c| if c == '\n' { ' ' } else { c })
        .collect();
    if from > 0 {
        out.insert(0, '…');
    }
    if to < chars.len() {
        out.push('…');
    }
    out
}

/// Next active-result index with wrap-around. `dir > 0` forward, `dir < 0` back.
/// `active = None` → first (dir > 0) or last (dir < 0).
pub fn next_search_index(len: usize, active: Option<usize>, dir: i32) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match active {
        Some(i) if dir > 0 => (i + 1) % len,
        Some(i) if dir == 0 => i, // dir == 0 is a no-op: stay put
        Some(i) => (i + len - 1) % len,
        None if dir > 0 => 0,
        None if dir == 0 => return None, // dir == 0 with nothing active: no movement
        None => len - 1,
    })
}

/// Fraction of the reading area to leave above a match when scrolling it into
/// view, so it lands in comfortable reading position rather than jammed against
/// the top edge.
const MATCH_VIEW_BIAS: f64 = 0.35;

/// Scroll offset that brings a match into view, or `None` if it already is.
///
/// Targets the MATCH, not the page top: jumping to the page put a hit near
/// the bottom of a tall page off-screen. Deliberately lazy — while the match
/// is comfortably inside the reading area the view does not move, so stepping
/// through hits on one screen highlights in place instead of jerking.
///
/// Arguments are in the scroll container's coordinates: `match_top`/
/// `match_bot` are the match's edges within the column, `scroll_top` the
/// current offset, `viewport_h` the container height, `inset_top`/
/// `inset_bottom` the parts hidden behind the toolbar / search bar, and
/// `margin` keeps the match clear of those edges.
pub fn scroll_to_reveal(
    match_top: f64,
    match_bot: f64,
    scroll_top: f64,
    viewport_h: f64,
    inset_top: f64,
    inset_bottom: f64,
    margin: f64,
) -> Option<f64> {
    // The genuinely readable band, in scroll coordinates.
    let view_top = scroll_top + inset_top + margin;
    let view_bot = scroll_top + viewport_h - inset_bottom - margin;
    // A viewport too small for the insets (or a match taller than the band):
    // fall back to putting the match's top at the top of the readable area.
    if view_bot <= view_top || match_bot - match_top > view_bot - view_top {
        return Some((match_top - inset_top - margin).max(0.0));
    }
    if match_top >= view_top && match_bot <= view_bot {
        return None; // already comfortably visible — don't move
    }
    // Off-screen (or clipped): place it at the bias line, which reads better
    // than pinning it to whichever edge it left from.
    let band = view_bot - view_top;
    let target = match_top - inset_top - margin - band * MATCH_VIEW_BIAS;
    Some(target.max(0.0))
}

#[cfg(test)]
mod tests;
