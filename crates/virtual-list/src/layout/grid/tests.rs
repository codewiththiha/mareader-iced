//! The grid layout's own cases.

use super::*;
use crate::Overscan;

fn thumbs(items: usize) -> GridLayout {
    let pitch = 120.0 * (792.0 / 612.0) + 8.0;
    GridLayout::uniform(items, 2, pitch, 120.0, 12.0)
}

#[test]
fn mapping_and_partial_last_row() {
    let g = thumbs(5);
    assert_eq!(g.columns(), 2);
    // Three rows for five items in two columns: the last item sits in row 2.
    assert_eq!(g.row_of(4), 2);
    assert_eq!(g.row_of(3), 1);
    assert_eq!(g.col_of(3), 1);
    assert_eq!(g.row_items(0), 0..2);
    assert_eq!(g.row_items(2), 4..5, "last row is partial");
    assert_eq!(g.cross_offset(0), 0.0);
    assert_eq!(g.cross_offset(1), 132.0);
    assert_eq!(g.cross_size(0), 120.0);
    assert_eq!(g.offset(2), g.row_pitch());
    assert_eq!(g.total(), 3.0 * g.row_pitch());
}

#[test]
fn thousand_item_grid_mounts_a_bounded_window() {
    let g = thumbs(1_000);
    assert_eq!(g.row_of(999), 499);
    let budget = Budget::items(2, 64);
    let vp = Viewport::new(720.0, 264.0);

    let top_rows = g.rows_window(0.0, vp, budget).expect("window at top");
    assert!(top_rows.len() < 20, "mounted {} rows", top_rows.len());

    let mid = g.row_pitch() * 200.0;
    let mid_rows = g.rows_window(mid, vp, budget).expect("window mid-grid");
    assert!(mid_rows.len() < 20);
    assert!(mid_rows.first > 0 && mid_rows.last < 499);

    let items = g.window(mid, vp, budget).unwrap();
    assert_eq!(items.first, mid_rows.first * 2);
    assert_eq!(items.last, (mid_rows.last + 1) * 2 - 1);
}

#[test]
fn mounted_rows_follow_viewport_height() {
    let g = GridLayout::uniform(200, 2, 120.0, 100.0, 8.0);
    let exact = Budget {
        overscan: Overscan::Screenfuls(0.0),
        max_items: 1_000,
    };
    let top = g.row_pitch() * 16.0;

    let small = g
        .rows_window(top, Viewport::main_only(720.0), exact)
        .unwrap();
    let tall = g
        .rows_window(top, Viewport::main_only(1_440.0), exact)
        .unwrap();

    assert_eq!(
        small.len(),
        6,
        "720px / 120px rows = exactly 6 visible rows"
    );
    assert_eq!(tall.len(), 12, "double the height mounts double the rows");

    let buffered = g
        .rows_window(top, Viewport::main_only(720.0), Budget::items(1, 1_000))
        .unwrap();
    assert_eq!(buffered.len(), 8, "6 visible + 1 buffer row each side");
}

#[test]
fn responsive_columns_follow_viewport_width() {
    let spec = GridSpec::responsive(120.0, 12.0);
    assert_eq!(spec.columns_at(264.0), 2);
    assert_eq!(spec.columns_at(540.0), 4);
    assert_eq!(spec.columns_at(1_000.0), 7);
    assert_eq!(spec.columns_at(50.0), 1, "never zero columns");
    assert_eq!(spec.columns_at(0.0), 1);

    let g = GridLayout::resolve(spec, 100, 150.0, 540.0);
    assert_eq!(g.columns(), 4);
    assert_eq!(g.row_of(99), 24);
    assert_eq!(g.col_of(5), 1);
    // The resolved cell width, read the way a consumer reads it: as the
    // cross extent of a cell, and as the stride between two columns.
    assert!((g.cross_size(0) - 126.0).abs() < 1e-9);
    assert!((g.cross_offset(5) - 138.0).abs() < 1e-9);
}

#[test]
fn one_column_grid_is_a_list() {
    let g = GridLayout::uniform(30, 1, 100.0, 200.0, 0.0);
    let l: super::super::ListLayout = super::super::ListLayout::uniform(30, 100.0, 0.0);
    assert_eq!(g.total(), l.total());
    for i in 0..30 {
        assert_eq!(g.offset(i), l.offset(i), "offset {i}");
    }
    let budget = Budget::default();
    let mut pos = 0.0;
    while pos < g.total() {
        assert_eq!(
            g.window(pos, Viewport::main_only(350.0), budget),
            l.window(pos, Viewport::main_only(350.0), budget),
            "window disagreement at {pos}"
        );
        pos += 37.0;
    }
}

#[test]
fn grid_boundary_semantics() {
    let g = GridLayout::uniform(9, 3, 120.0, 100.0, 8.0);
    let exact = Budget {
        overscan: Overscan::Screenfuls(0.0),
        max_items: 1_000,
    };
    let vp = Viewport::main_only(120.0);

    let w = g.window(120.0, vp, exact).unwrap();
    assert_eq!((w.first, w.last), (3, 5));
    let w0 = g.window(0.0, vp, exact).unwrap();
    assert_eq!((w0.first, w0.last), (0, 2));
}

#[test]
fn dominant_is_first_item_of_dominant_row() {
    let g = thumbs(100);
    let pitch = g.row_pitch();
    let top = 4.0 * pitch - pitch * 0.25;
    assert_eq!(g.dominant(top, 300.0), 8);
}

#[test]
fn hinted_matches_unhinted() {
    let g = thumbs(500);
    let mut hint = 0usize;
    let mut pos = 0.0;
    while pos < g.total() {
        assert_eq!(
            g.index_at(pos),
            g.index_at_hinted(pos, &mut hint),
            "index_at {pos}"
        );
        pos += 23.0;
    }
    let budget = Budget::items(2, 64);
    let mut hint = 0usize;
    let mut top = 0.0;
    while top < g.total() {
        assert_eq!(
            g.window(top, Viewport::main_only(720.0), budget),
            g.window_hinted(top, Viewport::main_only(720.0), budget, &mut hint),
            "window at {top}"
        );
        top += 61.0;
    }
}

#[test]
fn degenerate_inputs() {
    let g = thumbs(0);
    assert!(g.is_empty());
    assert_eq!(g.total(), 0.0);
    assert_eq!(
        g.window(0.0, Viewport::main_only(720.0), Budget::default()),
        None
    );
    assert_eq!(g.dominant(0.0, 720.0), 0);

    let g = thumbs(10);
    assert_eq!(
        g.window(99_999.0, Viewport::main_only(720.0), Budget::default()),
        None
    );

    let g = thumbs(1);
    let w = g
        .window(0.0, Viewport::main_only(720.0), Budget::default())
        .unwrap();
    assert_eq!((w.first, w.last), (0, 0));
}
