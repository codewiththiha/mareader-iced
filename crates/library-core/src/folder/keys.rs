//! Folder keys and the paths they stand for: a key is the folder-relative path of
//! a rung, and these turn one into the other.

/// `path` relative to `root`, `/`-separated, no leading or trailing separator.
/// `Some("")` when the two name the same directory, `None` when `path` is not
/// inside `root` — a directory edge rather than a string prefix, which keeps
/// "/bookshelf" out of "/book".
pub fn rel_under(path: &str, root: &str) -> Option<String> {
    fn norm(p: &str) -> String {
        p.trim_end_matches(['/', '\\']).replace('\\', "/")
    }
    let (path, root) = (norm(path), norm(root));
    if path == root {
        return Some(String::new());
    }
    let rest = path.strip_prefix(root.as_str())?.strip_prefix('/')?;
    (!rest.is_empty()).then(|| rest.to_string())
}

/// Every rung of a shelf key's path, root first and the key itself last: `""`, `"2"`, `"2/deep"`.
pub fn key_chain(key: &str) -> Vec<&str> {
    let mut out = vec![""];
    if key.is_empty() {
        return out;
    }
    for (at, _) in key.match_indices('/') {
        out.push(&key[..at]);
    }
    out.push(key);
    out
}

/// The rung a shelf key sits inside: `"2/deep"` is inside `"2"`, and the root is inside nothing.
pub fn parent_key(key: &str) -> Option<&str> {
    if key.is_empty() {
        return None;
    }
    match key.rfind('/') {
        Some(at) => Some(&key[..at]),
        None => Some(""),
    }
}

