//! The shelf's "Duplicate": a second instance of one thing, asked for by name.
//!
//! A duplicate is the app's own object from the moment it exists: nothing
//! about it is shared with what it came from. Not the bytes — a read-at-place
//! book, a stored copy and a link at a book all duplicate into the library's
//! own store, each in its own item folder — and not the reader's work: when
//! the marks' store arrives with the reader, the highlights land on the copy
//! as its own list under its own id. A shelf duplicates the same way: a
//! second tree holding fresh copies of the books, not a second door onto the
//! same rows.
//!
//! The one thing that stays a pointer is a link at a shelf: a level holds
//! membership and never held a byte, so there is nothing to store.
//!
//! This is the pure half the web service ran against its signals: the plans
//! read the lists, the landings write them, and the store batch between the
//! two is the app's — [`tree_requests`] hands it the copy work a tree owes
//! and [`land_tree`] takes the batch's answer. The queue that walks entries
//! one at a time lives in the app too: each counter name counts against the
//! level as the last landing left it, which is what the web's own sequential
//! loop gave.

use std::collections::{HashMap, HashSet};

use library_core::book::{find_row, Book, Fingerprint, Origin, Row};
use library_core::conflict::{next_name, next_shelf_name};
use library_core::id;
use library_core::shelf::{self, Shelf, ShelfKind};
use library_core::wire::BookFileRequest;

/// What one entry's duplicate turns out to be, planned against the lists as
/// they stand. The app turns each answer into either a landing on the spot
/// or a store batch with a landing behind it.
#[derive(Debug)]
pub enum DupPlan {
    /// A second link at the same shelf, filed beside the first: the one
    /// duplicate that stays a pointer, owing no bytes to the store.
    ShelfLink { row_id: String, name: String, target: String },
    /// One book's copy: the bytes the store will copy and the row the copy
    /// lands beside.
    Book(Box<BookCopy>),
    /// A whole tree: the fresh subtree and every member of it resolved,
    /// waiting on the one store batch the run owes.
    Tree(TreePlan),
    /// Nothing to copy and nothing to say: a book whose address died (the
    /// menu row was disabled before the click could land) or a shelf that is
    /// not there.
    Skip,
    /// A link whose target is gone — the one refusal said out loud, wearing
    /// the toast's own sentence.
    Dead(String),
}

/// One book's copy, resolved at the ask: the id is minted early because the
/// store's request and the item folder it makes both wear it.
#[derive(Debug)]
pub struct BookCopy {
    /// The row id the copy lands as — also the store's own name for the
    /// copy's item folder.
    pub new_id: String,
    /// The book whose bytes the copy wears.
    pub book: Book,
    /// The row the copy is filed beside: the row the reader pointed at, not
    /// the original a link happened to read.
    pub beside: String,
    /// The name the copy wears before the level's counter steps it: the
    /// book's own display name, or the link's name when a link was what was
    /// duplicated.
    pub shown: String,
}

/// The whole run's plan before any bytes move: the fresh subtree, and every
/// member of it resolved to the thing it will become. Pure on the lists it
/// read, so the host tests can ask it directly — the store is the only half
/// of a duplicate that cannot run there.
#[derive(Debug)]
pub struct TreePlan {
    /// The copy's own name, the level's counter.
    pub name: String,
    /// The shelf the reader pointed at: the run's card is labelled with what
    /// is being duplicated, the way a folder run is labelled with its folder.
    pub label: String,
    /// Where the fresh subtree splices in: right behind this shelf, the row
    /// rule every duplicate follows.
    pub original_id: String,
    /// The fresh subtree in the original's own order — new ids, the counter
    /// name on the root, everything `Virtual`, parents remapped — with `books`
    /// still holding the ORIGINAL member ids, because which of them survive is
    /// the landing's to say and only for the members that actually copied.
    pub shelves: Vec<Shelf>,
    /// Every member of the subtree in first-seen order, keyed by the id the
    /// original shelves hold.
    pub members: Vec<(String, Member)>,
}

/// One member of the subtree, resolved for the copy it becomes. A book filed
/// twice inside one tree is one member, not two: the copy mirrors the tree it
/// came from, and it is the ORIGINAL the copy never shares with.
#[derive(Debug)]
pub enum Member {
    /// A book, or a link at one: the bytes the run will copy, the name the copy
    /// wears, and the row whose highlights ride along — landed under `new_id`,
    /// which is also the store's own name for the copy's item folder.
    Copy { new_id: String, book: Book, shown: String },
    /// A link at a shelf: carried as a fresh link row, its target remapped onto
    /// the copy when the shelf it points at is inside the tree.
    Link(Row),
    /// A book whose address died, a link at one, a membership naming no row:
    /// nothing to copy, so the copy of the tree goes without it rather than
    /// refusing the whole shelf.
    Skip,
}

