//! The suggestions a query earns: how its matches rank, and which to offer.

use crate::book::{Book, Row, book_rows};

use super::matching::{is_active, matches, matches_terms, term_match};

/// Seven rows fill the suggestion panel without scrolling.
pub const SUGGEST_LIMIT: usize = 7;

/// One suggestion: the book and the spans each of its three fields lit up.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub book: Book,
    pub title_spans: Vec<(usize, usize)>,
    pub author_spans: Vec<(usize, usize)>,
    pub path_spans: Vec<(usize, usize)>,
    pub score: i32,
}

/// One term's best field hit: the weighted score, which field won it, and
/// the spans that field lit up.
type BestHit = (i32, usize, Vec<(usize, usize)>);

/// Score one book against every term, keeping the best field per term and
/// weighting them as a reader means them: name first, author second, address
/// last. `None` when any term misses every field.
fn rank(book: &Book, query: &str) -> Option<Suggestion> {
    let title = book.title();
    let author = book.author();
    let path = book.path();
    let mut score = 0i32;
    let mut spans: [Vec<(usize, usize)>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for term in query.split_whitespace() {
        let candidates = [
            term_match(&title, term),
            author.as_deref().and_then(|a| term_match(a, term)),
            term_match(path, term),
        ];
        let weights = [3i32, 2, 1];
        let mut best: Option<BestHit> = None;
        for (field, candidate) in candidates.into_iter().enumerate() {
            let Some(m) = candidate else { continue };
            let weighted = weights[field] * m.score;
            let beats = match &best {
                Some((b, _, _)) => weighted > *b,
                None => true,
            };
            if beats {
                best = Some((weighted, field, m.spans));
            }
        }
        let (weighted, field, hit) = best?;
        score += weighted;
        spans[field].extend(hit);
    }
    for list in &mut spans {
        sort_merge(list);
    }
    Some(Suggestion {
        book: book.clone(),
        title_spans: std::mem::take(&mut spans[0]),
        author_spans: std::mem::take(&mut spans[1]),
        path_spans: std::mem::take(&mut spans[2]),
        score,
    })
}

/// Fold overlapping and adjacent spans, so painting a field's hits is one
/// walk with no double-lit characters.
fn sort_merge(spans: &mut Vec<(usize, usize)>) {
    spans.sort_unstable_by_key(|s| s.0);
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
    for span in spans.drain(..) {
        match merged.last_mut() {
            Some(last) if span.0 <= last.1 => last.1 = last.1.max(span.1),
            _ => merged.push(span),
        }
    }
    *spans = merged;
}

/// The books the bar suggests for a query, best first: score, then recency,
/// then title, so a tie between two editions lands on the one opened last.
/// Links are not suggested; the book a link points at is.
pub fn suggest(rows: &[Row], query: &str, limit: usize) -> Vec<Suggestion> {
    if !is_active(query) {
        return Vec::new();
    }
    let mut ranked: Vec<Suggestion> = book_rows(rows).filter_map(|b| rank(b, query)).collect();
    ranked.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.book.last_read_ms.cmp(&a.book.last_read_ms))
            .then_with(|| a.book.title().cmp(&b.book.title()))
    });
    ranked.truncate(limit);
    ranked
}

/// The rows a query keeps, in the given order — the shelf's own sort has
/// already run. A book is kept by [`matches`]; a link by its name, through
/// [`matches_terms`], because a search that hid a visible row would quietly
/// drop results.
pub fn filter(rows: &[Row], query: &str) -> Vec<Row> {
    if !is_active(query) {
        return rows.to_vec();
    }
    rows.iter()
        .filter(|row| match row {
            Row::Book(b) => matches(b, query),
            Row::Link { name, .. } => matches_terms(name, query),
        })
        .cloned()
        .collect()
}