/// Whether a rung key stands inside a zone: the zone itself or anywhere below
/// it. The empty zone is the whole tree. Directory-edge matching, the rule
/// [`rel_under`] gives for addresses.
pub fn key_in_zone(key: &str, zone: &str) -> bool {
    if zone.is_empty() {
        return true;
    }
    key == zone
        || key
            .strip_prefix(zone)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The address a rung's directory stands at: [`rel_under`] run backwards. One
/// function per direction, so a shelf's ground and its folder's map cannot
/// drift about what a key names.
pub fn dir_of_rung(root: &str, rel: &str) -> String {
    if rel.is_empty() {
        return root.to_string();
    }
    format!("{}/{}", root.trim_end_matches(['/', '\\']), rel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use crate::folder::kit::{folder, fp};
    use crate::folder::tombstone::Tombstone;
    use crate::scan::FoundFile;
    use crate::shelf::Shelf;
    use reader_core::format::Format;

    #[test]
    fn a_key_is_a_chain_of_rungs_root_first() {
        assert_eq!(key_chain(""), vec![""]);
        assert_eq!(key_chain("2"), vec!["", "2"]);
        assert_eq!(key_chain("2/deep"), vec!["", "2", "2/deep"]);
        assert_eq!(parent_key(""), None);
        assert_eq!(parent_key("2"), Some(""));
        assert_eq!(parent_key("2/deep"), Some("2"));
    }

    #[test]
    fn inside_a_folder_starts_on_a_directory_edge() {
        assert_eq!(rel_under("/books/a/b.pdf", "/books").as_deref(), Some("a/b.pdf"));
        assert_eq!(rel_under("/books/b.pdf", "/books").as_deref(), Some("b.pdf"));
        assert_eq!(rel_under("/books", "/books").as_deref(), Some(""));
        assert_eq!(rel_under("/books/", "/books/").as_deref(), Some(""));
        assert_eq!(rel_under("/bookshelf/a.pdf", "/book"), None);
        assert_eq!(rel_under("/other/a.pdf", "/books"), None);
        // Windows paths answer in `/` like every other path in the ledger: a
        // key holding a `\` would never match a found file again.
        assert_eq!(rel_under("C:\\books\\a\\b.pdf", "C:\\books").as_deref(), Some("a/b.pdf"));
    }

    #[test]
    fn a_zone_holds_its_own_key_and_the_rungs_below_it() {
        assert!(key_in_zone("2", "2"));
        assert!(key_in_zone("2/deep", "2"));
        assert!(key_in_zone("2/deep/deeper", "2"));
        assert!(!key_in_zone("20", "2"));
        assert!(!key_in_zone("20/deep", "2"));
        assert!(!key_in_zone("3", "2"));
        assert!(!key_in_zone("", "2"));
        assert!(key_in_zone("", ""));
        assert!(key_in_zone("2", ""));
        assert!(key_in_zone("2/deep", ""));
    }

    #[test]
    fn a_rung_s_address_is_its_key_joined_back_onto_the_root() {
        assert_eq!(dir_of_rung("/books", ""), "/books");
        assert_eq!(dir_of_rung("/books", "2/3"), "/books/2/3");
        assert_eq!(dir_of_rung("/books/", "2"), "/books/2");
        assert_eq!(dir_of_rung("C:\\books", "2"), "C:\\books/2");
        let dir = dir_of_rung("/books", "2/3");
        assert_eq!(rel_under(&dir, "/books").as_deref(), Some("2/3"));
    }

    #[test]
    fn the_ledger_skips_what_it_placed_and_honours_a_tombstone() {
        let mut f = folder("/books");
        assert!(!f.is_ignored(&fp(1)));
        f.mark_placed(fp(1));
        assert!(f.placed.contains(&fp(1)));
        assert!(!f.is_ignored(&fp(1)));
        f.ignored.push(stone(1));
        assert!(f.is_ignored(&fp(1)), "a removal outranks everything");
    }

    #[test]
    fn grouping_decides_whether_a_subfolder_is_its_own_shelf() {
        let mut f = folder("/books");
        let found = FoundFile {
            path: "/books/scifi/dune.pdf".into(),
            rel: "scifi/dune.pdf".into(),
            ext: "pdf".into(),
            size: 1,
            fp: fp(1),
        };
        assert_eq!(f.shelf_key(&found), "scifi");
        f.opts.groups = false;
        assert_eq!(f.shelf_key(&found), "", "one flat shelf for the whole tree");
        f.opts.groups = true;
        let at_root = FoundFile { rel: "dune.pdf".into(), ..found };
        assert_eq!(f.shelf_key(&at_root), "");
    }

    #[test]
    fn the_shelf_map_reuses_the_shelf_it_minted() {
        let mut f = folder("/books");
        let mut made: Vec<(String, String, String, Option<String>)> = Vec::new();
        let mut seq = 0usize;
        let leaf = f.shelf_chain_for(
            "scifi/deep",
            |_| {
                seq += 1;
                format!("s{seq}")
            },
            |rung| rung.rsplit('/').next().unwrap_or(rung).to_string(),
            |rung, id, name, parent| made.push((rung.to_string(), id.to_string(), name, parent)),
        );
        assert_eq!(
            made.iter().map(|(rung, _, _, _)| rung.as_str()).collect::<Vec<_>>(),
            vec!["", "scifi", "scifi/deep"]
        );
        assert_eq!(made[2].2, "deep", "the leaf is named by its own subfolder");
        assert_eq!(made[0].3, None, "the folder's own shelf hangs at the level it was on");
        assert_eq!(made[1].3.as_deref(), Some(made[0].1.as_str()));
        assert_eq!(made[2].3.as_deref(), Some(made[1].1.as_str()));
        assert_eq!(leaf, made[2].1);
        assert_eq!(f.shelf_map.len(), 3);

        made.clear();
        let again = f.shelf_chain_for(
            "scifi/deep",
            |_| {
                seq += 1;
                format!("s{seq}")
            },
            |rung| rung.to_string(),
            |rung, id, name, parent| made.push((rung.to_string(), id.to_string(), name, parent)),
        );
        assert_eq!(again, leaf);
        assert!(made.is_empty(), "no rung was minted, so none was reported");

        let sibling = f.shelf_chain_for(
            "scifi/deep/er",
            |_| {
                seq += 1;
                format!("s{seq}")
            },
            |rung| rung.to_string(),
            |rung, id, name, parent| made.push((rung.to_string(), id.to_string(), name, parent)),
        );
        assert_eq!(made.len(), 1, "only the new leaf");
        assert_eq!(made[0].3.as_deref(), Some(leaf.as_str()));
        assert_ne!(sibling, leaf);
    }

    #[test]
    fn turning_a_rung_off_below_a_watched_root_leaves_the_root_watching() {
        // What the single flag could not express: the root keeps its answer,
        // the rung below does not.
        let mut f = folder("/books");
        f.set_tracking("", true);
        assert!(f.tracked() && f.tracks_rung("Fiction") && f.tracks_rung("Fiction/SciFi"));
        f.set_tracking("Fiction", false);
        assert!(f.tracked(), "the tree is still watched");
        assert!(f.opts.watch, "and the flag still says so");
        assert!(!f.tracks_rung("Fiction"), "the rung turned off is off");
        assert!(!f.tracks_rung("Fiction/SciFi"), "and so is everything below it");
        assert!(f.tracks_rung("Poetry"), "a sibling is untouched");
        f.tracking.set("Fiction", crate::tracking::Track::Inherit);
        assert!(f.tracks_rung("Fiction/SciFi"));
    }

    #[test]
    fn a_map_pointer_at_a_shelf_that_went_is_cut() {
        // A dead pointer is a rung the walk reuses instead of minting; every
        // placement that rides it lands on no shelf at all.
        let standing = [crate::testkit::folder_shelf("s1", "Books", "f1", None, &[], None)];
        let mut f = folder("/books");
        f.shelf_map = BTreeMap::from([
            (String::new(), "s1".to_string()),
            ("scifi".to_string(), "gone".to_string()),
        ]);
        assert!(f.prune_shelf_map(&standing), "a dead pointer is news");
        let keys: Vec<&String> = f.shelf_map.keys().collect();
        assert_eq!(keys, vec![""], "and the rung that stands is kept");
        assert!(
            !f.prune_shelf_map(&standing),
            "cutting it once is the whole of it"
        );
        let none: [Shelf; 0] = [];
        assert!(f.prune_shelf_map(&none));
        assert!(f.shelf_map.is_empty());
    }

    fn stone(n: u32) -> Tombstone {
        Tombstone {
            fp: fp(n),
            title: Some(format!("Book {n}")),
            format: Format::Pdf,
            last_path: format!("/books/{n}.pdf"),
            shelf_id: None,
            removed_ms: 5,
            moved: false,
            returned_row: None,
        }
    }
}
