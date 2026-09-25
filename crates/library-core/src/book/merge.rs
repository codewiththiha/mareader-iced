//! How two books that turn out to be one fold back into one: the fold keeps
//! the further read point, whichever name is real, and both shelves'
//! memberships.

use super::{Book, ReadPoint};

/// Fold one book into another: `gone` dissolves and `survivor` keeps its id,
/// name and address, taking everything the survivor does not already know.
/// Placement, read point and measurement travel from whichever row knows more;
/// a name the survivor already has wins.
pub fn fold_books(survivor: &mut Book, gone: &Book) {
    // The resume point travels as one unit: a page without its count is a
    // position the progress bar cannot draw.
    let mine = ReadPoint {
        page: survivor.page,
        num_pages: survivor.num_pages,
        fraction: survivor.fraction,
    };
    let theirs = ReadPoint {
        page: gone.page,
        num_pages: gone.num_pages,
        fraction: gone.fraction,
    };
    let point = further_point(mine, theirs).settled();
    survivor.page = point.page;
    survivor.fraction = point.fraction;
    survivor.num_pages = point.num_pages.max(survivor.num_pages).max(gone.num_pages);
    if crate::text::non_blank(survivor.title.as_deref()).is_none()
        && let Some(title) = crate::text::non_blank(gone.title.as_deref())
    {
        survivor.title = Some(title.to_string());
    }
    if survivor.author.is_none() {
        survivor.author = crate::text::non_blank(gone.author.as_deref()).map(str::to_string);
    }
    survivor.added_ms = earliest_known(survivor.added_ms, gone.added_ms);
    survivor.last_read_ms = survivor.last_read_ms.max(gone.last_read_ms);
    survivor.missing = survivor.missing && gone.missing;
    // The pending flag is the survivor's own before the fold: two placeholders
    // stay one, but a placeholder yields to a measured row.
    let was_pending = survivor.fp_pending;
    survivor.fp_pending = was_pending && gone.fp_pending;
    if was_pending && !gone.fp_pending {
        survivor.fp = gone.fp;
    }
}

/// The further of two read points: higher page wins, and on a tie the deeper
/// stream fraction. A full tie keeps `mine`, so "came from the other row" is
/// a fact rather than a coin toss.
pub fn further_point(mine: ReadPoint, theirs: ReadPoint) -> ReadPoint {
    use std::cmp::Ordering;
    match theirs.page.cmp(&mine.page) {
        Ordering::Greater => theirs,
        Ordering::Less => mine,
        Ordering::Equal => match (mine.fraction, theirs.fraction) {
            (_, None) => mine,
            (None, Some(_)) => theirs,
            (Some(a), Some(b)) => {
                if b > a {
                    theirs
                } else {
                    mine
                }
            }
        },
    }
}

/// The earlier of two stamps, treating `0` as "unknown": a migrated row has
/// no join stamp, and folding zero as the epoch would date every touched book
/// to 1970.
fn earliest_known(mine: u64, theirs: u64) -> u64 {
    match (mine, theirs) {
        (0, other) | (other, 0) => other,
        _ => mine.min(theirs),
    }
}

#[cfg(test)]
mod tests {
    use crate::book::Fingerprint;
    use crate::book::kit::linked;
    use crate::book::merge::fold_books;
    use crate::book::merge::further_point;
    use crate::book::read::ReadPoint;

    #[test]
    fn a_fold_takes_the_further_place_and_fills_the_gaps() {
        let mut keep = linked("keep", "/books/dune.pdf");
        keep.page = 12;
        keep.num_pages = 300;
        keep.added_ms = 500;
        keep.last_read_ms = 900;
        let mut gone = linked("gone", "/copies/dune.pdf");
        gone.page = 240;
        gone.title = Some("Dune".into());
        gone.author = Some("Frank Herbert".into());
        gone.added_ms = 300;
        gone.last_read_ms = 700;

        fold_books(&mut keep, &gone);
        assert_eq!(keep.page, 240, "a merge never sends a reader backwards");
        assert_eq!(keep.num_pages, 300, "the count survives from whichever row knew it");
        assert_eq!(keep.title.as_deref(), Some("Dune"), "a name fills a gap");
        assert_eq!(keep.author.as_deref(), Some("Frank Herbert"));
        assert_eq!(keep.added_ms, 300, "the book joined when it first joined");
        assert_eq!(keep.last_read_ms, 900, "and was read as recently as it was");
        assert_eq!(keep.id, "keep");
        assert_eq!(keep.path(), "/books/dune.pdf");
        let mut keep2 = linked("keep", "/books/dune.pdf");
        keep2.page = 12;
        keep2.title = Some("Mine".into());
        fold_books(&mut keep2, &gone);
        assert_eq!(keep2.page, 240);
        assert_eq!(keep2.title.as_deref(), Some("Mine"), "and never overwrites a name");
    }

    #[test]
    fn a_page_tie_goes_to_the_deeper_stream_fraction() {
        let a = ReadPoint { page: 10, num_pages: 0, fraction: Some(0.4) };
        let b = ReadPoint { page: 10, num_pages: 0, fraction: Some(0.7) };
        assert_eq!(further_point(a, b), b);
        assert_eq!(further_point(b, a), b);
        let plain = ReadPoint { page: 10, num_pages: 0, fraction: None };
        assert_eq!(further_point(plain, a), a);
        assert_eq!(further_point(a, plain), a);
        assert_eq!(further_point(a, a), a);
    }

    #[test]
    fn a_fold_measures_and_unloses_an_address() {
        // A placeholder yields to a measurement; an address is dead only when both rows say so.
        let mut pending = linked("a", "/gone/dune.pdf");
        pending.fp = Fingerprint::placeholder("/gone/dune.pdf");
        pending.fp_pending = true;
        pending.missing = true;
        let measured = linked("b", "/books/dune.pdf");
        fold_books(&mut pending, &measured);
        assert!(!pending.fp_pending, "the merged row has been weighed");
        assert_eq!(pending.fp, measured.fp);
        assert!(!pending.missing);
        let mut both = linked("a", "/gone/dune.pdf");
        both.fp_pending = true;
        both.missing = true;
        let mut also = linked("b", "/gone/dune.pdf");
        also.fp_pending = true;
        also.missing = true;
        fold_books(&mut both, &also);
        assert!(both.fp_pending && both.missing);
        // added_ms == 0 means never, not the epoch.
        let mut never = linked("a", "/one.pdf");
        never.added_ms = 0;
        let joined = {
            let mut b = linked("b", "/two.pdf");
            b.added_ms = 40;
            b
        };
        fold_books(&mut never, &joined);
        assert_eq!(never.added_ms, 40);
    }
}
