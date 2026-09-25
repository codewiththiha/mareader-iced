//! The bar's own cases: the overflow panel's box and where it hangs.

use super::overflow::{PANEL_ROW_H, ellipsis_anchor, panel_size};
use crate::chrome::desktop;
use crate::library::fold::{self, Crumb, ELLIPSIS_PX, FoldPlan};
use crate::ui::menu::PAD;

fn plan(widths: Vec<f32>, split: usize) -> FoldPlan {
    let chain = (0..widths.len() - 1)
        .map(|at| Crumb { id: format!("s{at}"), name: format!("Level {at}") })
        .collect();
    FoldPlan { widths, chain, split }
}

#[test]
fn the_panel_is_as_wide_as_its_widest_row_and_charges_a_row_to_each() {
    let plan = plan(vec![ELLIPSIS_PX, 60.0, 80.0, 40.0], 2);
    let size = panel_size(&plan, 400.0);
    assert_eq!(size.height, PAD * 2.0 + PANEL_ROW_H, "one row, and the popover's air");
    let widest = fold::row_widths(&[60.0, 80.0], &[2])[0] as f32;
    assert_eq!(size.width, widest, "the box is the row the fold's own packing measured");
}

#[test]
fn a_narrow_budget_packs_the_panel_into_more_rows() {
    let plan = plan(vec![ELLIPSIS_PX, 60.0, 80.0, 40.0], 2);
    let wide = panel_size(&plan, 400.0);
    let narrow = panel_size(&plan, 80.0);
    assert!(narrow.height > wide.height, "a cramped panel stacks its rows");
    assert!(narrow.width < wide.width, "and the widest row is the narrower one");
}

#[test]
fn the_panel_hangs_on_the_ellipsis_and_below_the_bar() {
    let home = 120.0 + fold::crumb_px("Home", true);
    let at = ellipsis_anchor(120.0);
    assert!(at.x > home, "past the Home crumb");
    assert!(at.x < home + ELLIPSIS_PX, "and still on the ellipsis's own box");
    assert_eq!(at.y, desktop::TITLE_BAR_H + 2.0, "just under the bar");
}
