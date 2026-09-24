//! The module's tests: the subjects beside this hold the code, and this
//! holds the cases they have to satisfy.

use super::*;

use std::collections::{BTreeMap, BTreeSet, HashSet};

use reader_core::format::Format;

use crate::book::Fingerprint;

use crate::scan::{selectable_formats, FoundFile};

use crate::shelf::Shelf;

use crate::tracking::TrackingTree;

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
fn a_file_stands_on_the_rung_its_own_subfolder_names() {
    let f = WatchedFolder {
        shelf_map: BTreeMap::from([
            ("".to_string(), "shelf1".to_string()),
            ("Fiction".to_string(), "shelf2".to_string()),
            ("Fiction/SciFi".to_string(), "shelf3".to_string()),
        ]),
        ..folder("/books")
    };
    let deep = "/books/Fiction/SciFi/dune.pdf";
    assert_eq!(f.rungs_for(deep), (Some("shelf3"), Some("shelf1")));
    // A drag from shelf3 to shelf2 has left the ground the tree names for
    // this file — the whole of the departure rule.
    assert_eq!(
        f.rungs_for("/books/Fiction/other.pdf"),
        (Some("shelf2"), Some("shelf1"))
    );
    assert_eq!(f.rungs_for("/books/top.pdf"), (Some("shelf1"), Some("shelf1")));
    assert_eq!(f.rungs_for("/books/Unmapped/x.pdf"), (None, Some("shelf1")));
    assert_eq!(f.rungs_for("/other/x.pdf"), (None, None));
}

#[test]
fn a_shape_answered_for_one_rung_cuts_the_rungs_under_it_alone() {
    let mut f = WatchedFolder {
        opts: FolderOpts {
            groups: false,
            ..FolderOpts::default()
        },
        shelf_map: BTreeMap::from([
            (String::new(), "root".to_string()),
            ("Fiction".to_string(), "fic".to_string()),
        ]),
        ..folder("/books")
    };
    assert_eq!(
        f.rung_for("Fiction/SciFi"),
        "",
        "one shelf files every address on its root rung"
    );
    assert_eq!(f.rungs_for("/books/Reference/x.pdf").0, Some("root"));
    // The nested folder the re-import answered for cuts its own rungs;
    // the tree above keeps the books never under it.
    f.set_shape("Fiction", true);
    assert_eq!(f.rung_for("Fiction"), "Fiction");
    assert_eq!(f.rung_for("Fiction/SciFi"), "Fiction/SciFi");
    assert_eq!(f.rung_for("Reference"), "", "the rest of the tree is where it was");
    assert_eq!(
        f.rungs_for("/books/Fiction/SciFi/dune.pdf").0,
        None,
        "no shelf stands for the rung the answer cut yet"
    );
    assert_eq!(f.rungs_for("/books/Fiction/other.pdf").0, Some("fic"));
}

#[test]
fn the_roots_answer_stands_for_the_whole_tree() {
    let mut f = WatchedFolder {
        opts: FolderOpts {
            groups: false,
            ..FolderOpts::default()
        },
        ..folder("/books")
    };
    f.set_shape("Fiction", true);
    assert_eq!(f.rung_for("Fiction/SciFi"), "Fiction/SciFi");
    f.set_shape("", true);
    assert!(f.opts.groups, "the root's answer IS the folder's own shape");
    assert!(f.shapes.is_empty(), "and it takes every deeper answer with it");
    assert_eq!(f.rung_for("Fiction/SciFi"), "Fiction/SciFi");
    f.set_shape("", false);
    assert_eq!(
        f.rung_for("Fiction/SciFi"),
        "",
        "one shelf puts every directory on the root rung"
    );
}

#[test]
fn a_folder_that_does_not_group_has_one_rung_for_every_file() {
    let f = WatchedFolder {
        opts: FolderOpts {
            groups: false,
            ..FolderOpts::default()
        },
        shelf_map: BTreeMap::from([
            ("".to_string(), "root".to_string()),
            ("Fiction".to_string(), "ignored".to_string()),
        ]),
        ..folder("/books")
    };
    assert_eq!(f.rungs_for("/books/Fiction/SciFi/dune.pdf"), (Some("root"), Some("root")));
    assert_eq!(f.rungs_for("/books/top.pdf"), (Some("root"), Some("root")));
}

#[test]
fn the_rung_a_walk_names_and_the_rung_an_address_names_agree() {
    // One key arithmetic behind both: a rescan and a drag cannot
    // disagree about where a file belongs.
    let f = WatchedFolder {
        shelf_map: BTreeMap::from([("Fiction/SciFi".to_string(), "shelf3".to_string())]),
        ..folder("/books")
    };
    let found = FoundFile {
        path: "/books/Fiction/SciFi/dune.pdf".into(),
        rel: "Fiction/SciFi/dune.pdf".into(),
        ext: "pdf".into(),
        size: 1,
        fp: fp(1),
    };
    assert_eq!(f.shelf_key(&found), "Fiction/SciFi");
    assert_eq!(f.rungs_for(&found.path).0, Some("shelf3"));
}

