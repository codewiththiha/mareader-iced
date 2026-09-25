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
mod tests {
    use super::*;
    use crate::shelf::kit::{cut, ids, ids_of, nested, plain, shelf};
    use crate::shelf::tree::{children_of, reparent, rung_above};

    #[test]
    fn the_pseudo_shelf_is_not_a_shelf_to_either_lookup() {
        // `sanitize` drops a row wearing the id; the lookups answer it as a rule.
        let mut shelves = vec![plain("s", &["b1"])];
        assert!(find(&shelves, ALL_SHELF).is_none());
        assert!(find_mut(&mut shelves, ALL_SHELF).is_none());
        assert_eq!(find(&shelves, "s").map(|s| s.name.as_str()), Some("s"));
        find_mut(&mut shelves, "s").unwrap().name = "renamed".into();
        assert_eq!(find(&shelves, "s").map(|s| s.name.as_str()), Some("renamed"));
        assert!(find_mut(&mut shelves, "gone").is_none());
    }

    #[test]
    fn all_is_not_a_shelf_and_never_looks_like_one() {
        let shelves = vec![shelf("s1", "Sci-fi", &["a"])];
        assert!(find(&shelves, ALL_SHELF).is_none());
        assert!(find(&shelves, "s1").is_some());
        assert!(find(&shelves, "nope").is_none());
    }

