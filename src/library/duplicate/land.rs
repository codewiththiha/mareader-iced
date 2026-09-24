//! Writing a plan into the lists: the rows a duplicate becomes, the member lists
//! rewritten onto the fresh ids, and the fresh tree spliced in behind the one it
//! was copied from.

use std::collections::HashMap;

use library_core::book::{Book, Fingerprint, Origin, Row};
use library_core::conflict::next_name;
use library_core::id;
use library_core::shelf::{self, Shelf};

use super::{BookCopy, Member, TreePlan};

/// A shelf link's landing: a second link at the same shelf, filed beside the
/// first. The counter name and the filing are the row rules every duplicate
/// follows; only the "stays a pointer" half is this function's own.
#[allow(clippy::too_many_arguments)]
pub fn land_shelf_link(
    rows: &mut Vec<Row>,
    shelves: &mut [Shelf],
    level: &str,
    row_id: &str,
    name: &str,
    target: &str,
    now: u64,
) -> String {
    let title = next_name(rows, shelves, level, name);
    let dup = Row::link(id::next_id(now), title.clone(), target.to_string(), now);
    let dup_id = dup.id().to_string();
    rows.push(dup);
    file_beside(shelves, row_id, &dup_id);
    title
}

/// A book copy's landing, wearing the name the reader pointed at. Whatever
/// origin the original had, the copy is `Stored`; the row lands beside the
/// row the reader pointed at, not beside the original a link happened to
/// read. When the marks' store arrives with the reader, the original's
/// highlights ride along here as the copy's own list — same words at the
/// same spots, ids of their own — so the two books' marks are separate from
/// the first moment.
pub fn land_book(
    rows: &mut Vec<Row>,
    shelves: &mut [Shelf],
    level: &str,
    copy: BookCopy,
    store: String,
    measured: Option<Fingerprint>,
    now: u64,
) -> String {
    let title = next_name(rows, shelves, level, &copy.shown);
    let dup = stored_copy(&copy.book, copy.new_id, store, measured, &title, now);
    let dup_id = dup.id.clone();
    rows.push(Row::Book(dup));
    file_beside(shelves, &copy.beside, &dup_id);
    title
}

/// Land the plan: the rows the copies and carried links become, the member
/// lists rewritten onto the fresh ids, and the fresh tree spliced in right
/// behind the original. `landed` is the store batch's answer — which copies
/// came home, and each one's own measurement — and a member whose copy is
/// not in it is dropped, not shared.
pub fn land_tree(
    rows: &mut Vec<Row>,
    shelves: &mut Vec<Shelf>,
    mut plan: TreePlan,
    landed: &HashMap<String, (String, Option<Fingerprint>)>,
    now: u64,
) -> String {
    let mut mapped: HashMap<String, String> = HashMap::with_capacity(plan.members.len());
    let mut fresh_rows: Vec<Row> = Vec::with_capacity(plan.members.len());
    for (old, what) in &plan.members {
        match what {
            Member::Copy { new_id, book, shown } => {
                let Some((store, measured)) = landed.get(new_id) else {
                    continue;
                };
                let dup = stored_copy(book, new_id.clone(), store.clone(), *measured, shown, now);
                mapped.insert(old.clone(), new_id.clone());
                fresh_rows.push(Row::Book(dup));
            }
            Member::Link(row) => {
                mapped.insert(old.clone(), row.id().to_string());
                fresh_rows.push(row.clone());
            }
            Member::Skip => {}
        }
    }
    if !fresh_rows.is_empty() {
        rows.extend(fresh_rows);
    }
    for level in &mut plan.shelves {
        let original_members = std::mem::take(&mut level.books);
        level.books =
            original_members.into_iter().filter_map(|old| mapped.get(&old).cloned()).collect();
    }
    // Right behind the original's own row: the shelf list IS the render
    // order, so a copy appended to the end would be a shelf the reader has
    // to go and find.
    let at = shelves
        .iter()
        .position(|s| s.id == plan.original_id)
        .map_or(shelves.len(), |at| at + 1);
    shelves.splice(at..at, plan.shelves);
    plan.name
}

/// The copy of a book, which is the same object whichever door duplicated
/// it: a stored book at a fresh id, the original's address kept as where the
/// bytes came from, and the name the reader asked for rather than the file's
/// own.
///
/// `shown` is worn as a locked title — a base the file itself carried
/// underscores in ("harry_potter_1") reads as snake-case debris to the title
/// rule, and a name the reader just asked for is not debris.
fn stored_copy(
    book: &Book,
    new_id: String,
    store: String,
    measured: Option<Fingerprint>,
    shown: &str,
    now: u64,
) -> Book {
    let mut dup = Book::new(
        new_id,
        Fingerprint::placeholder(&store),
        book.format,
        Origin::Stored { src: book.origin.source().map(str::to_string), store },
        now,
    );
    dup.title = Some(shown.to_string());
    dup.title_locked = true;
    dup.adopt_measurement(measured);
    dup
}

/// `place` is the drag's spelling — remove, insert at the index — which is
/// what "beside the row you pointed at" means on a list the reader can see.
fn file_beside(shelves: &mut [Shelf], original_id: &str, dup_id: &str) {
    let seats: Vec<(String, usize)> = shelf::containing(shelves, original_id)
        .into_iter()
        .filter_map(|level| {
            let at = level.books.iter().position(|m| m == original_id)?;
            Some((level.id.clone(), at + 1))
        })
        .collect();
    for (shelf_id, index) in &seats {
        if let Some(level) = shelf::find_mut(shelves, shelf_id) {
            shelf::place(&mut level.books, dup_id, Some(*index));
        }
    }
}
