//! The add menu's Moved shelf: the facts its row stands on, its two-choice face,
//! and the answers that close it.

use crate::app::Mareader;
use crate::app::message::{Message, MovedAsk};
use crate::chrome::icons::IconName;
use crate::library::menus;
use crate::platform::now_ms;
use iced::Task;
use library_core::folder::{self as folder_ops, Tombstone};
use library_core::ledger::{self, Recovered};
use library_core::shelf::{self, ALL_SHELF};
use library_core::{paths, text as lib_text};

/// The removed row's second line: when the reader took the book out, and —
/// remembered — how big the promise is.
fn removed_sublabel(entry: &Tombstone, stamp: u64) -> String {
    let age = lib_text::human_age(entry.removed_ms, stamp);
    match entry.fp.mtime_ms {
        0 => format!("removed {age}"),
        _ => format!("removed {age} · {}", lib_text::human_size(entry.fp.size)),
    }
}
impl Mareader {
    /// The confirm face's second answer: close the menu and take the
    /// reader to the shelf the book is on — the first membership in shelf
    /// order, or the library's own floor when it is on none.
    pub(in crate::app) fn go_and_look(&mut self, book_id: &str) -> Task<Message> {
        self.menu = None;
        self.menu_confirm = None;
        self.context = None;
        self.shelf = shelf::containing(&self.library.shelves, book_id)
            .first()
            .map(|shelf| shelf.id.clone())
            .unwrap_or_else(|| ALL_SHELF.to_string());
        Task::none()
    }

    /// One book, two memberships, nothing copied: the folder's ledger is
    /// untouched — the book stays placed where it was placed, which keeps
    /// the next rescan quiet about it. The level's name screen rides the
    /// filing's own gate, as it does for every second membership.
    pub(in crate::app) fn also_show(&mut self, book_id: &str, shelf_id: &str) -> Task<Message> {
        self.menu = None;
        self.menu_confirm = None;
        let one = [book_id.to_string()];
        if self.gated_file(&one, shelf_id) {
            return self.persist_library();
        }
        Task::none()
    }

    /// The add menu's folder-side facts: the in-folder picker's door, the
    /// Restore section's rows, and the confirm face when a Moved row was
    /// asked. The labels are computed here — the menu lays out, the app
    /// knows.
    pub(in crate::app) fn add_facts(&self) -> menus::AddFacts {
        let confirm = self.menu_confirm.as_ref().map(|ask| self.confirm_face(ask));
        let mut from_folder = None;
        let mut restore = Vec::new();
        if let Some(folder_id) = self.standing_folder_id()
            && let Some(folder) = folder_ops::find(&self.library.folders, &folder_id)
        {
            from_folder = Some(menus::MenuLine {
                icon: IconName::Drop,
                label: "Choose files from this folder".to_string(),
                sublabel: Some(paths::dir_label(&folder.root)),
                message: Some(Message::PickFilesInFolder(folder.root.clone())),
            });
            let index = ledger::index_by_fp(&self.library.books);
            let stamp = now_ms();
            for item in ledger::recoverables(folder, &index, &self.library.shelves) {
                restore.push(match &item {
                    Recovered::Deleted(entry) => {
                        let gone = self.restore_gone.contains(&entry.last_path);
                        menus::MenuLine {
                            icon: IconName::Undo,
                            label: entry.label(),
                            sublabel: Some(if gone {
                                "not there any more".to_string()
                            } else {
                                removed_sublabel(entry, stamp)
                            }),
                            message: (!gone)
                                .then(|| Message::RestoreDeleted(folder_id.clone(), entry.fp)),
                        }
                    }
                    Recovered::Moved { book_id, title, path, home_shelf } => menus::MenuLine {
                        icon: IconName::Next,
                        label: lib_text::display_or_stem(title.as_deref(), path),
                        sublabel: Some(match home_shelf {
                            Some(name) => format!("now on “{name}”"),
                            None => "in the library, on no shelf".to_string(),
                        }),
                        message: Some(Message::ConfirmMoved(MovedAsk {
                            book_id: book_id.clone(),
                            title: title.clone(),
                            path: path.clone(),
                            home_shelf: home_shelf.clone(),
                        })),
                    },
                });
            }
        }
        menus::AddFacts { from_folder, restore, confirm }
    }

    /// The two-choice face a Moved row swaps the panel into: show the book
    /// here as well — one book, two shelves, nothing copied — or close the
    /// menu and go look at where it went.
    pub(in crate::app) fn confirm_face(&self, ask: &MovedAsk) -> menus::ConfirmFace {
        let label = lib_text::display_or_stem(ask.title.as_deref(), &ask.path);
        let go_label = match &ask.home_shelf {
            Some(name) => format!("Show it in “{name}”"),
            None => "Show it in Home".to_string(),
        };
        menus::ConfirmFace {
            back: menus::MenuLine {
                icon: IconName::Prev,
                label: "Back".to_string(),
                sublabel: None,
                message: Some(Message::MenuBack),
            },
            question: format!("“{label}” is on another shelf."),
            also: menus::MenuLine {
                icon: IconName::Plus,
                label: "Also show it here".to_string(),
                sublabel: Some("One book, two shelves — nothing is copied".to_string()),
                // The offer is this level's: standing on the library's own
                // floor, there is no "here" to also show the book in.
                message: (self.shelf != ALL_SHELF)
                    .then(|| Message::AlsoShow(ask.book_id.clone(), self.shelf.clone())),
            },
            go: menus::MenuLine {
                icon: IconName::Next,
                label: go_label,
                sublabel: Some("Closes this menu and takes you to it".to_string()),
                message: Some(Message::GoAndLook(ask.book_id.clone())),
            },
        }
    }
}
