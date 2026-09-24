//! The document formats and the engines that render them.
//!
//! One module per format family, and the same division of labour in each: a
//! worker that owns the format's own machinery, a protocol the reading surface
//! speaks, and pure modules for the arithmetic that can be tested without a
//! document at all. The reflowable family (txt, Markdown) arrives in P4; the
//! split exists from the start so that phase adds a sibling here rather than a
//! branch in the reader.

pub mod pdf;
