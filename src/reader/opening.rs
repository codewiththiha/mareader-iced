//! Opening and closing: what a book is opened with, the session that answers, and
//! the effects an open or a close hands back.

use std::path::PathBuf;

use crate::formats::pdf::Request;
use crate::ui::toast::Tone;
use super::document::DocStatus;
use super::{Effect, Reader};

/// The book the app asks the reader to open: where it lives, and what the
/// library already knows about it.
///
/// The resume point travels *in* rather than being looked up here, because
/// which row answers for an address is the library's business and the app has
/// the rows in hand when it dispatches the open — the web app read its resume
/// point before the engine was asked for anything, so a concurrent progress
/// write from the closing document could not clobber it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Open {
    pub path: PathBuf,
    /// The library row the reader named, when they named one.
    pub book_id: Option<String>,
    /// The row's own name for the book — the library's display name for it,
    /// which is what answers for the title when the document itself carries
    /// none worth showing.
    pub row_title: Option<String>,
    /// The page the library remembers.
    pub resume: u32,
}

impl Reader {
    /// Start reading a document.
    ///
    /// An engine that already said it has no library answers here and now: a
    /// request posted to a worker that has exited would leave the reader
    /// waiting on an answer nobody is left to send.
    pub(super) fn begin_open(&mut self, open: Open) -> Vec<Effect> {
        self.session += 1;
        self.awaiting = None;
        self.frame = None;
        self.document.begin(&open);
        match self.bound.clone() {
            Some(Err(message)) => {
                self.document.status = DocStatus::Error;
                self.document.error = Some(message.clone());
                vec![Effect::Toast(Tone::Error, message)]
            }
            _ => {
                self.engine.send(Request::Open {
                    stamp: self.session,
                    path: open.path,
                });
                Vec::new()
            }
        }
    }

    /// Let the document go and tell the app where the reader stopped, so the
    /// shelf can resume them there.
    pub fn close(&mut self) -> Vec<Effect> {
        // Taken before the reset, which is what forgets the name of the book.
        let effects = self.effect_of(Effect::Record);
        self.session += 1;
        self.awaiting = None;
        self.frame = None;
        self.document.reset();
        // Whatever was still moving belonged to the book that just closed;
        // a transition left open would keep asking for frames of a page
        // nobody is reading.
        self.zoom.cancel();
        // The page belonged to the book that just closed; the reader's own
        // layout — mode, fit, scale — is theirs and survives it.
        self.viewer.reset_position();
        self.engine.send(Request::Close);
        effects
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::pdf;
    use std::path::PathBuf;
    use crate::reader::kit::reader;
    use iced::Size;

    #[test]
    fn a_read_names_the_row_and_only_a_title_worth_persisting() {
        // The row is what keeps a book of the reader's own from handing its
        // position to its twin at the address; the title is the metadata half
        // only, because the address's stem is a display fallback and writing
        // it down would rename a stored book after the store's own layout.
        let mut reader = reader();
        reader.viewer.container = Size::new(800.0, 1000.0);
        reader.session += 1;
        reader.document.begin(&Open {
            path: PathBuf::from("/store/items/b1/source.pdf"),
            book_id: Some("b1".to_string()),
            row_title: Some("Dune".to_string()),
            resume: 1,
        });
        reader.seed(pdf::Opened {
            path: PathBuf::from("/store/items/b1/source.pdf"),
            num_pages: 2,
            page_sizes: vec![
                pdf::PageBox {
                    width: 612.0,
                    height: 792.0,
                };
                2
            ],
            title: Some("Untitled".to_string()),
            author: Some("Frank Herbert".to_string()),
        });
        let read = reader.read().expect("a book is open");
        assert_eq!(read.book_id.as_deref(), Some("b1"));
        assert_eq!(read.title, None, "a placeholder title is not worth writing down");
        assert_eq!(read.author.as_deref(), Some("Frank Herbert"));
    }
}
