//! A shelf's duplicate: the whole subtree planned before any bytes move, and the
//! copy work it owes the store.

use std::collections::{HashMap, HashSet};

use library_core::book::{find_row, Book, Row};
use library_core::conflict::next_shelf_name;
use library_core::id;
use library_core::shelf::{self, Shelf, ShelfKind};
use library_core::wire::BookFileRequest;

use super::{link_at, LinkAt};

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
