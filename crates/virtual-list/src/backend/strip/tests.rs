//! The `Strip` API's own cases.

use super::*;
use crate::units::{SUBPIXEL_FACTOR, from_sub, to_sub};
use crate::window::{Budget, Window};

/// Tolerance used everywhere an `i64`-derived `f64` (from
/// [`Strip::offset`] / [`Strip::size`] / [`Strip::total`]) is compared
/// against an independently-computed `f64`. The prefix-sum is held in
/// 1/65536 sub-pixel units, so a value with a non-power-of-2 denominator
/// (0.1, 0.333) is truncated in and out — worst-case round-trip error
/// just over 1.53e-5. `1e-3` is well above the precision floor and well
/// below any real arithmetic bug (a missing `+ gap` term is off by
/// ~24).
const APPROX_TOL: f64 = 1e-3;

/// Three items, sizes 100 / 200 / 100, gap 24 => starts 0 / 124 / 348.
fn fixture() -> Strip {
    Strip::new([100.0, 200.0, 100.0], 24.0)
}

#[test]
fn offsets_sizes_and_total() {
    let s = fixture();
    assert_eq!(s.len(), 3);
    assert_eq!(s.offset(0), 0.0);
    assert_eq!(s.offset(1), 124.0);
    assert_eq!(s.offset(2), 348.0);
    assert_eq!(s.size(0), 100.0);
    assert_eq!(s.size(1), 200.0);
    assert_eq!(s.size(2), 100.0);
    // No trailing gap.
    assert_eq!(s.total(), 448.0);
    // Past the end reads as the total, for trailing spacers.
    assert_eq!(s.offset(3), 448.0);
    assert_eq!(s.offset(99), 448.0);
    assert_eq!(s.size(3), 0.0);
}

#[test]
fn empty_strip_is_inert() {
    let s = Strip::new([], 24.0);
    assert!(s.is_empty());
    assert_eq!(s.len(), 0);
    assert_eq!(s.total(), 0.0);
    assert_eq!(s.offset(0), 0.0);
    assert_eq!(s.index_at(500.0), 0);
    assert_eq!(s.dominant(0.0, 100.0), 0);
    assert_eq!(s.overlapping(0.0, 100.0), None);
    assert_eq!(s.window(0.0, 100.0, Budget::default()), None);
}

#[test]
fn uniform_matches_explicit() {
    let a = Strip::uniform(3, 100.0, 24.0);
    let b = Strip::new([100.0, 100.0, 100.0], 24.0);
    assert_eq!(a, b);
    assert_eq!(a.total(), 348.0);
}

#[test]
fn index_at_resolves_gaps_and_ends() {
    let s = fixture();
    assert_eq!(s.index_at(-10.0), 0);
    assert_eq!(s.index_at(0.0), 0);
    assert_eq!(s.index_at(99.0), 0);
    // An item ending exactly at `pos` has scrolled out: the next one leads.
    assert_eq!(s.index_at(100.0), 1);
    // 100..124 is the gap after item 0 — the item BELOW it now leads, so
    // this agrees with `overlapping`, which uses the same strict top edge.
    assert_eq!(s.index_at(110.0), 1);
    assert_eq!(s.index_at(124.0), 1);
    assert_eq!(s.index_at(347.0), 2);
    assert_eq!(s.index_at(348.0), 2);
    // Past the end clamps to the last item.
    assert_eq!(s.index_at(10_000.0), 2);
}

/// `index_at` and `overlapping` must agree about who leads the viewport,
/// including at exact boundaries and inside gaps.
#[test]
fn index_at_agrees_with_overlapping() {
    let s = Strip::new([100.0, 200.0, 100.0], 24.0);
    let mut pos = 0.0;
    while pos < s.total() {
        if let Some(w) = s.overlapping(pos, 10.0) {
            assert_eq!(s.index_at(pos), w.first, "disagreement at pos={pos}");
        }
        pos += 0.5;
    }
}

#[test]
fn overlapping_edges() {
    let s = fixture();
    assert_eq!(
        s.overlapping(0.0, 100.0).unwrap(),
        Window { first: 0, last: 0 }
    );
    // An item ending exactly at the top edge has scrolled out.
    assert_eq!(
        s.overlapping(100.0, 100.0).unwrap(),
        Window { first: 1, last: 1 }
    );
    assert_eq!(
        s.overlapping(0.0, 150.0).unwrap(),
        Window { first: 0, last: 1 }
    );
    assert_eq!(
        s.overlapping(0.0, 10_000.0).unwrap(),
        Window { first: 0, last: 2 }
    );
    // A viewport parked wholly inside the 100..124 gap sees nothing.
    assert_eq!(s.overlapping(105.0, 10.0), None);
    // Past the end.
    assert_eq!(s.overlapping(1_000.0, 100.0), None);
}

