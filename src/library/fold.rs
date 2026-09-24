//! The fold's arithmetic: how many crumbs the bar keeps, which the width
//! hides, and how the panel packs the hidden ones into rows. Pure, so the
//! fold's shape is host-tested rather than eyeballed — the web
//! breadcrumb's `fold.rs`, carried over whole.
//!
//! The one native deviation is where the widths come from: the web bar
//! measured every crumb's live box with a hidden probe; the native bar has
//! no ruler to hide, so the same arithmetic runs on estimates of those
//! boxes — the 13px face's average glyph step, the crumb's own 160px cap
//! and 12px of padding, the trailing chevron, and the ellipsis's 26px.
//! The fold's shape is the web's; the numbers feeding it are honest
//! estimates rather than measurements.

use library_core::blob::LibraryBlob;
use library_core::shelf::{ancestors, find, ALL_SHELF};

/// One level of the chain the bar folds: the shelf it names and the way
/// back to it.
#[derive(Clone, PartialEq)]
pub struct Crumb {
    pub id: String,
    pub name: String,
}

/// What the bar folds: the chain root-to-leaf, the split hiding its oldest
/// levels, and the widths the split was decided from — `widths[0]` is the
/// ellipsis's own, the probe's arrangement.
pub struct FoldPlan {
    pub widths: Vec<f32>,
    pub chain: Vec<Crumb>,
    pub split: usize,
}

/// Three: the left cluster shares a row with a search box that must stay
/// usable in a window that can be 640px wide. Once the probe has measured,
/// the live split is the widths' answer.
const CRUMB_KEEP: usize = 3;

/// A shallower chain never folds, however cramped — its crumbs truncate
/// against each other instead: the smallest fold hides two levels, and below
/// four that leaves one lonely crumb beside the ellipsis.
const FOLD_MIN_DEPTH: usize = 4;

/// The panel's row packing charges the same gap; its CSS runs a column gap
/// of zero (the chevron is the spacing).
const CRUMB_GAP_PX: f64 = 2.0;

const ROW_CHROME_PX: f64 = 14.0;

/// The estimated box of one crumb: the label's own character budget at the
/// 13px face's average step, the button's padding, the bar's 160px cap,
/// and — for a crumb that trails — the chevron and its gap.
const CHAR_PX: f32 = 7.0;
const CRUMB_PAD: f32 = 12.0;
const CRUMB_CAP: f32 = 160.0;
const CHEVRON_PX: f32 = 17.0;

/// The ellipsis's estimated box: the More glyph between the probe item's
/// own paddings.
pub const ELLIPSIS_PX: f32 = 26.0;

/// The crumb label's character budget — the bar's own cut, so the estimate
/// and the drawn label agree.
const LABEL_CHARS: usize = 24;

/// One crumb's estimated width, in the probe's arrangement.
pub fn crumb_px(label: &str, trails: bool) -> f32 {
    let chars = label.chars().count().min(LABEL_CHARS) as f32;
    let face = (chars * CHAR_PX).min(CRUMB_CAP);
    face + CRUMB_PAD + if trails { CHEVRON_PX } else { 0.0 }
}

/// The fold's answer for the level the shelf stands on: the chain, the
/// widths the bar estimates for it, and the split the cluster's available
/// width decides. At the root the chain is empty and nothing folds.
pub fn plan(library: &LibraryBlob, shelf: &str, avail: f32) -> FoldPlan {
    let chain: Vec<Crumb> = if shelf == ALL_SHELF {
        Vec::new()
    } else {
        let mut chain: Vec<Crumb> = ancestors(&library.shelves, shelf)
            .into_iter()
            .map(|level| Crumb { id: level.id.clone(), name: level.name.clone() })
            .collect();
        if let Some(current) = find(&library.shelves, shelf) {
            chain.push(Crumb { id: current.id.clone(), name: current.name.clone() });
        }
        chain
    };
    let len = chain.len();
    let last = len.saturating_sub(1);
    let mut widths: Vec<f32> = vec![ELLIPSIS_PX];
    widths.extend(
        chain
            .iter()
            .enumerate()
            .map(|(at, crumb)| crumb_px(&crumb.name, at != last)),
    );
    let measured: Vec<f64> = widths.iter().map(|width| f64::from(*width)).collect();
    let split = choose_split(&measured, f64::from(avail), len);
    FoldPlan { widths, chain, split }
}

pub fn pack_rows(widths: &[f64], budget: f64) -> Vec<usize> {
    let mut counts = vec![0usize];
    let mut used = ROW_CHROME_PX;
    for width in widths {
        let item = width + CRUMB_GAP_PX;
        if *counts.last().unwrap_or(&0) > 0 && used + item > budget {
            counts.push(0);
            used = ROW_CHROME_PX;
        }
        *counts.last_mut().unwrap() += 1;
        used += item;
    }
    counts
}

