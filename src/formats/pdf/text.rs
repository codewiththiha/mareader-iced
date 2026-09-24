//! Turning Pdfium's per-character boxes into the runs the reader can use.
//!
//! The web app took pdf.js's text items — already runs, each with a transform
//! — and handed them to the search index as scale-1 rectangles in the reader's
//! own space (origin top-left, y growing down). Pdfium's high-level text
//! interface reports something different: one box per CHARACTER, in the page's
//! own space, where y grows *up* from the bottom-left. This module is the
//! whole of the difference — a flip, and a grouping pass that puts characters
//! back together into runs — and it is pure, so the rule that decides where a
//! run ends is a rule with tests rather than a claim about Pdfium.
//!
//! A run ends at any of three cues:
//!
//! * **a new line** — the next character's top is more than a fraction of a
//!   glyph height away from the run's, which is a line break or a superscript;
//! * **a different size** — a heading's characters must not join the paragraph
//!   beneath them, and a caption's must not join the body it annotates;
//! * **a gap wider than a space** — pdf.js split at the same seam, and a run
//!   that spans a table's column gap would give a search hit a highlight box
//!   that covers the whitespace between two cells.
//!
//! Grouping is coarser than pdf.js in one visible way, deliberately: pdf.js
//! also split at every font change, so a bold word inside a sentence produced
//! three items where this produces one. A coarser run costs a little precision
//! on a highlight rectangle and saves the reader a per-character index; the
//! character offsets, the snippet and the match count are unaffected, because
//! those are facts about the page's text, which come out the same either way.

/// One character as Pdfium measured it, already flipped into the reader's
/// space: origin at the page's top-left, y growing down, in PDF points (which
/// are CSS px at scale 1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawChar {
    /// The glyph. Pdfium answers `None` for a code point that is not a
    /// character (a broken font mapping); those arrive as U+FFFD so a run
    /// keeps its offsets honest rather than silently closing a gap.
    pub ch: char,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// The glyph's size, which is the ruler this module measures gaps against:
    /// a page set at 9 pt has different spaces than one set at 18 pt.
    pub size: f64,
}

/// The character that stands in for a glyph Pdfium could not map.
pub const UNMAPPED: char = char::REPLACEMENT_CHARACTER;

/// A vertical shift greater than this fraction of a glyph height ends the run:
/// a new line, or a marker lifted above the baseline.
const LINE_TOLERANCE: f64 = 0.45;

/// A gap wider than this fraction of the glyph size ends the run. A word space
/// is about a quarter of an em, and some text layers leave the space out of
/// the character list entirely (the advance survives, the glyph does not), so
/// the tolerance is generous: a little under one em keeps a missing space from
/// shredding a sentence into words, while the jump between two table columns —
/// which is wider than a word at any real gutter — still ends the run.
const GAP_TOLERANCE: f64 = 0.9;

/// A size change larger than this many points is a different kind of text.
/// A quarter of a point, because PDFs routinely spell one size 11.999 and
/// another 12.0, and a run must not split over a rounding artefact.
const SIZE_TOLERANCE: f64 = 0.25;

/// One run of text with every character's box united: what the search index
/// stores, what a hit's highlight is drawn from, and what a selection starts
/// as.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// The run's own glyph size, which is the size a highlight's padding and
    /// the gloss card's type are derived from.
    pub size: f64,
}

impl Run {
    fn of(ch: &RawChar) -> Self {
        Self {
            text: ch.ch.to_string(),
            x: ch.x,
            y: ch.y,
            w: ch.w,
            h: ch.h,
            size: ch.size,
        }
    }

    /// Grow the run over one more character's box.
    fn absorb(&mut self, ch: &RawChar) {
        let right = (self.x + self.w).max(ch.x + ch.w);
        let bottom = (self.y + self.h).max(ch.y + ch.h);
        self.x = self.x.min(ch.x);
        self.y = self.y.min(ch.y);
        self.w = right - self.x;
        self.h = bottom - self.y;
        self.text.push(ch.ch);
    }
}

/// Whether `ch` continues `run`, by the three cues above.
fn continues(run: &Run, ch: &RawChar) -> bool {
    let same_line = (ch.y - run.y).abs() <= LINE_TOLERANCE * ch.h.max(1.0);
    let same_size = (ch.size - run.size).abs() <= SIZE_TOLERANCE;
    // The gap is measured from the run's own right edge, which is why the
    // run keeps a united box rather than the last character's.
    let gap = ch.x - (run.x + run.w);
    // A character that overlaps the run's edge (a kerned pair, a ligature)
    // arrives with a negative gap and is the clearest continuation there is.
    same_line && same_size && gap <= GAP_TOLERANCE * ch.size.max(1.0)
}

/// Group characters into runs, in the order Pdfium reported them — which is
/// the reading order of the page's text operators, the same order pdf.js's
/// items arrived in.
pub fn runs(chars: &[RawChar]) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    for ch in chars {
        match runs.last_mut() {
            Some(run) if continues(run, ch) => run.absorb(ch),
            _ => runs.push(Run::of(ch)),
        }
    }
    runs
}