#[test]
fn window_keeps_every_visible_item() {
    let s = Strip::uniform(40, 300.0, 24.0);
    let budget = Budget::screenfuls(1.0, 3);
    // Sweep the whole scrollable range; the invariant must never break.
    let mut top = 0.0;
    while top < s.total() {
        let vh = 900.0;
        if let Some(vis) = s.visible(top, vh) {
            let win = s.window(top, vh, budget).expect("non-empty");
            assert!(
                win.first <= vis.first && win.last >= vis.last,
                "window {win:?} dropped a visible item {vis:?} at top={top}"
            );
        }
        top += 37.0;
    }
}

#[test]
fn window_honours_max_items_when_it_can() {
    let s = Strip::uniform(40, 100.0, 24.0);
    // Viewport shows ~2 items; read-ahead would pull in many more.
    let win = s
        .window(1_000.0, 200.0, Budget::screenfuls(5.0, 4))
        .unwrap();
    assert_eq!(win.len(), 4);

    // `max_items: 0` is documented to behave as `1` — a budget of zero
    // would blank out the entire list, which can never be correct (the
    // reader always sees something).
    let s0 = Strip::uniform(10, 1_000.0, 24.0);
    let win0 = s0.window(0.0, 100.0, Budget::screenfuls(0.0, 0)).unwrap();
    assert_eq!(win0.len(), 1);
}

#[test]
fn window_exceeds_budget_only_for_visible_items() {
    // Ten short items are all on screen at once, with a budget of 1.
    let s = Strip::uniform(10, 50.0, 0.0);
    let win = s.window(0.0, 500.0, Budget::screenfuls(0.0, 1)).unwrap();
    let vis = s.visible(0.0, 500.0).unwrap();
    assert_eq!(win, vis, "visibility must win over the ceiling");
}

#[test]
fn window_trims_furthest_first_and_keeps_the_item_below() {
    // Items are 100 tall, gap 0. Viewport 100 tall parked exactly on item 5.
    let s = Strip::uniform(20, 100.0, 0.0);
    let win = s.window(500.0, 100.0, Budget::screenfuls(2.0, 3)).unwrap();
    // Visible is item 5; with 3 slots we keep 5 and prefer below => 5,6,7.
    assert!(win.contains(5));
    assert_eq!(win.len(), 3);
    assert_eq!(win.first, 5, "should evict above before below");
}

#[test]
fn window_past_end_of_document_is_none() {
    let s = Strip::uniform(3, 100.0, 24.0);
    assert_eq!(s.window(100_000.0, 900.0, Budget::default()), None);
}

#[test]
fn dominant_picks_the_item_you_see_most_of() {
    let s = Strip::uniform(10, 100.0, 0.0);
    // Viewport 0..100 => item 0 fully covered.
    assert_eq!(s.dominant(0.0, 100.0), 0);
    // Viewport 90..190 => 10px of item 0, 90px of item 1.
    assert_eq!(s.dominant(90.0, 100.0), 1);
    // Viewport 40..140 => 60px of item 0, 40px of item 1.
    assert_eq!(s.dominant(40.0, 100.0), 0);
    // Exactly 50/50 between items 0 and 1: ties go to the lower index.
    assert_eq!(s.dominant(50.0, 100.0), 0);

    // A viewport with zero extent cannot compute area-of-coverage, so it
    // falls back to the top edge — same answer as `index_at(scroll_top)`.
    let s2 = Strip::new([100.0, 200.0, 100.0], 24.0);
    assert_eq!(s2.dominant(130.0, 0.0), s2.index_at(130.0));
    assert_eq!(s2.dominant(130.0, 0.0), 1);
}

#[test]
fn dominant_after_a_jump_reports_the_jumped_to_item() {
    let s = Strip::uniform(30, 300.0, 24.0);
    for i in 0..30 {
        let top = s.offset(i);
        assert_eq!(s.dominant(top, 900.0), i, "jump to {i} should report {i}");
    }
}

#[test]
fn window_iteration_is_inclusive() {
    let w = Window { first: 2, last: 5 };
    assert_eq!(w.len(), 4);
    assert!(w.contains(2) && w.contains(5) && !w.contains(6));
    assert_eq!(w.iter().collect::<Vec<_>>(), alloc::vec![2, 3, 4, 5]);
    assert_eq!(w.into_iter().count(), 4);
}

#[test]
fn offsets_are_consistent_with_sizes_for_ragged_input() {
    // Power-of-2 denominators (7.5, 999.25, 0.25, 0.0625) round-trip
    // EXACTLY through the i64 sub-pixel layer; non-power-of-2 ones (0.1,
    // 0.333, 0.999, 123.456) round to the nearest 1/65536 and lose
    // ~1.5e-5 — the documented trade-off for i64 storage.
    let sizes = [
        13.0, 400.0, 7.5, 999.25, 1.0, 0.1, 0.333, 0.999, 123.456, 0.25, 0.0625,
    ];
    let s = Strip::new(sizes, 11.0);
    let mut expect = 0.0;
    for (i, &sz) in sizes.iter().enumerate() {
        assert!(
            (s.offset(i) - expect).abs() < APPROX_TOL,
            "offset {i}: {} vs {}",
            s.offset(i),
            expect
        );
        assert!(
            (s.size(i) - sz).abs() < APPROX_TOL,
            "size {i}: {} vs {}",
            s.size(i),
            sz
        );
        expect += sz + 11.0;
    }
    assert!((s.total() - (expect - 11.0)).abs() < APPROX_TOL);
}

