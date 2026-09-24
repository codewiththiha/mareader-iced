//! The questions written rather than drawn: every sheet's heading,
//! subtitle and answers, off one snapshot of the library.

use library_core::book::{find_row, Row};
use library_core::conflict::{next_name, Placement};
use library_core::folder::WatchedFolder;
use library_core::paths::dir_label;
use library_core::shelf::{self, Shelf, ALL_SHELF};
use super::{offers_for, ConflictAsk};

/// One answer row on the sheet: its name, the line that promises what
/// choosing it does, and the answer it carries.
#[derive(Clone, PartialEq, Debug)]
pub struct ChoiceSpec {
    pub label: &'static str,
    pub note: String,
    pub placement: Placement,
}


/// The question described rather than drawn: every sentence built off one
/// snapshot of the library, because a row that counted one way and answered
/// another is a receipt for something else.
pub struct SheetSpec {
    pub heading: String,
    pub subtitle: String,
    pub question: String,
    /// Whether the sheet shows the apply-to-all switch: the two-answer and
    /// the folder-merge sheets batch, the name question does not — its
    /// answers mint names and dissolve rows, and batching that is a promise
    /// no one question can keep for another.
    pub apply_all: bool,
    /// How many MORE questions of this sheet's own kind wait behind the one
    /// on screen — the switch's count and the subtitle's tail read the same
    /// number.
    pub waiting: usize,
    pub choices: Vec<ChoiceSpec>,
}


/// One spelling for every question that names the level: two sheets wording
/// the same shelf differently would read as two places. An empty name means
/// the shelf went while the sheet was up.
fn where_line(shelves: &[Shelf], shelf_id: &str) -> String {
    if shelf_id == ALL_SHELF {
        "in your library".to_string()
    } else {
        match shelf::find(shelves, shelf_id).map(|s| s.name.as_str()).unwrap_or("") {
            name if !name.is_empty() => format!("on “{name}”"),
            _ => "on this shelf".to_string(),
        }
    }
}


fn more_waiting(subtitle: String, waiting: usize) -> String {
    if waiting > 0 {
        format!("{subtitle} · {waiting} more waiting")
    } else {
        subtitle
    }
}


/// The one entry the sheet reads: the kind decides which question's words
/// describe the ask, and every describer counts only the questions of its
/// own kind in the queue — a count that included another shape would
/// promise an apply-all these answers cannot consume.
pub fn describe(
    rows: &[Row],
    shelves: &[Shelf],
    folders: &[WatchedFolder],
    ask: &ConflictAsk,
    waiting: &[ConflictAsk],
) -> SheetSpec {
    if ask.kind.is_folder_merge() {
        describe_folder_merge(rows, shelves, ask, waiting)
    } else if ask.kind.is_two_answer() {
        describe_covered(shelves, folders, ask, waiting)
    } else {
        describe_name(rows, shelves, folders, ask, waiting)
    }
}


/// The name question's own words. `waiting` is the queue behind this one,
/// counted to the questions this sheet answers.
///
/// The replace note's highlight count is the one clause the native app
/// cannot keep yet: the reader has no marks store until the engines land, so
/// the count is always zero and the note wears its zero-mark spelling.
fn describe_name(
    rows: &[Row],
    shelves: &[Shelf],
    folders: &[WatchedFolder],
    ask: &ConflictAsk,
    waiting: &[ConflictAsk],
) -> SheetSpec {
    let waiting = waiting.iter().filter(|each| each.kind.is_name_question()).count();
    let where_line = where_line(shelves, &ask.arrival.shelf_id);
    let import = ask.arrival.is_import();
    // The apply's own list rather than a re-derived condition, so a row the
    // sheet renders is a row the answer will take.
    let offers = offers_for(rows, folders, ask);
    let existing_name = ask.existing_name.clone();
    let new_name = next_name(rows, shelves, &ask.arrival.shelf_id, &ask.arrival.name);

    let question = if import {
        format!(
            "“{}” is already {where_line}. Add a second book of its own, put a link here \
             instead, or go to the one you have.",
            ask.arrival.name
        )
    } else if offers.contains(&Placement::LinkOnly) {
        format!(
            "A book called “{existing_name}” is already {where_line}, and it is one of the \
             library's own copies. Keep one book, reach the copy from here, or keep both under \
             a new name."
        )
    } else {
        format!(
            "A book called “{existing_name}” is already {where_line}. Keep one book, keep this \
             one instead, or keep both under a new name."
        )
    };
    let go_to_note = format!("Add nothing — go to “{existing_name}” where it already is");
    let new_note = format!("Keeps both, under the next free name — “{new_name}”");
    const LINK_BOOK_NOTE: &str = "A pointer row, not a copy: tapping it goes to the book where it lives";
    let merge_note = format!(
        "One book — “{existing_name}” stays, and takes this one's shelves, its highlights, \
         and the further place in it"
    );
    let replace_note = format!(
        "“{existing_name}” leaves the library — this one takes its place on every shelf it \
         was on"
    );
    let move_new_note = format!("Keeps both — this one becomes “{new_name}”");
    let link_note = format!(
        "The book you dragged becomes a pointer here — “{existing_name}” stays, the file on \
         disk stays, and nothing is destroyed"
    );

    let choices = offers
        .iter()
        .map(|choice| match choice {
            Placement::Open => ChoiceSpec {
                label: "Already imported",
                note: go_to_note.clone(),
                placement: Placement::Open,
            },
            Placement::KeepBoth if import => ChoiceSpec {
                label: "Add as new",
                note: new_note.clone(),
                placement: Placement::KeepBoth,
            },
            Placement::KeepBoth => ChoiceSpec {
                label: "As new",
                note: move_new_note.clone(),
                placement: Placement::KeepBoth,
            },
            Placement::LinkOnly if import => ChoiceSpec {
                label: "Make link",
                note: LINK_BOOK_NOTE.to_string(),
                placement: Placement::LinkOnly,
            },
            Placement::LinkOnly => ChoiceSpec {
                label: "Make link",
                note: link_note.clone(),
                placement: Placement::LinkOnly,
            },
            Placement::Merge => ChoiceSpec {
                label: "Merge",
                note: merge_note.clone(),
                placement: Placement::Merge,
            },
            Placement::Replace => ChoiceSpec {
                label: "Replace",
                note: replace_note.clone(),
                placement: Placement::Replace,
            },
        })
        .collect();

    SheetSpec {
        heading: ask.arrival.name.clone(),
        subtitle: more_waiting(format!("Already {where_line}"), waiting),
        question,
        apply_all: false,
        waiting,
        choices,
    }
}


