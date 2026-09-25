//! Shelves: an ordered list of book ids, and the kinds that produce one.
//!
//! A shelf holds membership and nothing else — no copies, no paths, no
//! filesystem intent — which makes dragging a book between shelves safe by
//! construction: a drop can only change an ordered list of ids.

use serde::{Deserialize, Serialize};

mod content;
mod family;
mod members;
mod tree;

pub use content::{badge_kind, ContentKind};
pub use family::{departing_moves, departs_on_move, family_for};
pub use members::{containing, forget, forget_everywhere, members_of, place, shelf_add};
pub use tree::{
    ancestors, can_nest, children_of, lift_children, rehang_moves, reparent, rung_above, rungs_of,
    subtree_ids,
};

/// The pseudo-shelf holding every book: the library's root level, ordered by
/// the persisted book list. Not a [`Shelf`].
pub const ALL_SHELF: &str = "all";

/// What produced a shelf, which decides whether the UI offers it a folder glyph, a watch dot, or a rename.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ShelfKind {
    /// The default, because a shelf a blob does not describe is one the reader made.
    #[default]
    Virtual,
    /// Cut from a watched folder's tree. `rel` is the subfolder within
    /// [`crate::folder::WatchedFolder::root`], `None` for the root itself.
    Folder {
        #[serde(rename = "folderId")]
        folder_id: String,
        #[serde(default)]
        rel: Option<String>,
    },
    /// A rung that left its tree: the reader moved it off the seat the
    /// folder's shelves name for it and the move paid the copy, so its books
    /// are the library's own and no tree answers for this shelf any more.
    Departed,
}

impl ShelfKind {
    /// The folder a shelf is a rung of; a virtual or departed shelf is
    /// nobody's rung.
    pub fn folder_id(&self) -> Option<&str> {
        match self {
            ShelfKind::Virtual | ShelfKind::Departed => None,
            ShelfKind::Folder { folder_id, .. } => Some(folder_id),
        }
    }

    pub fn is_folder_root(&self) -> bool {
        matches!(self, ShelfKind::Folder { rel: None, .. })
    }

    /// The rung this shelf stands on, keyed like the folder's own map: the
    /// empty string for a watched root and for a shelf no directory names
    /// (virtual or departed). One spelling of "which rung is this" for
    /// callers asking a standing shelf rather than a ledger.
    pub fn rung(&self) -> &str {
        match self {
            ShelfKind::Virtual | ShelfKind::Departed => "",
            ShelfKind::Folder { rel, .. } => rel.as_deref().unwrap_or(""),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shelf {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: ShelfKind,
    /// Member row ids in the reader's order: a shelf is a list, not a set.
    #[serde(default)]
    pub books: Vec<String>,
    /// Defaulted: a blob written before shelves could nest has no `parent`
    /// key, and every shelf in it is a root shelf.
    #[serde(default)]
    pub parent: Option<String>,
    /// The reader moved this shelf by hand off the seat its folder's shelves
    /// name for it, so its place is the reader's, not the disk's: a rescan
    /// re-hangs the shelves it owns and skips one wearing this mark. Written
    /// by [`reparent`], which clears the mark when a hand puts the shelf back.
    #[serde(default)]
    pub manual_parent: bool,
}

impl Shelf {
    pub fn is_folder(&self) -> bool {
        self.kind.folder_id().is_some()
    }

    /// A shelf the reader made (or a copy's landing level): no folder answers
    /// for it. One constructor, so the field list is answered here rather than
    /// spelled out at every mint.
    pub fn virtual_shelf(
        id: impl Into<String>,
        name: impl Into<String>,
        parent: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind: ShelfKind::Virtual,
            books: Vec::new(),
            parent,
            manual_parent: false,
        }
    }

    /// A rung a folder's tree cut: `rel` is the subfolder it stands for,
    /// `None` for the root. A minted rung starts on the tree's own ground
    /// (`manual_parent` false); a hand-move sets the mark later.
    pub fn folder_shelf(
        id: impl Into<String>,
        name: impl Into<String>,
        folder_id: impl Into<String>,
        rel: Option<String>,
        parent: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind: ShelfKind::Folder {
                folder_id: folder_id.into(),
                rel,
            },
            books: Vec::new(),
            parent,
            manual_parent: false,
        }
    }
}

/// `None` for [`ALL_SHELF`]: the caller falls back to the whole book list.
pub fn find<'a>(shelves: &'a [Shelf], id: &str) -> Option<&'a Shelf> {
    if id == ALL_SHELF {
        return None;
    }
    shelves.iter().find(|s| s.id == id)
}

/// Like [`find`], and the reason both special-case [`ALL_SHELF`]: the
/// pseudo-shelf is the book list and has no member list, so a caller handing
/// a route's shelf id straight in gets `None` and takes its root-level branch.
pub fn find_mut<'a>(shelves: &'a mut [Shelf], id: &str) -> Option<&'a mut Shelf> {
    if id == ALL_SHELF {
        return None;
    }
    shelves.iter_mut().find(|s| s.id == id)
}

/// Drop shelves with no id or name and any row wearing the [`ALL_SHELF`] id,
/// dedupe by id (first wins), drop blank and duplicate members, and cut the
/// nesting graph back to a forest. Idempotent.
pub fn sanitize(shelves: &mut Vec<Shelf>) {
    let mut seen = std::collections::HashSet::new();
    shelves.retain(|s| {
        !s.id.trim().is_empty() && !s.name.trim().is_empty() && s.id != ALL_SHELF
    });
    shelves.retain(|s| seen.insert(s.id.clone()));
    for s in shelves.iter_mut() {
        let mut members = std::collections::HashSet::new();
        s.books.retain(|m| !m.trim().is_empty() && members.insert(m.clone()));
    }

    // A parent that is the shelf itself, or that names no shelf, renders on
    // no level: both collapse to the root.
    for s in shelves.iter_mut() {
        if s.parent.as_deref() == Some(s.id.as_str()) {
            s.parent = None;
        }
    }
    // Owned ids: the pass below writes to the list it reads names from.
    let ids: std::collections::HashSet<String> =
        shelves.iter().map(|s| s.id.clone()).collect();
    for s in shelves.iter_mut() {
        if s.parent.as_deref().is_some_and(|p| !ids.contains(p)) {
            s.parent = None;
        }
    }

    // Then the cycles, which no single row shows. Only shelves ON a loop are
    // cut: one that merely leads into a loop keeps its parent and becomes a
    // root shelf's child once the loop below it is open.
    let on_a_cycle: std::collections::HashSet<String> = {
        let parents: std::collections::HashMap<&str, Option<&str>> = shelves
            .iter()
            .map(|s| (s.id.as_str(), s.parent.as_deref()))
            .collect();
        let mut out = std::collections::HashSet::new();
        for s in shelves.iter() {
            let mut current = parents.get(s.id.as_str()).copied().flatten();
            for _ in 0..=shelves.len() {
                let Some(parent_id) = current else {
                    break;
                };
                if parent_id == s.id {
                    out.insert(s.id.clone());
                    break;
                }
                current = parents.get(parent_id).copied().flatten();
            }
        }
        out
    };
    for s in shelves.iter_mut() {
        if on_a_cycle.contains(&s.id) {
            s.parent = None;
        }
    }
}


#[cfg(test)]
mod kit;

#[cfg(test)]
mod tests;