pub fn row_widths(widths: &[f64], counts: &[usize]) -> Vec<f64> {
    let mut out = Vec::with_capacity(counts.len());
    let mut at = 0usize;
    for &count in counts {
        let end = (at + count).min(widths.len());
        let row = &widths[at..end];
        out.push(row.iter().sum::<f64>() + CRUMB_GAP_PX * row.len() as f64 + ROW_CHROME_PX);
        at = end;
    }
    out
}

pub fn split_by_counts<T>(chain: Vec<T>, counts: &[usize]) -> Vec<Vec<T>> {
    let mut rows: Vec<Vec<T>> = Vec::with_capacity(counts.len());
    let mut rest = chain;
    for &count in counts {
        if count == 0 {
            continue;
        }
        let take = count.min(rest.len());
        rows.push(rest.drain(..take).collect());
    }
    if !rest.is_empty() {
        rows.push(rest);
    }
    rows
}

/// The elided crumbs packed for the panel's budget: how many go on each row,
/// and the widths those rows are measured from. The panel's box and the rows
/// it draws both read this one packing, so the two cannot disagree about how
/// tall it stands.
pub fn pack_elided(plan: &FoldPlan, budget: f32) -> (Vec<usize>, Vec<f64>) {
    let widths: Vec<f64> =
        plan.widths[1..=plan.split].iter().map(|width| f64::from(*width)).collect();
    let counts = pack_rows(&widths, f64::from(budget));
    (counts, widths)
}

/// Never one: a single elided level costs a hover to reach and the same bar
/// width as showing it, so the ellipsis earns its slot from two levels up.
fn elide_at(len: usize) -> usize {
    let split = len.saturating_sub(CRUMB_KEEP);
    if split == 1 {
        0
    } else {
        split
    }
}