/// One landing, for the report the whole run ends with.
#[derive(Debug)]
pub struct Duplicated {
    pub name: String,
    pub shelf: bool,
}

/// What a link points at, for the duplicate it is about to become: the book
/// whose bytes the copy will wear, or the shelf a second pointer stays at. A
/// link whose target is gone, dead or not a book, answers [`LinkAt::Dead`],
/// which is the refusal every caller of this shares.
enum LinkAt {
    Book(Book),
    Shelf,
    Dead,
}

fn link_at(rows: &[Row], target: &str) -> LinkAt {
    if id::is_shelf(target) {
        return LinkAt::Shelf;
    }
    match find_row(rows, target) {
        Some(Row::Book(book)) if !book.missing => LinkAt::Book(book.clone()),
        _ => LinkAt::Dead,
    }
}

/// One entry's answer, asked of the lists as they stand. ENTRIES because a
/// selection holds shelf ids and row ids alike, and the two namespaces are
/// disjoint by prefix: the row list answering "not mine" is the shelf list's
/// turn.
pub fn plan_one(rows: &[Row], shelves: &[Shelf], entry: &str, now: u64) -> DupPlan {
    let Some(row) = find_row(rows, entry) else {
        return match plan_tree(rows, shelves, entry, now) {
            Some(tree) => DupPlan::Tree(tree),
            None => DupPlan::Skip,
        };
    };
    match row {
        Row::Link { id: row_id, name, target, .. } => match link_at(rows, target) {
            // A shelf's link is the one duplicate that stays a pointer: a
            // level holds no bytes, so there is nothing to store and nothing
            // to separate.
            LinkAt::Shelf => DupPlan::ShelfLink {
                row_id: row_id.clone(),
                name: name.clone(),
                target: target.clone(),
            },
            // A book's link is a doorway onto the target's bytes: the
            // duplicate is the app's own copy of what it opens, filed beside
            // the link itself.
            LinkAt::Book(book) => DupPlan::Book(Box::new(BookCopy {
                new_id: id::next_id(now),
                book,
                beside: row_id.clone(),
                shown: name.clone(),
            })),
            LinkAt::Dead => {
                DupPlan::Dead(format!("“{name}” points at a book that is not there any more."))
            }
        },
        Row::Book(book) => {
            if book.missing {
                return DupPlan::Skip;
            }
            let shown = book.title();
            let beside = book.id.clone();
            DupPlan::Book(Box::new(BookCopy {
                new_id: id::next_id(now),
                book: book.clone(),
                beside,
                shown,
            }))
        }
    }
}

/// The copy work a tree owes the store, in the members' own order: one
/// request per book the run will copy, each wearing the id it lands as.
pub fn tree_requests(plan: &TreePlan) -> Vec<BookFileRequest> {
    plan.members
        .iter()
        .filter_map(|(_, what)| match what {
            Member::Copy { new_id, book, .. } => {
                Some(BookFileRequest { from: book.path().to_string(), id: new_id.clone() })
            }
            _ => None,
        })
        .collect()
}

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

