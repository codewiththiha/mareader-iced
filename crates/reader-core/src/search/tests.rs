//! The search's own cases: where a term lands, and which view it scrolls to.

use super::*;

/// Cycling through results: wrap in both directions, start at the first or
/// last when nothing is active, and treat a single result as its own
/// neighbour. `dir` carries only a sign, so a larger stride behaves the same.
#[test]
fn cycles_and_wraps() {
    // (len, active, dir, expected)
    let cases: &[(usize, Option<usize>, i32, Option<usize>)] = &[
        (3, Some(2), 1, Some(0)),
        (3, Some(0), -1, Some(2)),
        (3, Some(1), 1, Some(2)),
        (3, Some(1), -1, Some(0)),
        (3, Some(0), 1, Some(1)),
        (3, None, 1, Some(0)),
        (3, None, -1, Some(2)),
        (1, Some(0), 1, Some(0)),
        (1, Some(0), -1, Some(0)),
        (1, None, 1, Some(0)),
        (1, None, -1, Some(0)),
    ];
    for &(len, active, dir, want) in cases {
        assert_eq!(next_search_index(len, active, dir), want, "len={len} active={active:?} dir={dir}");
    }
}

/// No results means nothing to select, and `dir == 0` means stay put.
#[test]
fn empty_results_and_zero_direction() {
    assert_eq!(next_search_index(0, None, 1), None);
    assert_eq!(next_search_index(0, None, -1), None);
    assert_eq!(next_search_index(0, Some(0), 1), None);
    assert_eq!(next_search_index(3, Some(1), 0), Some(1));
    assert_eq!(next_search_index(1, Some(0), 0), Some(0));
    assert_eq!(next_search_index(3, None, 0), None);
}

/// A match already sitting in the readable band does not move the view.
/// This is what lets several hits on one screen light up one after another
/// without the page twitching.
#[test]
fn visible_match_does_not_scroll() {
    // scroll 600, viewport 800, top inset 48, bottom inset 56, margin 24
    // => readable band spans 672..1320 in scroll coordinates.
    assert_eq!(scroll_to_reveal(700.0, 720.0, 600.0, 800.0, 48.0, 56.0, 24.0), None);
    // Flush against each edge of the band is still "visible".
    assert_eq!(scroll_to_reveal(672.0, 700.0, 600.0, 800.0, 48.0, 56.0, 24.0), None);
    assert_eq!(scroll_to_reveal(1290.0, 1320.0, 600.0, 800.0, 48.0, 56.0, 24.0), None);
}

/// A match under the fold, or hidden behind the top chrome, is brought to
/// the bias line — NOT to the edge it left from, and never past 0.
#[test]
fn offscreen_match_scrolls_to_the_bias_line() {
    let band = 800.0 - 48.0 - 56.0 - 2.0 * 24.0; // 624
    // Far below the fold.
    let want = 5000.0 - 48.0 - 24.0 - band * MATCH_VIEW_BIAS;
    assert_eq!(
        scroll_to_reveal(5000.0, 5020.0, 600.0, 800.0, 48.0, 56.0, 24.0),
        Some(want)
    );
    // Above the band (scrolled past): comes back to the same bias line.
    let want_up = 100.0 - 48.0 - 24.0 - band * MATCH_VIEW_BIAS;
    assert_eq!(
        scroll_to_reveal(100.0, 120.0, 600.0, 800.0, 48.0, 56.0, 24.0),
        Some(want_up.max(0.0))
    );
    // Near the very top of the document: clamped, never negative.
    assert!(scroll_to_reveal(10.0, 30.0, 600.0, 800.0, 48.0, 56.0, 24.0).unwrap() >= 0.0);
}

/// A match partly clipped by the bottom edge counts as not visible: the
/// bug being fixed is precisely "the hit is on this page but off-screen".
#[test]
fn clipped_match_is_revealed() {
    // Band is 672..1320; this straddles the bottom edge, so the reader can
    // only see part of the hit.
    assert!(scroll_to_reveal(1300.0, 1360.0, 600.0, 800.0, 48.0, 56.0, 24.0).is_some());
    // And this one is clipped by the top chrome.
    assert!(scroll_to_reveal(650.0, 690.0, 600.0, 800.0, 48.0, 56.0, 24.0).is_some());
}

