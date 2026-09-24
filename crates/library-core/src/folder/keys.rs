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