/// Read the tree, mint its copy, and resolve every member. The subtree walk,
/// the shelf-id map and the member resolution are one pass because they
/// answer one question: what would the copy of this tree be?
pub fn plan_tree(rows: &[Row], shelves: &[Shelf], shelf_id: &str, now: u64) -> Option<TreePlan> {
    let original = shelf::find(shelves, shelf_id)?;
    let name = next_shelf_name(shelves, original.parent.as_deref(), &original.name);

    let mut order: Vec<&Shelf> = vec![original];
    let mut seen: HashSet<String> = HashSet::from([original.id.clone()]);
    let mut stack: Vec<&str> = vec![original.id.as_str()];
    while let Some(parent) = stack.pop() {
        for child in shelf::children_of(shelves, Some(parent)) {
            if !seen.insert(child.id.clone()) {
                continue;
            }
            order.push(child);
            stack.push(child.id.as_str());
        }
    }

    let mut fresh: HashMap<String, String> = HashMap::with_capacity(order.len());
    for level in &order {
        fresh.insert(level.id.clone(), id::next_shelf_id(now));
    }
    let skeleton: Vec<Shelf> = order
        .iter()
        .enumerate()
        .map(|(at, level)| Shelf {
            id: fresh.get(&level.id).cloned().unwrap_or_default(),
            name: if at == 0 { name.clone() } else { level.name.clone() },
            // One directory is one linked shelf: a second folder shelf of
            // one rung would be two doors to one directory with only one of
            // them on the ledger — and a rescan would re-hang a shelf the
            // reader made. The copy is the reader's own second tree.
            kind: ShelfKind::Virtual,
            books: level.books.clone(),
            // The subtree keeps its shape and closes no loop; a parent the
            // walk could not reach is no parent at all rather than an edge at
            // the ORIGINAL.
            parent: if at == 0 {
                original.parent.clone()
            } else {
                level.parent.as_deref().and_then(|p| fresh.get(p).cloned())
            },
            manual_parent: false,
        })
        .collect();

    let mut members: Vec<(String, Member)> = Vec::new();
    let mut taken: HashSet<String> = HashSet::new();
    for level in &order {
        for member in &level.books {
            if !taken.insert(member.clone()) {
                continue;
            }
            let what = match find_row(rows, member) {
                Some(Row::Book(book)) if !book.missing => Member::Copy {
                    new_id: id::next_id(now),
                    book: book.clone(),
                    // The fresh levels hold nothing to collide with, so the
                    // copy wears the original's own display name: the tree
                    // mirrors the tree, counter names and all.
                    shown: book.title(),
                },
                Some(Row::Link { name, target, .. }) => match link_at(rows, target) {
                    LinkAt::Book(book) => {
                        Member::Copy { new_id: id::next_id(now), book, shown: name.clone() }
                    }
                    LinkAt::Shelf => Member::Link(Row::link(
                        id::next_id(now),
                        name.clone(),
                        // A link at a shelf inside the tree points at the copy
                        // of that shelf; a link at one outside it keeps
                        // pointing where it did, because that shelf is
                        // nobody's to duplicate here.
                        fresh.get(target).cloned().unwrap_or_else(|| target.clone()),
                        now,
                    )),
                    LinkAt::Dead => Member::Skip,
                },
                _ => Member::Skip,
            };
            members.push((member.clone(), what));
        }
    }

    Some(TreePlan {
        name,
        label: original.name.clone(),
        original_id: original.id.clone(),
        shelves: skeleton,
        members,
    })
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

/// The run's closing sentence: one landing is named, a batch is counted by
/// the kinds it landed.
pub fn report(landed: &[Duplicated]) -> String {
    if landed.len() == 1 {
        return format!("Duplicated as “{}”.", landed[0].name);
    }
    let shelves = landed.iter().filter(|one| one.shelf).count();
    let books = landed.len() - shelves;
    let noun = match (books, shelves) {
        (_, 0) => "books",
        (0, _) => "shelves",
        _ => "shelves and books",
    };
    format!("Duplicated {} {noun}.", landed.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use library_core::testkit;

    /// One clock for the whole suite: the ids are a stamp plus a counter, so
    /// a fixed stamp still mints distinct ids.
    const NOW: u64 = 1_700_000_000_000;

    fn nested_state() -> (Vec<Row>, Vec<Shelf>) {
        (
            vec![testkit::markdown_row("b1"), testkit::markdown_row("b2")],
            vec![
                testkit::shelf("s1", "Shelf", &["b1"], None),
                testkit::shelf("s2", "Inside", &["b2"], Some("s1")),
                testkit::shelf("s3", "Other", &[], None),
            ],
        )
    }

    fn copy_of(shelves: &[Shelf], name: &str) -> Shelf {
        shelves
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("no shelf called {name}"))
            .clone()
    }

    /// The batch the store would have answered with, fabricated for the
    /// members a plan wants to copy: every copy lands, wearing its own item
    /// folder and its own measurement.
    fn everything_landed(plan: &TreePlan) -> HashMap<String, (String, Option<Fingerprint>)> {
        plan.members
            .iter()
            .filter_map(|(_, what)| match what {
                Member::Copy { new_id, .. } => Some((
                    new_id.clone(),
                    (
                        format!("/app/Library/items/{new_id}/source.md"),
                        Some(testkit::fp_n(7)),
                    ),
                )),
                _ => None,
            })
            .collect()
    }

    fn copies_of(plan: &TreePlan) -> Vec<&Member> {
        plan.members
            .iter()
            .map(|(_, what)| what)
            .filter(|what| matches!(what, Member::Copy { .. }))
            .collect()
    }

    #[test]
    fn a_link_at_a_shelf_duplicates_as_a_link_beside_it() {
        let mut rows = vec![
            testkit::row_at("b1", "/books/dune.md"),
            testkit::link("l1", "Dune", "s1"),
        ];
        let mut s1 = testkit::plain_shelf("s1", &["b1", "l1"]);
        s1.name = "Shelf".into();
        let mut shelves = vec![s1];

        let name = land_shelf_link(&mut rows, &mut shelves, "s1", "l1", "Dune", "s1", NOW);
        assert_eq!(name, "Dune_1", "the level's counter, not a collision");
        assert_eq!(rows.len(), 3);
        let dup = rows.iter().find(|r| r.id() != "b1" && r.id() != "l1").unwrap();
        match dup {
            Row::Link { target, name, .. } => {
                assert_eq!(target, "s1", "the pointer points where the pointer pointed");
                assert_eq!(name, "Dune_1");
            }
            Row::Book(_) => panic!("a shelf's link duplicates as a link"),
        }
        assert_eq!(
            shelves[0].books,
            vec!["b1".to_string(), "l1".to_string(), dup.id().to_string()],
            "filed right behind the row the reader pointed at"
        );
    }

    #[test]
    fn a_link_answers_with_what_it_points_at() {
        let rows = vec![
            testkit::row("b1"),
            testkit::link("l1", "Dune", "b1"),
            testkit::link("ls", "Shelf door", "s1"),
            testkit::link("dead", "Gone", "b9"),
        ];
        let mut missing = testkit::book("b2");
        missing.missing = true;
        let rows = [rows, vec![Row::Book(missing)]].concat();

        assert!(matches!(link_at(&rows, "s1"), LinkAt::Shelf));
        match link_at(&rows, "b1") {
            LinkAt::Book(book) => assert_eq!(book.id, "b1"),
            _ => panic!("a live book is a copy to make"),
        }
        assert!(
            matches!(link_at(&rows, "b9"), LinkAt::Dead),
            "a target no row answers for"
        );
        assert!(
            matches!(link_at(&rows, "b2"), LinkAt::Dead),
            "a target whose address died"
        );
        assert!(matches!(link_at(&rows, "l1"), LinkAt::Dead), "a link at a link");
    }

    #[test]
    fn plan_one_answers_per_entry() {
        let (mut rows, shelves) = nested_state();
        rows.push(testkit::link("l1", "Dune", "b1"));
        rows.push(testkit::link("dead", "Gone", "b9"));

        match plan_one(&rows, &shelves, "b1", NOW) {
            DupPlan::Book(copy) => {
                assert_eq!(copy.beside, "b1");
                assert_eq!(copy.shown, "b1", "the book's own display name");
            }
            other => panic!("a book asks for a copy: {other:?}"),
        }
        match plan_one(&rows, &shelves, "l1", NOW) {
            DupPlan::Book(copy) => {
                assert_eq!(copy.beside, "l1", "beside the link, not the target");
                assert_eq!(copy.shown, "Dune", "the link's own name");
            }
            other => panic!("a link at a book asks for a copy: {other:?}"),
        }
        match plan_one(&rows, &shelves, "dead", NOW) {
            DupPlan::Dead(note) => assert!(note.contains("Gone"), "the toast names the link"),
            other => panic!("a dead link is the one loud refusal: {other:?}"),
        }
        match plan_one(&rows, &shelves, "s2", NOW) {
            DupPlan::Tree(plan) => assert_eq!(plan.name, "Inside_1"),
            other => panic!("a shelf asks for a tree: {other:?}"),
        }
        assert!(
            matches!(plan_one(&rows, &shelves, "gone", NOW), DupPlan::Skip),
            "an id no list answers for is nobody's to duplicate"
        );
    }

    #[test]
    fn a_tree_plan_copies_every_member_once() {
        let (rows, shelves) = nested_state();
        let plan = plan_tree(&rows, &shelves, "s1", NOW).expect("the shelf is there");
        assert_eq!(plan.name, "Shelf_1", "the level's counter, not a collision");
        assert_eq!(plan.label, "Shelf", "the card names what was duplicated");

        // One member per row, first-seen order: b1 off the root, b2 off the
        // rung inside.
        let order: Vec<&str> = plan.members.iter().map(|(old, _)| old.as_str()).collect();
        assert_eq!(order, vec!["b1", "b2"]);
        let copies = copies_of(&plan);
        assert_eq!(copies.len(), 2, "every member becomes a copy");
        let ids: Vec<&str> = plan
            .members
            .iter()
            .filter_map(|(_, what)| match what {
                Member::Copy { new_id, .. } => Some(new_id.as_str()),
                _ => None,
            })
            .collect();
        assert_ne!(ids[0], "b1", "the copy is a row of its own");
        assert_ne!(ids[1], "b2", "the copy is a row of its own");
        assert_ne!(ids[0], ids[1], "two members, two copies");
    }

    #[test]
    fn a_missing_book_and_a_dead_link_are_skipped_not_shared() {
        let (mut rows, mut shelves) = nested_state();
        let mut missing = testkit::markdown_book("b3");
        missing.missing = true;
        rows.push(Row::Book(missing));
        rows.push(testkit::link("dead", "Gone", "b9"));
        shelves[0].books = vec!["b1".into(), "b3".into(), "dead".into()];

        let plan = plan_tree(&rows, &shelves, "s1", NOW).expect("the shelf is there");
        let kinds: Vec<&str> = plan
            .members
            .iter()
            .map(|(_, what)| match what {
                Member::Copy { .. } => "copy",
                Member::Link(_) => "link",
                Member::Skip => "skip",
            })
            .collect();
        assert_eq!(
            kinds,
            vec!["copy", "skip", "skip", "copy"],
            "the live books copy; the dead rows are no one's to share"
        );
    }

    #[test]
    fn a_link_at_a_shelf_inside_the_tree_points_at_the_copy() {
        let (mut rows, mut shelves) = nested_state();
        rows.push(testkit::link("l2", "Inside door", "s2"));
        rows.push(testkit::link("l3", "Other door", "s3"));
        shelves[0].books.push("l2".into());
        shelves[0].books.push("l3".into());

        let plan = plan_tree(&rows, &shelves, "s1", NOW).expect("the shelf is there");
        let fresh_inside = plan.shelves[1].id.clone();
        let targets: Vec<(String, String)> = plan
            .members
            .iter()
            .filter_map(|(old, what)| match what {
                Member::Link(row) => Some((old.clone(), row.target().unwrap().to_string())),
                _ => None,
            })
            .collect();
        assert_eq!(targets.len(), 2, "both links came along, each as its own row");
        assert_eq!(
            targets[0],
            ("l2".to_string(), fresh_inside),
            "a link at a shelf inside the tree points at the copy of that shelf"
        );
        assert_ne!(targets[0].1, "s2", "remapped, not shared");
        assert_eq!(
            targets[1],
            ("l3".to_string(), "s3".to_string()),
            "a link at a shelf outside the tree keeps pointing where it did"
        );
    }

    #[test]
    fn the_landing_remaps_the_tree_onto_fresh_copies() {
        let (mut rows, mut shelves) = nested_state();
        let plan = plan_tree(&rows, &shelves, "s1", NOW).expect("the shelf is there");
        let landed = everything_landed(&plan);
        let name = land_tree(&mut rows, &mut shelves, plan, &landed, NOW);

        assert_eq!(name, "Shelf_1");
        assert_eq!(shelves.len(), 5, "the shelf and the one inside it, copied");
        let root = copy_of(&shelves, "Shelf_1");
        assert_eq!(root.books.len(), 1, "one member, and it is not the original's");
        let fresh_id = root.books[0].clone();
        assert_ne!(fresh_id, "b1", "the tree holds its own copy, not the row itself");

        let dup = library_core::book::find_by_id(&rows, &fresh_id).expect("the copy's row");
        match &dup.origin {
            Origin::Stored { src, store } => {
                assert_eq!(
                    src.as_deref(),
                    Some("/books/b1.md"),
                    "read at its place, and the copy is the library's own anyway"
                );
                assert_eq!(
                    store,
                    &format!("/app/Library/items/{fresh_id}/source.md"),
                    "the copy's item folder is its own"
                );
            }
            other => panic!("the duplicate is stored, whatever the original was: {other:?}"),
        }
        assert_eq!(dup.title.as_deref(), Some("b1"), "the name the original showed");
        assert!(dup.title_locked, "a name the reader asked for is not debris");
        assert_eq!(dup.fp, testkit::fp_n(7), "known by its own measurement");
        assert!(!dup.fp_pending, "the measurement the copy rode home with landed");

        let inside = copy_of(&shelves, "Inside");
        assert_eq!(inside.books.len(), 1);
        assert_ne!(inside.books[0], "b2", "the rung's copy is its own row too");
        assert_eq!(
            inside.parent.as_deref(),
            Some(root.id.as_str()),
            "still inside the copy, not the original"
        );
        let level: Vec<&str> = shelf::children_of(&shelves, None)
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(level, vec!["Shelf", "Shelf_1", "Other"], "spliced in behind the original");
    }

    #[test]
    fn a_member_the_store_refused_is_dropped_not_shared() {
        let (mut rows, mut shelves) = nested_state();
        let plan = plan_tree(&rows, &shelves, "s1", NOW).expect("the shelf is there");
        let before = rows.len();
        let name = land_tree(&mut rows, &mut shelves, plan, &HashMap::new(), NOW);

        assert_eq!(name, "Shelf_1", "the shelf itself still landed");
        assert_eq!(shelves.len(), 5, "the tree keeps its shape");
        let root = copy_of(&shelves, "Shelf_1");
        assert!(
            root.books.is_empty(),
            "a copy that did not come home is not filed as the original"
        );
        assert_eq!(rows.len(), before, "no row landed for a copy that did not");
    }

    #[test]
    fn a_folder_shelf_duplicates_as_a_shelf_of_the_readers_own() {
        // One directory is one linked shelf: a second folder shelf of one
        // rung would be two doors to one directory with only one of them on
        // the ledger — and a rescan would re-hang a shelf the reader made.
        // The copy is the reader's own second tree over fresh copies of the
        // books.
        let (mut rows, mut shelves) = nested_state();
        shelves[0] = testkit::folder_shelf("s1", "Books", "f1", None, &["b1"], None);
        let plan = plan_tree(&rows, &shelves, "s1", NOW).expect("the shelf is there");
        assert_eq!(plan.name, "Books_1");
        assert!(
            plan.shelves[0].kind.folder_id().is_none(),
            "the copy is not a second shelf of the folder"
        );
        let landed = everything_landed(&plan);
        land_tree(&mut rows, &mut shelves, plan, &landed, NOW);

        let root = copy_of(&shelves, "Books_1");
        assert_ne!(root.books[0], "b1", "the folder's copy holds a copy, not the row");
        // The original is untouched: the copy is not a rung of the folder, so
        // a walk that mints the tree again mints the tree it already had.
        let original = shelf::find(&shelves, "s1").expect("the original stands");
        assert_eq!(original.kind.folder_id(), Some("f1"));
        assert_eq!(original.books, vec!["b1".to_string()]);
    }

    #[test]
    fn a_duplicate_of_a_duplicate_steps_the_shelf_counter() {
        let (mut rows, mut shelves) = nested_state();
        let first = plan_tree(&rows, &shelves, "s1", NOW).expect("the shelf is there");
        assert_eq!(first.name, "Shelf_1");
        let first_id = first.shelves[0].id.clone();
        land_tree(&mut rows, &mut shelves, first, &HashMap::new(), NOW);
        // Duplicating the copy steps rather than stacks, the reading a file
        // manager gives: the counter is not part of the name.
        assert_eq!(
            plan_tree(&rows, &shelves, &first_id, NOW).map(|plan| plan.name),
            Some("Shelf_2".to_string())
        );
    }

    #[test]
    fn a_shelf_that_is_not_there_duplicates_into_nothing() {
        let (rows, shelves) = nested_state();
        assert!(plan_tree(&rows, &shelves, "gone", NOW).is_none());
        // "All" is the book list and not a shelf, so it has no second
        // instance to make either — the pseudo-shelf's own answer everywhere
        // else.
        assert!(plan_tree(&rows, &shelves, library_core::shelf::ALL_SHELF, NOW).is_none());
        assert_eq!(shelves.len(), 3);
    }

    #[test]
    fn a_report_counts_the_kinds_it_landed() {
        let one = |name: &str, shelf| Duplicated { name: name.to_string(), shelf };
        assert_eq!(report(&[one("Dune_1", false)]), "Duplicated as “Dune_1”.");
        assert_eq!(report(&[one("Shelf_1", true)]), "Duplicated as “Shelf_1”.");
        assert_eq!(report(&[one("a", false), one("b", false)]), "Duplicated 2 books.");
        assert_eq!(report(&[one("a", true), one("b", true)]), "Duplicated 2 shelves.");
        assert_eq!(
            report(&[one("a", true), one("b", false), one("c", false)]),
            "Duplicated 3 shelves and books."
        );
    }
}