/// Beyond the depth gate, the smallest split (never exactly one — the rule
/// [`elide_at`] keeps) whose ellipsis and kept crumbs fit the cluster's live
/// box; 0 when the whole chain already fits.
fn choose_split(widths: &[f64], available: f64, len: usize) -> usize {
    if len < FOLD_MIN_DEPTH {
        return 0;
    }
    if widths.len() != len + 1 || available <= 0.0 {
        return elide_at(len);
    }
    let items = &widths[1..];
    let gap = |n: usize| CRUMB_GAP_PX * n.saturating_sub(1) as f64;
    let total: f64 = items.iter().sum::<f64>() + gap(len);
    if total <= available {
        return 0;
    }
    let ellipsis = widths[0];
    for split in 2..len {
        let suffix: f64 = items[split..].iter().sum::<f64>() + gap(len - split);
        if ellipsis + suffix + CRUMB_GAP_PX <= available {
            return split;
        }
    }
    (len - 1).max(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shallow_chain_elides_nothing() {
        for len in 0..=CRUMB_KEEP {
            assert_eq!(elide_at(len), 0, "a chain of {len} fits the bar whole");
        }
    }

    #[test]
    fn one_elided_crumb_is_not_worth_an_affordance() {
        assert_eq!(elide_at(CRUMB_KEEP + 1), 0);
        assert_eq!(elide_at(CRUMB_KEEP + 2), 2);
    }

    #[test]
    fn past_the_first_fold_the_bar_stays_the_same_width() {
        for len in (CRUMB_KEEP + 2)..=(CRUMB_KEEP * 4) {
            let split = elide_at(len);
            assert_eq!(len - split, CRUMB_KEEP, "a chain of {len} shows only {kept}", kept = CRUMB_KEEP);
            assert!(split >= 2, "and never hides just one");
        }
    }

    #[test]
    fn the_elided_and_the_shown_are_one_chain_and_never_overlap() {
        for len in 0..12 {
            let split = elide_at(len);
            assert!(split <= len);
            assert_eq!(split + (len - split), len);
        }
    }

    fn measured(len: usize, ellipsis: f64, crumb: f64) -> Vec<f64> {
        std::iter::once(ellipsis).chain(std::iter::repeat_n(crumb, len)).collect()
    }

    #[test]
    fn a_chain_the_cluster_holds_folds_nothing() {
        let widths = measured(4, 22.0, 120.0);
        assert_eq!(choose_split(&widths, 600.0, 4), 0);
    }

    #[test]
    fn a_shallow_chain_never_folds_however_cramped() {
        for len in 0..FOLD_MIN_DEPTH {
            let widths = measured(len, 22.0, 900.0);
            for available in [0.0, 40.0, 324.0, 5000.0] {
                assert_eq!(
                    choose_split(&widths, available, len),
                    0,
                    "a chain of {len} keeps every crumb at every width"
                );
            }
        }
    }

    #[test]
    fn a_cramped_cluster_folds_the_oldest_levels_it_must_and_no_more() {
        let widths = measured(4, 22.0, 200.0);
        assert_eq!(choose_split(&widths, 500.0, 4), 2);
        assert_eq!(choose_split(&widths, 426.0, 4), 2, "the fit is inclusive");
        assert_eq!(
            choose_split(&widths, 425.0, 4),
            3,
            "one pixel less and a level goes behind the fold"
        );
        assert_eq!(choose_split(&widths, 224.0, 4), 3);
    }

    #[test]
    fn nothing_fits_but_the_newest_level_and_the_fold_stops_there() {
        let widths = measured(4, 22.0, 200.0);
        assert_eq!(choose_split(&widths, 100.0, 4), 3, "len - 1, never past the chain");
    }

    #[test]
    fn the_fold_never_hides_exactly_one_level_whatever_the_numbers() {
        for len in 1..8 {
            let widths = measured(len, 22.0, 300.0);
            for available in [0.0, 24.0, 324.0, 646.0, 5000.0] {
                let split = choose_split(&widths, available, len);
                assert_ne!(split, 1, "one hidden level costs a hover and a bar slot");
                assert!(
                    split <= len,
                    "split_at({split}) on a chain of {len} would panic"
                );
            }
        }
    }

    #[test]
    fn unmeasured_numbers_fall_back_to_the_count_rule() {
        assert_eq!(choose_split(&[], 500.0, 6), elide_at(6));
        assert_eq!(
            choose_split(&measured(4, 22.0, 100.0), 500.0, 6),
            elide_at(6),
            "a probe out of step with the chain is not trusted"
        );
        assert_eq!(
            choose_split(&measured(5, 22.0, 100.0), 0.0, 5),
            elide_at(5),
            "a cluster with no measured box folds by count"
        );
    }

    #[test]
    fn one_crumb_that_does_not_fit_is_truncated_not_hidden() {
        let widths = measured(1, 22.0, 900.0);
        assert_eq!(
            choose_split(&widths, 100.0, 1),
            0,
            "hiding the only level behind an ellipsis leaves the bar with nowhere to go"
        );
    }

    #[test]
    fn rows_pack_to_the_budget_and_later_rows_start_fresh() {
        assert_eq!(pack_rows(&[100.0, 100.0, 100.0], 220.0), vec![2, 1]);
        assert_eq!(
            pack_rows(&[100.0, 100.0, 100.0], 1000.0),
            vec![3],
            "under the budget it is one row"
        );
        assert_eq!(
            pack_rows(&[500.0, 100.0], 200.0),
            vec![1, 1],
            "a crumb wider than the budget gets a row to itself; it is never dropped"
        );
        assert_eq!(
            pack_rows(&[], 220.0).iter().sum::<usize>(),
            0,
            "no crumbs, no rows worth of them"
        );
    }

    #[test]
    fn a_packed_row_is_as_wide_as_its_own_labels_plus_chrome() {
        let widths = [100.0, 100.0, 100.0];
        let counts = pack_rows(&widths, 220.0);
        let rows = row_widths(&widths, &counts);
        assert_eq!(rows, vec![218.0, 116.0]);
        assert_eq!(
            rows.into_iter().fold(0.0, f64::max),
            218.0,
            "the panel is the width of its widest row and no wider"
        );
    }

    #[test]
    fn splitting_a_chain_by_counts_never_loses_a_level() {
        let chain: Vec<Crumb> = (0..5)
            .map(|at| Crumb { id: format!("s{at}"), name: format!("Level {at}") })
            .collect();
        let rows = split_by_counts(chain, &[2, 2]);
        assert_eq!(
            rows.len(),
            3,
            "the leftover rides a tail row rather than vanishing"
        );
        assert_eq!(rows.iter().flatten().count(), 5);
        assert_eq!(rows[0][0].id, "s0");
        assert_eq!(rows[1][0].id, "s2");
        assert_eq!(rows[2][0].id, "s4");
    }
    #[test]
    fn the_elided_half_of_a_plan_is_what_the_panel_packs() {
        let plan = FoldPlan {
            widths: vec![ELLIPSIS_PX, 40.0, 40.0, 40.0, 40.0],
            chain: vec![
                Crumb { id: "a".into(), name: "A".into() },
                Crumb { id: "b".into(), name: "B".into() },
                Crumb { id: "c".into(), name: "C".into() },
                Crumb { id: "d".into(), name: "D".into() },
            ],
            split: 2,
        };
        let (counts, widths) = pack_elided(&plan, 400.0);
        assert_eq!(widths, vec![40.0, 40.0], "the shown crumbs are the panel's business");
        assert_eq!(counts.iter().sum::<usize>(), 2, "every elided crumb takes a slot");
    }

}