/// The engine's JSON has no `block_hit` in it, and a search response that
/// failed to deserialize would take the whole feature down — so the field is
/// optional on the wire and reads as `None` for a PDF.
#[test]
fn a_match_from_the_engine_deserializes_without_a_block_hit() {
    let json = r#"{"query":"dune","total":1,"matches":[
        {"page":4,"index":0,"text":"…the dune sea…","x":12.0,"y":80.5,"w":30.0,"h":9.0}
    ]}"#;
    let response: SearchResponse = serde_json::from_str(json).unwrap();
    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].block_hit, None);

    // And a reflowable match round-trips the half a PDF never sends.
    let hit = BlockHit { block: 17, occurrence: 2 };
    let json = serde_json::to_string(&hit).unwrap();
    assert_eq!(serde_json::from_str::<BlockHit>(&json).unwrap(), hit);
}

/// The scan both pipelines and the highlight painter share: reading order,
/// non-overlapping, and counting CHARACTERS — an emoji is one of them, so a
/// hit after it starts where the reader would count, not where its bytes are.
#[test]
fn occurrences_are_numbered_in_reading_order_without_overlapping() {
    let folded = |s: &str| s.to_lowercase();
    let spans = |hay: &str, needle: &str| occurrence_spans(hay, &folded(hay), needle);

    assert_eq!(spans("The Dune of Dune", "dune"), vec![(4, 8), (12, 16)]);
    // One hit, not two: the first consumes the characters the second would
    // have started on.
    assert_eq!(spans("aaa", "aa"), vec![(0, 2)]);
    assert_eq!(spans("ab\u{1F600}cd dune", "dune"), vec![(6, 10)]);
    assert_eq!(spans("héllo wörld", "HÉLLO"), vec![(0, 5)]);
    // Nothing to search for, nothing found — including a query of spaces.
    assert!(spans("anything", "").is_empty());
    assert!(spans("anything", "   ").is_empty());
    // A padded query still matches, at the trimmed needle's length.
    assert_eq!(spans("a target here", " target "), vec![(2, 8)]);
}

/// 'İ' folds to two characters, so the folded copy's offsets are not the
/// original's. The scan refuses to guess: it drops back to a
/// case-sensitive read of the text the reader actually sees.
#[test]
fn a_fold_that_changes_length_never_reports_a_moved_offset() {
    let text = "İstanbul dune";
    let folded = text.to_lowercase();
    assert_ne!(folded.chars().count(), text.chars().count());
    // The case-sensitive fallback still finds the exact hit, at the offset
    // the ORIGINAL text has.
    assert_eq!(occurrence_spans(text, &folded, "dune"), vec![(9, 13)]);
    // And it does not pretend to a case-insensitive one it cannot place.
    assert!(occurrence_spans(text, &folded, "DUNE").is_empty());
}

/// The window the results list shows: original casing, newlines folded, and
/// an ellipsis only on the edges it actually cut.
#[test]
fn the_snippet_window_elides_only_the_edges_it_cuts() {
    let long = format!("{}target{}", "x".repeat(200), "y".repeat(200));
    let (start, end) = occurrence_spans(&long, &long.to_lowercase(), "target")[0];
    let s = snippet(&long, start, end);
    assert!(s.starts_with('…') && s.ends_with('…'), "{s}");
    assert_eq!(s.chars().count(), SNIPPET_RADIUS + 6 + SNIPPET_RADIUS + 2);

    // A hit at the very start has no left edge to elide.
    let head = format!("target{}", "y".repeat(200));
    let (start, end) = occurrence_spans(&head, &head.to_lowercase(), "target")[0];
    let s = snippet(&head, start, end);
    assert!(!s.starts_with('…'), "{s}");
    assert!(s.ends_with('…'));

    // The whole text fits: no ellipses, casing kept, newlines folded.
    let folded_newlines = "alpha\nbeta GAMMA delta";
    let (start, end) =
        occurrence_spans(folded_newlines, &folded_newlines.to_lowercase(), "gamma")[0];
    assert_eq!(snippet(folded_newlines, start, end), "alpha beta GAMMA delta");
}

/// Degenerate geometry must still produce a usable offset rather than
/// panicking or returning None: a match taller than the band, and a
/// viewport smaller than its own insets.
#[test]
fn degenerate_geometry_falls_back_to_top_alignment() {
    assert_eq!(
        scroll_to_reveal(2000.0, 4000.0, 0.0, 800.0, 48.0, 56.0, 24.0),
        Some(2000.0 - 48.0 - 24.0)
    );
    assert_eq!(
        scroll_to_reveal(2000.0, 2020.0, 0.0, 60.0, 48.0, 56.0, 24.0),
        Some(2000.0 - 48.0 - 24.0)
    );
}