fn fp(n: u32) -> Fingerprint {
    Fingerprint {
        size: u64::from(n),
        mtime_ms: u64::from(n),
        head_hash: n,
    }
}

fn folder(root: &str) -> WatchedFolder {
    WatchedFolder {
        id: "f1".into(),
        root: root.into(),
        opts: FolderOpts::default(),
        placed: HashSet::new(),
        ignored: Vec::new(),
        shelf_map: BTreeMap::new(),
        last_seen: Vec::new(),
        scanned_ms: 0,
        tracking: TrackingTree::default(),
        shapes: crate::shape::ShapeTree::default(),
    }
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

#[test]
fn the_defaults_are_the_ones_the_sheet_opens_on() {
    let o = FolderOpts::default();
    assert_eq!(o.min_size, DEFAULT_MIN_SIZE);
    assert_eq!(o.min_size_label(), "30 KB");
    assert!(o.include_selected);
    assert!(o.in_place, "read in place is the mode the app always had");
    assert!(!o.watch, "watching is opt-in");
    assert!(o.groups);
    assert_eq!(o.formats.len(), selectable_formats().len());
}

#[test]
fn a_blob_from_before_the_folder_options_existed_loads_them() {
    let f: WatchedFolder =
        serde_json::from_str(r#"{"id":"f1","root":"/books"}"#).unwrap();
    assert_eq!(f.opts, FolderOpts::default());
    assert!(f.placed.is_empty() && f.ignored.is_empty());
    assert!(f.shelf_map.is_empty());
}

#[test]
fn the_size_dial_steps_in_kb_and_stops_at_its_bounds() {
    let mut o = FolderOpts::default();
    o.step_min_size(1);
    assert_eq!(o.min_size, 40 * 1024);
    o.step_min_size(-2);
    assert_eq!(o.min_size, 20 * 1024);
    for _ in 0..100 {
        o.step_min_size(-1);
    }
    assert_eq!(o.min_size, MIN_SIZE_FLOOR);
    assert_eq!(o.min_size_label(), "0 KB");
    for _ in 0..200 {
        o.step_min_size(1);
    }
    assert_eq!(o.min_size, MIN_SIZE_CEIL);
    assert_eq!(o.min_size_label(), "500 KB");
}

#[test]
fn a_sub_thousand_byte_threshold_still_prints_honestly() {
    let o = FolderOpts { min_size: 512, ..Default::default() };
    assert_eq!(o.min_size_label(), "0.5 KB");
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
fn a_blob_from_before_tracking_was_a_tree_keeps_watching() {
    // A folder from an older build carries `opts.watch` and no tree, and
    // an empty tree tracks nothing — without carrying the flag across, a
    // load would silently stop rescanning every watched folder.
    let raw = r#"{"id":"f1","root":"/books","opts":{"inPlace":true,"watch":true}}"#;
    let mut folders: Vec<WatchedFolder> = serde_json::from_str(&format!("[{raw}]")).unwrap();
    assert!(folders[0].tracking.is_empty(), "the old blob has no tree");
    assert!(folders[0].opts.watch, "and the flag it did have");
    sanitize(&mut folders);
    assert!(folders[0].tracked(), "the flag became the root rung's decision");
    assert!(folders[0].tracks_rung("Fiction"), "and the tree below it inherits");
    assert!(folders[0].opts.watch, "the flag is left agreed with the tree");
    let raw_off = r#"{"id":"f2","root":"/dvds","opts":{"inPlace":true,"watch":false}}"#;
    let mut off: Vec<WatchedFolder> =
        serde_json::from_str(&format!("[{raw_off}]")).unwrap();
    sanitize(&mut off);
    assert!(!off[0].tracked());
    // The tree is the answer from here on; a stale flag yields to it.
    let mut written = vec![mode("f3", "/books", true, false)];
    written[0].set_tracking("", true);
    assert!(written[0].opts.watch, "set_tracking mirrors the root onto the flag");
    written[0].opts.watch = false;
    sanitize(&mut written);
    assert!(written[0].tracked(), "the tree wins");
    assert!(written[0].opts.watch, "and the flag is brought back into agreement");
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
fn sanitize_dedupes_roots_and_clamps_the_dial() {
    let mut folders = vec![
        WatchedFolder {
            opts: FolderOpts {
                min_size: 10_000_000,
                ..FolderOpts::default()
            },
            ..folder("/books")
        },
        folder("/books"),
        folder(""),
        WatchedFolder {
            id: " ".into(),
            ..folder("/other")
        },
    ];
    sanitize(&mut folders);
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].opts.min_size, MIN_SIZE_CEIL);
}

#[test]
fn an_empty_format_set_is_not_a_folder_that_admits_nothing() {
    // A hand-edited blob or a format removed from the registry must not
    // silently turn a watched folder into a dead one.
    let mut folders = vec![WatchedFolder {
        opts: FolderOpts {
            formats: BTreeSet::new(),
            ..FolderOpts::default()
        },
        ..folder("/books")
    }];
    sanitize(&mut folders);
    assert_eq!(folders[0].opts.formats.len(), selectable_formats().len());
}

#[test]
fn a_backslash_never_survives_into_the_shelf_map() {
    // A `\` in a key means the writer did not normalise it; such a key
    // would never match a found file again.
    let mut folders = vec![WatchedFolder {
        shelf_map: BTreeMap::from([
            ("scifi".to_string(), "s1".to_string()),
            ("scifi\\deep".to_string(), "s2".to_string()),
        ]),
        ..folder("/books")
    }];
    sanitize(&mut folders);
    let keys: Vec<&String> = folders[0].shelf_map.keys().collect();
    assert_eq!(keys, vec!["scifi"]);
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

#[test]
fn a_folder_is_found_by_id_for_a_read_and_for_a_write() {
    let mut folders = vec![folder("/one"), folder("/two")];
    folders[1].id = "f2".into();
    assert_eq!(find(&folders, "f2").map(|f| f.root.as_str()), Some("/two"));
    assert!(find(&folders, "gone").is_none());
    // Through the writer, not the field: `set_tracking` keeps the tree and
    // the flag agreed.
    find_mut(&mut folders, "f2").unwrap().set_tracking("", true);
    assert!(find(&folders, "f2").is_some_and(|f| f.tracked() && f.opts.watch));
}

#[test]
fn the_two_switches_add_up_to_one_mode_and_its_questions() {
    // One of the four switch combinations is not offerable; the folding
    // happens where the mode is computed.
    let opts = |in_place: bool, watch: bool| FolderOpts {
        in_place,
        watch,
        ..FolderOpts::default()
    };
    assert_eq!(opts(true, false).mode(), FolderMode::LinkInPlace);
    assert_eq!(opts(true, true).mode(), FolderMode::LinkInPlaceWatched);
    assert_eq!(opts(false, false).mode(), FolderMode::Copy);
    assert_eq!(
        opts(false, true).mode(),
        FolderMode::Copy,
        "a copy does not care what the source folder does next"
    );
    assert!(opts(false, true).mode().copies_files());
    assert!(!opts(false, true).mode().reads_in_place());
    assert!(opts(true, true).mode().reads_in_place());
    assert!(!opts(true, true).mode().copies_files());
    assert!(opts(true, true).mode().tracks_new_files());
    assert!(!opts(true, false).mode().tracks_new_files());
    let mut folder = folder("/books");
    folder.opts.in_place = true;
    folder.set_tracking("", true);
    assert!(folder.opts.watch, "the root's answer is mirrored onto the flag");
    assert_eq!(folder.mode(), FolderMode::LinkInPlaceWatched);
    assert_eq!(folder.opts.mode(), folder.mode());
}

#[test]
fn a_folder_the_library_copies_is_owed_no_walk_whatever_its_tree_says() {
    // The unofferable pair (`in_place=false, watch=true`) folds into
    // `Copy`: a tree left standing on a copying folder is chrome, not a
    // second opinion.
    let copying = mode("f1", "/books", false, true);
    assert_eq!(copying.mode(), FolderMode::Copy, "a watching copy is a copy");
    assert!(copying.tracks_anything(), "and its tree is still standing");
    assert!(!copying.owes_walk(), "so nothing walks it");

    // A root turned off with one subfolder left on is still watched.
    let mut partly = mode("f2", "/dvds", true, true);
    partly.set_tracking("", false);
    partly.set_tracking("Films", true);
    assert!(!partly.opts.watch, "the root's own answer is off");
    assert!(partly.owes_walk(), "and the subfolder still owes the walk");

    let quiet = mode("f3", "/comics", true, false);
    assert!(!quiet.owes_walk());
}

/// The watch arrives as a tree, not the flag alone: the flag is now the
/// root rung's mirror, and a fixture setting only the flag would be a
/// folder this build never writes.
fn mode(id: &str, root: &str, in_place: bool, watch: bool) -> WatchedFolder {
    let mut folder = WatchedFolder {
        id: id.into(),
        opts: FolderOpts {
            in_place,
            watch,
            ..FolderOpts::default()
        },
        ..folder(root)
    };
    if watch {
        folder.set_tracking("", true);
    }
    folder
}