/// The reader's y for a box measured in Pdfium's space: the distance from the
/// page's top rather than from its bottom.
pub fn flip(top: f64, bottom: f64, page_height: f64) -> (f64, f64) {
    (page_height - top, page_height - bottom)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A line of text laid out left to right at one size, with the given
    /// per-character advances.
    fn line(chars: &[(char, f64)], x0: f64, y: f64, size: f64) -> Vec<RawChar> {
        let mut x = x0;
        let mut out = Vec::new();
        for (ch, advance) in chars {
            out.push(RawChar {
                ch: *ch,
                x,
                y,
                w: *advance,
                h: size,
                size,
            });
            x += advance;
        }
        out
    }

    fn word(text: &str, advance: f64) -> Vec<(char, f64)> {
        text.chars().map(|ch| (ch, advance)).collect()
    }

    #[test]
    fn a_line_of_words_is_one_run() {
        let chars = line(&word("Hello world", 10.0), 72.0, 100.0, 12.0);
        let runs = runs(&chars);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "Hello world");
        assert_eq!(runs[0].x, 72.0);
        assert!((runs[0].w - 110.0).abs() < 1e-9);
        assert_eq!(runs[0].size, 12.0);
    }

    #[test]
    fn a_word_space_is_not_a_break() {
        // Text layers that leave the space glyph out of the character list
        // still carry its advance: "ab cd" arrives as two pairs with a 3.4 pt
        // hole between them, and that hole is a word space, not a seam.
        let mut chars = line(&word("ab", 10.0), 0.0, 50.0, 12.0);
        chars.extend(line(&word("cd", 10.0), 23.4, 50.0, 12.0));
        assert_eq!(runs(&chars).len(), 1);
    }

    #[test]
    fn an_explicit_space_keeps_its_own_line_together() {
        // The ordinary case: Pdfium reports the space as a character, and the
        // whole sentence is one run.
        let chars = line(&word("two words", 6.0), 12.0, 90.0, 12.0);
        let runs = runs(&chars);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "two words");
    }

    #[test]
    fn a_column_gap_ends_the_run() {
        // Two cells of a table: the jump between them is wider than a space,
        // and a highlight must not reach across the gutter.
        let mut chars = line(&word("left", 10.0), 0.0, 50.0, 12.0);
        chars.extend(line(&word("right", 10.0), 200.0, 50.0, 12.0));
        let runs = runs(&chars);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].text, "left");
        assert_eq!(runs[1].text, "right");
        assert_eq!(runs[1].x, 200.0);
    }

    #[test]
    fn another_line_is_another_run() {
        let mut chars = line(&word("one", 10.0), 0.0, 100.0, 12.0);
        chars.extend(line(&word("two", 10.0), 0.0, 114.0, 12.0));
        let runs = runs(&chars);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[1].y, 114.0);
    }

    #[test]
    fn a_heading_does_not_join_the_paragraph_under_it() {
        let mut chars = line(&word("Title", 14.0), 0.0, 40.0, 20.0);
        chars.extend(line(&word("body", 10.0), 0.0, 80.0, 11.0));
        let runs = runs(&chars);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].size, 20.0);
        assert_eq!(runs[1].size, 11.0);
    }

    #[test]
    fn a_rounding_artefact_in_the_size_is_not_a_break() {
        // Two spellings of 12 pt inside one line: still one run.
        let mut chars = line(&word("abc", 10.0), 0.0, 50.0, 11.999);
        chars.extend(line(&word("def", 10.0), 30.0, 50.0, 12.0));
        assert_eq!(runs(&chars).len(), 1);
    }

    #[test]
    fn a_superscript_lifts_out_of_its_run() {
        // "x²": the marker sits above the baseline, past the line tolerance.
        let mut chars = line(&word("x", 10.0), 0.0, 100.0, 12.0);
        let mut raised = line(&word("2", 6.0), 10.0, 94.0, 8.0);
        raised[0].h = 12.0;
        chars.extend(raised);
        let runs = runs(&chars);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[1].text, "2");
    }

    #[test]
    fn an_unmapped_glyph_keeps_the_run_whole() {
        // Pdfium answers `None` for a code point that is not a character; the
        // run takes U+FFFD rather than dropping the glyph, so offsets stay
        // honest for everything after it.
        let mut chars = line(&word("a", 10.0), 0.0, 50.0, 12.0);
        chars.push(RawChar {
            ch: UNMAPPED,
            x: 10.0,
            y: 50.0,
            w: 10.0,
            h: 12.0,
            size: 12.0,
        });
        chars.extend(line(&word("b", 10.0), 20.0, 50.0, 12.0));
        let runs = runs(&chars);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "a\u{fffd}b");
    }

    #[test]
    fn nothing_in_is_nothing_out() {
        assert!(runs(&[]).is_empty());
    }

    #[test]
    fn a_flip_measures_from_the_top_of_the_page() {
        // Pdfium's box is measured up from the page's bottom; the reader's
        // space starts at its top. For an 800 pt page, a box 700..712 from the
        // bottom starts 88 pt from the top.
        let (y, bottom) = flip(712.0, 700.0, 800.0);
        assert_eq!(y, 88.0);
        assert_eq!(bottom, 100.0);
    }
}