/// Demonstrates the i64 sub-pixel precision trade-off explicitly.
/// Power-of-2 denominators are exact (round-trip error 0); anything else
/// lands within `1 / SUBPIXEL_FACTOR` of the original value. This is what
/// makes `APPROX_TOL = 1e-3` the right tolerance everywhere else.
#[test]
fn subpixel_precision_for_non_binary_fractions() {
    // Exact: denominators are powers of 2.
    for &x in &[0.0, 1.0, 0.5, 0.25, 0.125, 7.5, 999.25, 13.0, 1_234_567.0] {
        assert!(
            (from_sub(to_sub(x)) - x).abs() < 1e-12,
            "exact round-trip {x}"
        );
    }
    // Approximate: non-power-of-2 denominators lose up to 1/SUBPIXEL_FACTOR.
    // The bound is symmetric and tight (rounding to nearest, not truncating).
    let bound = 1.0 / (SUBPIXEL_FACTOR as f64);
    for &x in &[0.1, 0.333, 0.999, 123.456, 0.001, 0.789, 42.195] {
        let err = (from_sub(to_sub(x)) - x).abs();
        assert!(err < bound, "round-trip {x}: error {err} exceeds {bound}");
    }
    // NaN / inf / negative clamps to zero, never panics.
    assert_eq!(to_sub(f64::NAN), 0);
    assert_eq!(to_sub(f64::INFINITY), i64::MAX);
    assert_eq!(to_sub(-1.0), 0);
}

#[test]
fn hinted_index_matches_unhinted_for_all_positions() {
    let s = Strip::uniform(50, 100.0, 24.0);
    let mut hint = 0usize;
    let mut pos = 0.0;
    while pos < s.total() {
        let a = s.index_at(pos);
        let b = s.index_at_hinted(pos, &mut hint);
        assert_eq!(
            a, b,
            "hinted disagrees with unhinted at pos={pos}: {a} vs {b}"
        );
        pos += 1.0;
    }
}

#[test]
fn hinted_index_handles_large_jumps() {
    let s = Strip::uniform(1000, 100.0, 24.0);
    let mut hint = 0usize;
    // Jump to the middle.
    let mid = s.total() / 2.0;
    assert_eq!(s.index_at_hinted(mid, &mut hint), s.index_at(mid));
    // Jump to near the end.
    let end = s.total() - 50.0;
    assert_eq!(s.index_at_hinted(end, &mut hint), s.index_at(end));
    // Jump back to the start.
    assert_eq!(s.index_at_hinted(0.0, &mut hint), s.index_at(0.0));
}

#[test]
fn hinted_overlapping_matches_unhinted() {
    let s = Strip::new([100.0, 200.0, 150.0, 100.0, 200.0], 24.0);
    let mut hint = 0usize;
    let mut top = 0.0;
    while top < s.total() {
        let a = s.overlapping(top, 200.0);
        let b = crate::backend::overlapping_hinted(&s, top, 200.0, &mut hint);
        assert_eq!(a, b, "hinted overlapping disagrees at top={top}");
        top += 23.0;
    }
}

#[test]
fn set_size_updates_offsets_and_total() {
    let mut s = Strip::new([100.0, 200.0, 100.0], 24.0);
    let delta = s.set_size(1, 300.0);
    assert_eq!(delta, 100.0);
    assert_eq!(s.size(0), 100.0);
    assert_eq!(s.size(1), 300.0);
    assert_eq!(s.size(2), 100.0);
    assert_eq!(s.offset(0), 0.0);
    assert_eq!(s.offset(1), 124.0);
    assert_eq!(s.offset(2), 124.0 + 300.0 + 24.0);
    assert_eq!(s.total(), 100.0 + 24.0 + 300.0 + 24.0 + 100.0);

    // Out-of-range index is a no-op (returns 0.0, no panic, no mutation).
    // Same for a size that equals the current size — early return, no work.
    let mut s2 = Strip::new([100.0, 200.0], 24.0);
    assert_eq!(s2.set_size(5, 200.0), 0.0);
    assert_eq!(s2.set_size(0, 100.0), 0.0);
    assert_eq!(s2.size(0), 100.0);
    assert_eq!(s2.size(1), 200.0);
    assert_eq!(s2.total(), 100.0 + 24.0 + 200.0);
}

#[test]
fn a_size_change_above_the_anchor_moves_the_item_by_the_delta() {
    // The scroll correction itself is `crate::anchor::correct`, tested
    // there; what a strip owes it is an honest delta and offsets that
    // already reflect the new size.
    let mut s = Strip::uniform(20, 100.0, 0.0);
    let before = s.offset(10);
    assert_eq!(s.set_size(5, 150.0), 50.0);
    assert_eq!(s.offset(10), before + 50.0);
    // A change below the anchor leaves everything above it where it was.
    let before = s.offset(10);
    assert_eq!(s.set_size(15, 200.0), 100.0);
    assert_eq!(s.offset(10), before);
}
