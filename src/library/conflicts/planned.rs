//! The planned folder walk's screen: what the ledger admits, and which of
//! those arrivals meet a row already on the level they land on.

use std::collections::HashSet;
use library_core::book::{book_rows, Row};
use library_core::conflict::{collide, Arrival};
use library_core::folder::WatchedFolder;
use library_core::ledger::Registry;
use library_core::scan::FoundFile;
use library_core::shelf::Shelf;
use super::{existing_name_of, ConflictAsk};

/// The question and the placements a planned tree — *as new* or *merge* —
/// owes the files the library already holds. A rename or a merge re-seats
/// rows rather than minting them, so the file whose row is already here must
/// follow its rung onto the planned tree (or be asked about, when the
/// destination already wears its name); and the files the library did not
/// hold at all must be screened against the destination's own names.
///
/// The ledger's rule for what a walk admits is pure and lives in
/// `library_core::ledger`; this is the half that needs the shelf map the
/// merge is landing into.
pub struct PlannedScreen {
    /// The rows that move onto the planned tree: the row's id, and the file
    /// whose rung decides where it lands.
    pub replacements: Vec<(String, FoundFile)>,
    pub asks: Vec<ConflictAsk>,
}


/// `renames` and `into` are the run's own plan: an *as new* answer has no
/// destination shelf to screen against (every rung mints fresh), and a plain
/// walk screens nothing at all — the ledger's table is the whole of what it
/// admits.
#[allow(clippy::too_many_arguments)]
pub fn screen_planned(
    folder: &WatchedFolder,
    rows: &[Row],
    shelves: &[Shelf],
    registry: &Registry,
    found: &[FoundFile],
    plan: (bool, Option<&str>),
    adds: &mut Vec<FoundFile>,
    copy_paths: &HashSet<String>,
) -> PlannedScreen {
    let (renames, into) = plan;
    if !renames && into.is_none() {
        return PlannedScreen { replacements: Vec::new(), asks: Vec::new() };
    }
    let mut asks = Vec::new();
    // The merge's new arrivals land on a rung that may already wear their
    // name, and a collision there is the compact sheet's question too.
    if let Some(into) = into {
        adds.retain(|file| match merge_collision(folder, rows, shelves, into, file) {
            Some(ask) => {
                asks.push(ask);
                false
            }
            None => true,
        });
    }
    // And the files the library already holds are re-seated rather than
    // minted: a plan that renames or merges moves rows, and the row whose file
    // this is must follow its rung onto the planned tree.
    let mut replacements = Vec::new();
    for file in found {
        // A file the run owes a COPY of is the library's second instance,
        // never a row to move.
        if copy_paths.contains(&file.path) {
            continue;
        }
        let Some(row_id) = known_row(rows, registry, file) else {
            continue;
        };
        if let Some(into) = into
            && let Some(ask) = merge_collision(folder, rows, shelves, into, file)
        {
            asks.push(ask);
            continue;
        }
        replacements.push((row_id, file.clone()));
    }
    // A file the plan re-seats is not also an addition: one content is one
    // row, and the row is already here.
    adds.retain(|file| !replacements.iter().any(|(_, each)| each.path == file.path));
    PlannedScreen { replacements, asks }
}


/// The row that answers for a found file: by content identity first, which is
/// the ledger's answer, and by address second for a migrated row whose
/// placeholder identity no measurement ever matched.
fn known_row(rows: &[Row], registry: &Registry, file: &FoundFile) -> Option<String> {
    registry
        .get(&file.fp)
        .map(|known| known.id.clone())
        .or_else(|| {
            book_rows(rows)
                .find(|b| b.path() == file.path)
                .map(|b| b.id.clone())
        })
}


/// The merge's question about one file: the rung the merge files it onto, and
/// whether that rung already holds its name. `None` when the merge has no seat
/// for the file's rung, or when the seat is free.
fn merge_collision(
    folder: &WatchedFolder,
    rows: &[Row],
    shelves: &[Shelf],
    into: &str,
    file: &FoundFile,
) -> Option<ConflictAsk> {
    let key = folder.shelf_key(file);
    let target = if key.is_empty() {
        into.to_string()
    } else {
        folder.shelf_map.get(&key).cloned()?
    };
    let arrival = Arrival::import(file.clone(), target, None);
    let existing_id = collide(rows, shelves, &arrival)?;
    let existing_name = existing_name_of(rows, &existing_id, &arrival);
    Some(ConflictAsk::folder_merge(
        arrival,
        existing_id,
        existing_name,
        folder.mode(),
        folder.id.clone(),
    ))
}
