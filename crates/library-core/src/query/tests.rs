//! The search's own cases: what a term matches, and what the bar offers.

use super::*;
use super::matching::term_match;
use crate::book::{Book, Origin, Row};

fn row(title: &str, author: Option<&str>, path: &str) -> Row {
    Row::Book(book(title, author, path))
}

fn at(rows: &[Row], i: usize) -> &Book {
    rows[i].book().expect("a book row")
}

fn at_mut(rows: &mut [Row], i: usize) -> &mut Book {
    rows[i].as_book_mut().expect("a book row")
}

fn book(title: &str, author: Option<&str>, path: &str) -> Book {
    Book {
        title: Some(title.to_string()),
        author: author.map(str::to_string),
        origin: Origin::Linked {
            src: path.to_string(),
        },
        ..crate::testkit::book(title)
    }
}

#[test]
fn a_blank_bar_filters_nothing() {
    assert!(!is_active(""));
    assert!(!is_active("   "));
    assert!(is_active("d"));
    let books = vec![row("Dune", None, "/books/dune.pdf")];
    assert_eq!(filter(&books, "").len(), 1);
    assert_eq!(filter(&books, "  ").len(), 1);
}

#[test]
fn a_title_is_found_whatever_its_case() {
    let b = book("Dune Messiah", Some("Frank Herbert"), "/books/dune.pdf");
    assert!(matches(&b, "dune"));
    assert!(matches(&b, "DUNE"));
    assert!(matches(&b, "messiah"));
    assert!(!matches(&b, "foundation"));
}

#[test]
fn every_term_has_to_be_found_but_the_order_does_not_matter() {
    let b = book("Dune", Some("Frank Herbert"), "/books/dune.pdf");
    assert!(matches(&b, "herbert dune"));
    assert!(matches(&b, "dune herbert"));
    assert!(matches(&b, "frank  dune"));
    assert!(!matches(&b, "dune asimov"));
}

#[test]
fn the_address_is_searchable_too() {
    let b = book("Untitled scan", None, "/Books/Scifi/1984-report.pdf");
    assert!(matches(&b, "scifi"));
    assert!(matches(&b, "1984"));
    assert!(matches(&b, "report"));
}

#[test]
fn a_titleless_book_is_searchable_by_its_stem() {
    let mut b = book("ignored", None, "/books/Foundation.pdf");
    b.title = None;
    assert!(matches(&b, "foundation"), "the stem is the title when there is none");
}

#[test]
fn a_name_is_searched_by_the_same_rule_as_a_book() {
    assert!(matches_terms("Science Fiction", "sci"), "a prefix is enough");
    assert!(matches_terms("Science Fiction", "fiction science"));
    assert!(matches_terms("Science Fiction", "SCIENCE"));
    assert!(!matches_terms("Science Fiction", "science crime"));
    assert!(matches_terms("Science Fiction", "   "));
}

#[test]
fn filtering_keeps_the_order_it_was_given() {
    let books = vec![
        row("Zebra", None, "/books/z.pdf"),
        row("Apple", None, "/books/ap.pdf"),
        row("Apricot", None, "/books/apc.pdf"),
    ];
    let kept: Vec<String> = filter(&books, "ap").iter().map(Row::display_name).collect();
    assert_eq!(kept, vec!["Apple", "Apricot"], "the shelf's own sort ran first");
    assert!(filter(&books, "q").is_empty());
}

#[test]
fn a_link_is_found_by_its_name_and_never_suggested() {
    // A link is a visible row, so a search that hid it would drop
    // results — and it is not a book, so the bar does not suggest it.
    let rows = vec![
        row("Dune", Some("Frank Herbert"), "/books/dune.pdf"),
        Row::link("l1".into(), "Dune".into(), "b1".into(), 5),
        Row::link("l2".into(), "Recipes".into(), "b2".into(), 5),
    ];
    let kept = filter(&rows, "dune");
    assert_eq!(kept.len(), 2, "the book and the pointer that wears its name");
    assert!(kept.iter().any(Row::is_link));
    assert_eq!(filter(&rows, "recipes").len(), 1);
    assert_eq!(suggest(&rows, "dune", SUGGEST_LIMIT).len(), 1);
    assert_eq!(filter(&rows, "").len(), 3);
    assert_eq!(at(&rows, 0).title(), "Dune");
}

#[test]
fn a_subsequence_that_holds_its_shape_is_a_match() {
    // The shapes a reader half-remembers: missing vowels, initials, a
    // run broken once.
    let m = term_match("Mathematical Proofs", "mthmtcl").expect("fuzzy match");
    assert!(m.score >= 2 * 7);
    assert!(m.spans.len() >= 2, "runs merge, gaps split: {:?}", m.spans);
    assert!(term_match("Dune", "dne").is_some());
    assert!(term_match("The Left Hand of Darkness", "tlhod").is_some());
    // A word boundary is worth four, so the same term scores higher where
    // it starts a word.
    let boundary = term_match("mathematical proofs", "math").unwrap();
    let midword = term_match("xxmathxx", "math").unwrap();
    assert!(boundary.score > midword.score);
}

#[test]
fn a_subsequence_too_scattered_is_not_a_match() {
    // The floor is two points a character; a sprinkle across a long field
    // earns that minus its gaps. One point lower and fuzzy would match
    // everything.
    assert!(term_match("A Brief Collection of Zebra Essays", "xyz").is_none());
    assert!(term_match("Dune", "nud").is_none(), "order still matters");
    assert!(term_match("Dune", "dunee").is_none(), "so does count");
}

#[test]
fn a_substring_always_matches_wherever_it_sits() {
    let mid = term_match("xxdunexx", "dune").expect("substring");
    assert_eq!(mid.spans, vec![(2, 6)]);
    let start = term_match("dunexx", "dune").expect("substring at start");
    assert!(start.score > mid.score, "a start is worth more than a middle");
}

#[test]
fn suggestions_rank_the_name_over_the_author_over_the_address() {
    let books = vec![
        row("Notes on Dune", None, "/x/a.pdf"),
        row("Collected Papers", Some("Dune White"), "/x/b.pdf"),
        row("Collected Papers", None, "/dune/raw.pdf"),
    ];
    let ranked = suggest(&books, "dune", 3);
    let titles: Vec<String> = ranked.iter().map(|s| s.book.title()).collect();
    assert_eq!(
        titles,
        vec![
            "Notes on Dune".to_string(),
            "Collected Papers".to_string(),
            "Collected Papers".to_string()
        ],
        "title beats author beats address"
    );
    assert!(ranked[1].title_spans.is_empty());
    assert!(!ranked[1].author_spans.is_empty());
    assert!(!ranked[2].path_spans.is_empty());
}

#[test]
fn suggestions_stop_at_the_limit_and_keep_the_reader_s_last() {
    let mut books: Vec<Row> = (0..12)
        .map(|i| row(&format!("Dune {i}"), None, &format!("/x/{i}.pdf")))
        .collect();
    at_mut(&mut books, 7).last_read_ms = 99;
    let ranked = suggest(&books, "dune", SUGGEST_LIMIT);
    assert_eq!(ranked.len(), SUGGEST_LIMIT);
    assert_eq!(ranked[0].book.title(), "Dune 7", "recency breaks a score tie");
}

#[test]
fn a_fuzzy_query_suggests_and_the_spans_light_the_hits() {
    let books = vec![row("Mathematical Proofs", None, "/x/mp.pdf")];
    let ranked = suggest(&books, "mthprf", SUGGEST_LIMIT);
    assert_eq!(ranked.len(), 1);
    assert!(!ranked[0].title_spans.is_empty());
}