    #[test]
    fn a_pseudo_all_shelf_never_survives_a_load() {
        // A blob carrying an "all" row must not become a tile duplicating the
        // whole library.
        let mut shelves = vec![shelf(ALL_SHELF, "All", &["a"]), shelf("s1", "Sci-fi", &["a"])];
        sanitize(&mut shelves);
        let ids: Vec<&str> = shelves.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["s1"]);
    }

    #[test]
    fn a_folder_shelf_knows_its_folder_and_its_subfolder() {
        let root = Shelf {
            kind: ShelfKind::Folder {
                folder_id: "f1".into(),
                rel: None,
            },
            ..shelf("s1", "Books", &[])
        };
        let sub = Shelf {
            kind: ShelfKind::Folder {
                folder_id: "f1".into(),
                rel: Some("scifi".into()),
            },
            ..shelf("s2", "scifi", &[])
        };
        assert!(root.is_folder() && sub.is_folder());
        assert!(root.kind.is_folder_root() && !sub.kind.is_folder_root());
        assert_eq!(sub.kind.folder_id(), Some("f1"));
        assert_eq!(shelf("s3", "Mine", &[]).kind.folder_id(), None);
    }

    #[test]
    fn a_folder_kind_persists_with_its_folder_id() {
        let s = Shelf {
            kind: ShelfKind::Folder {
                folder_id: "f1".into(),
                rel: Some("scifi".into()),
            },
            ..shelf("s2", "scifi", &["a"])
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"kind\":\"folder\""), "{json}");
        assert!(json.contains("\"folderId\":\"f1\""), "{json}");
        let back: Shelf = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
        // A blob from before `rel` existed is the folder's root shelf.
        let older: Shelf = serde_json::from_str(
            r#"{"id":"s2","name":"scifi","kind":{"kind":"folder","folderId":"f1"}}"#,
        )
        .unwrap();
        assert!(older.kind.is_folder_root());
    }

    #[test]
    fn sanitize_dedupes_shelves_and_their_members() {
        let mut shelves = vec![
            shelf("s1", "One", &["a", "a", "b", "  "]),
            shelf("s1", "One again", &["c"]),
            shelf("  ", "Nameless id", &[]),
            shelf("s3", "   ", &[]),
        ];
        sanitize(&mut shelves);
        assert_eq!(shelves.len(), 1);
        assert_eq!(ids(&shelves[0].books), vec!["a", "b"]);
    }

    #[test]
    fn a_shelf_without_a_kind_still_loads() {
        let s: Shelf = serde_json::from_str(r#"{"id":"s1","name":"One"}"#).unwrap();
        assert_eq!(s.kind, ShelfKind::Virtual);
        assert!(s.books.is_empty());
        assert_eq!(s.parent, None, "a blob from before nesting has no parent");
    }

    #[test]
    fn a_hand_moved_watched_shelf_keeps_its_move_and_says_so() {
        let mut shelves = vec![
            shelf("s1", "Fiction", &[]),
            Shelf {
                kind: ShelfKind::Folder {
                    folder_id: "f1".into(),
                    rel: Some("scifi".into()),
                },
                ..shelf("s2", "Watched", &[])
            },
        ];
        // The reader's hand beats the disk's shape: the move lands, and the
        // mark makes it a promise the rescan keeps.
        assert!(reparent(&mut shelves, "s2", Some("s1")));
        assert_eq!(shelves[1].parent.as_deref(), Some("s1"));
        assert!(shelves[1].manual_parent);
        assert!(matches!(
            shelves[1].kind,
            ShelfKind::Folder { ref rel, .. } if rel.as_deref() == Some("scifi")
        ));
        assert!(reparent(&mut shelves, "s1", None));
        assert!(!shelves[0].manual_parent);
        assert!(!reparent(&mut shelves, "s1", Some("s2")));
        assert_eq!(shelves[0].parent, None);
    }

    #[test]
    fn a_shelf_from_before_the_mark_existed_is_the_scans() {
        // No `manualParent` key in an older blob is not a hand-move.
        let older: Shelf = serde_json::from_str(
            r#"{"id":"s2","name":"scifi","kind":{"kind":"folder","folderId":"f1"}}"#,
        )
        .unwrap();
        assert!(!older.manual_parent);
        let moved = Shelf {
            manual_parent: true,
            parent: Some("s1".into()),
            ..older.clone()
        };
        let json = serde_json::to_string(&moved).unwrap();
        assert!(json.contains("\"manualParent\":true"), "{json}");
        let back: Shelf = serde_json::from_str(&json).unwrap();
        assert_eq!(back, moved);
    }

    #[test]
    fn sanitize_collapses_a_parent_that_names_no_shelf() {
        let mut shelves = vec![nested("s1", "Orphan", "gone"), nested("s2", "Filed", "s1")];
        sanitize(&mut shelves);
        assert_eq!(shelves[0].parent, None, "an orphan is a root, not a hole");
        assert_eq!(shelves[1].parent.as_deref(), Some("s1"), "its child is untouched");
        assert_eq!(ids_of(&children_of(&shelves, None)), vec!["s1"]);
    }

    #[test]
    fn sanitize_opens_a_cycle_and_leaves_the_branch_above_it_alone() {
        let mut shelves = vec![
            nested("s1", "Leads in", "s2"),
            nested("s2", "Loop a", "s3"),
            nested("s3", "Loop b", "s2"),
            shelf("s4", "Unrelated", &[]),
        ];
        sanitize(&mut shelves);
        assert_eq!(shelves[1].parent, None, "both edges of the loop are cut");
        assert_eq!(shelves[2].parent, None);
        assert_eq!(
            shelves[0].parent.as_deref(),
            Some("s2"),
            "a shelf that leads into the loop keeps the parent it had"
        );
        assert_eq!(shelves[3].parent, None);
        let mut on_a_level: Vec<&str> = [None, Some("s1"), Some("s2"), Some("s3")]
            .iter()
            .flat_map(|at| children_of(&shelves, *at))
            .map(|s| s.id.as_str())
            .collect();
        on_a_level.sort_unstable();
        assert_eq!(on_a_level, vec!["s1", "s2", "s3", "s4"]);
        let before = shelves.clone();
        sanitize(&mut shelves);
        assert_eq!(shelves, before);
    }

    #[test]
    fn a_shelf_filed_inside_itself_is_put_back_at_the_root() {
        let mut shelves = vec![Shelf {
            parent: Some("s1".into()),
            ..shelf("s1", "Self", &[])
        }];
        sanitize(&mut shelves);
        assert_eq!(shelves[0].parent, None);
        assert_eq!(ids_of(&children_of(&shelves, None)), vec!["s1"]);
    }

    #[test]
    fn a_parent_persists_with_the_shelf() {
        let s = nested("s2", "Sci-fi", "s1");
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"parent\":\"s1\""), "{json}");
        let back: Shelf = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn the_rung_a_key_hangs_on_is_the_nearest_one_still_standing() {
        let shelves = vec![
            cut("r", "f1", None, None),
            cut("first", "f1", Some("1st"), Some("r")),
            cut("deep", "f1", Some("1st/2nd/books"), Some("first")),
        ];
        assert_eq!(
            rung_above(&shelves, "f1", "1st/2nd"),
            Some("first".to_string()),
            "the level between them is gone, so one rung up is the answer"
        );
        assert_eq!(
            rung_above(&shelves, "f1", "1st/2nd/books"),
            Some("first".to_string()),
            "and a shelf two levels under a hole comes up to the same rung"
        );
        assert_eq!(rung_above(&shelves, "f1", "1st"), Some("r".to_string()));
        assert_eq!(
            rung_above(&shelves, "f1", ""),
            None,
            "the root rung is the one key with nothing above it"
        );
        let bare: Vec<Shelf> = Vec::new();
        assert_eq!(rung_above(&bare, "f1", "1st"), None);
    }
}

#[cfg(test)]
mod kit;
