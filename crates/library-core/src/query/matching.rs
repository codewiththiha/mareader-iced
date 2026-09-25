//! Which books a query's terms match, and how one term hits a field.

use crate::book::Book;

/// Whether `at` starts a word, so a match there is a match on a boundary the
/// reader can see.
fn is_boundary(hay: &[char], at: usize) -> bool {
    at == 0 || matches!(hay[at - 1], ' ' | '-' | '_' | '.' | '/' | ':' | '(' | ')')
}

/// One fold per char, no allocation: the hot path runs per keystroke.
fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// One match: its score and the character spans it lit up, merged so a
/// consecutive run is one span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Match {
    pub(super) score: i32,
    pub(super) spans: Vec<(usize, usize)>,
}

fn merge_spans(indices: &[usize]) -> Vec<(usize, usize)> {
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for &at in indices {
        match spans.last_mut() {
            Some(last) if last.1 == at => last.1 = at + 1,
            _ => spans.push((at, at + 1)),
        }
    }
    spans
}

/// Match `term` against `field`: substring first, then a scored subsequence.
/// `None` when the term is not in the field, or only in a subsequence too
/// scattered to be a match.
///
/// The scatter floor is two points per term character — a dropped-vowel shape
/// (`mthmtcl`, `dne`) clears it; a sprinkle across a long field does not.
pub(super) fn term_match(field: &str, term: &str) -> Option<Match> {
    let term: Vec<char> = term.chars().map(fold).collect();
    let q = term.len();
    if q == 0 {
        return None;
    }
    let hay: Vec<char> = field.chars().map(fold).collect();
    if q > hay.len() {
        return None;
    }
    // Substring first: the first occurrence, case-folded, one walk.
    let mut at = 0;
    while at + q <= hay.len() {
        if hay[at..at + q] == term[..] {
            let mut score = 5 * q as i32 - 3;
            if is_boundary(&hay, at) {
                score += 4;
            }
            return Some(Match {
                score,
                spans: vec![(at, at + q)],
            });
        }
        at += 1;
    }
    // Fuzzy pass: one ordered walk, scoring runs and boundaries and charging
    // the gaps.
    let mut score = 0i32;
    let mut hits: Vec<usize> = Vec::with_capacity(q);
    let mut prev: Option<usize> = None;
    let mut ti = 0usize;
    for (at, c) in hay.iter().copied().enumerate() {
        if c != term[ti] {
            continue;
        }
        let mut add = 2;
        match prev {
            Some(p) if p + 1 == at => add += 3,
            Some(p) => add -= (at - p - 1).min(8) as i32,
            None => {}
        }
        if is_boundary(&hay, at) {
            add += 4;
        }
        score += add;
        hits.push(at);
        prev = Some(at);
        ti += 1;
        if ti == q {
            break;
        }
    }
    if ti < q {
        return None;
    }
    if score < 2 * q as i32 {
        return None;
    }
    Some(Match {
        score,
        spans: merge_spans(&hits),
    })
}

/// Whether a book survives `query`: every whitespace-separated term must
/// match some field — title, author or address — in any order, so "herbert
/// dune" and "dune herbert" answer the same.
pub fn matches(book: &Book, query: &str) -> bool {
    if !is_active(query) {
        return true;
    }
    let title = book.title();
    let author = book.author();
    let path = book.path();
    query.split_whitespace().all(|term| {
        term_match(&title, term).is_some()
            || author.as_deref().is_some_and(|a| term_match(a, term).is_some())
            || term_match(path, term).is_some()
    })
}

/// [`matches`] without the book: a shelf on the page is searched by the same
/// bar as the books on it, and the two must agree or a query would hide a
/// shelf whose name it matched.
pub fn matches_terms(text: &str, query: &str) -> bool {
    if !is_active(query) {
        return true;
    }
    query
        .split_whitespace()
        .all(|term| term_match(text, term).is_some())
}

/// Whether the bar holds a query; blank and all-spaces both mean "show
/// everything".
pub fn is_active(query: &str) -> bool {
    !query.trim().is_empty()
}