/// The two-answer question, and the two facts that raise it: a loose import
/// of a file inside a folder the library reads in place whose book is
/// alive, or of a file whose content the library already holds. Two answers
/// rather than three: a pointer at a row on this level is not an option a
/// covered file has.
fn describe_covered(
    shelves: &[Shelf],
    folders: &[WatchedFolder],
    ask: &ConflictAsk,
    waiting: &[ConflictAsk],
) -> SheetSpec {
    let waiting = waiting.iter().filter(|each| each.kind.is_two_answer()).count();
    let incoming = ask.arrival.name.clone();
    // Which fact the library noticed decides the sentence: same two
    // answers, different reason.
    let folder_name = ask
        .kind
        .folder_id()
        .and_then(|folder_id| folders.iter().find(|each| each.id == folder_id))
        .map(|folder| dir_label(&folder.root));
    let book_name = ask.existing_name.clone();
    let subtitle = more_waiting(
        match &folder_name {
            Some(name) => format!("Inside “{name}”"),
            None => "Already in your library".to_string(),
        },
        waiting,
    );
    let where_line = where_line(shelves, &ask.arrival.shelf_id);
    let question = match &folder_name {
        Some(name) => format!(
            "“{incoming}” is inside “{name}”, which the library reads in place — one book \
             per file, never a second link. Import your own copy {where_line}, or go to the \
             book the folder holds."
        ),
        None => format!(
            "The library already holds this book as “{book_name}”. Import your own copy \
             {where_line}, or go to the one you have."
        ),
    };
    let import_note = format!(
        "The library's own copy — its own book {where_line}, its own highlights, its own \
         place in it"
    );
    let show_note = match &folder_name {
        Some(name) => format!("Add nothing — go to “{book_name}” inside “{name}” and light it up"),
        None => format!("Add nothing — go to “{book_name}” and light it up"),
    };
    SheetSpec {
        heading: incoming,
        subtitle,
        question,
        apply_all: true,
        waiting,
        choices: vec![
            ChoiceSpec {
                label: "Import a copy here",
                note: import_note,
                placement: Placement::KeepBoth,
            },
            ChoiceSpec {
                label: "Show the imported one",
                note: show_note,
                placement: Placement::Open,
            },
        ],
    }
}


/// The compact per-file question a folder merge asks: two names, three
/// answers, and the switch that answers every waiting question at once.
fn describe_folder_merge(
    rows: &[Row],
    shelves: &[Shelf],
    ask: &ConflictAsk,
    waiting: &[ConflictAsk],
) -> SheetSpec {
    let waiting = waiting.iter().filter(|each| each.kind.is_folder_merge()).count();
    let incoming = ask.arrival.name.clone();
    let existing = ask.existing_name.clone();
    let subtitle = more_waiting(format!("Into “{existing}”"), waiting);
    // *As new* of the very file the row reads would be a second row of one
    // linked file, which the library does not make. A different file wearing
    // the same name keeps all three, and so does a stored folder.
    let twin = ask.kind.reads_in_place()
        && ask.arrival.file.as_ref().is_some_and(|file| {
            find_row(rows, &ask.existing_id)
                .and_then(|row| row.book())
                .is_some_and(|book| book.path() == file.path)
        });
    let question = if twin {
        format!(
            "“{incoming}” is arriving, and “{existing}” on this shelf reads this very file. \
             Keep the one that is here, or seat this file in its place."
        )
    } else {
        format!(
            "“{incoming}” is arriving, and “{existing}” is already on this shelf. Keep the \
             one that is here, seat this file in its place, or keep both under a name of its \
             own."
        )
    };
    let merge_note = format!("One book — “{existing}” stays, and takes this file's measurement");
    const REPLACE_NOTE: &str = "The row on the shelf leaves the library; this file takes its slot";
    let new_name = next_name(rows, shelves, &ask.arrival.shelf_id, &incoming);
    let new_note = format!("Keep both — this file becomes “{new_name}”");
    let mut choices = vec![
        ChoiceSpec {
            label: "Merge",
            note: merge_note,
            placement: Placement::Merge,
        },
        ChoiceSpec {
            label: "Replace",
            note: REPLACE_NOTE.to_string(),
            placement: Placement::Replace,
        },
    ];
    if !twin {
        choices.push(ChoiceSpec {
            label: "As new",
            note: new_note,
            placement: Placement::KeepBoth,
        });
    }
    SheetSpec {
        heading: incoming,
        subtitle,
        question,
        apply_all: true,
        waiting,
        choices,
    }
}

// ── The folder's own question ───────────────────────────────────────────
