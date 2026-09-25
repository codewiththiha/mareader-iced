//! The ask vocabulary: what a question is, the facts it carries, and the
//! sentence a small note answers with.

use library_core::conflict::Arrival;
use library_core::folder::FolderMode;

/// Which question an ask is, and the facts only that question has: the
/// sheets share one queue and one slot, so the describe and the answer both
/// dispatch on this rather than re-deriving the shape from the arrival.
#[derive(Clone, PartialEq, Debug)]
pub enum AskKind {
    /// WHICH three answers the sheet offers is the arrival's fact rather
    /// than this one's, so this variant carries nothing.
    NameCollision,
    /// A per-file question out of a folder import merging into a shelf the
    /// level already held. The walk's screen raises it when that screen
    /// lands; its words and answers already ride the sheet.
    #[allow(dead_code)] // ported ahead: the walk mints it
    FolderMerge {
        /// A folder that reads in place lands a linked answer now, a
        /// copying one lands it after its copy.
        mode: FolderMode,
        /// The watched folder whose ledger records the placement when the
        /// answer lands, so a later rescan stays quiet.
        folder_id: String,
    },
    /// Two answers rather than three — the library's own stored copy on
    /// this level, or the folder's book lit where it stands — because a
    /// second row of one linked file is the one thing a read-at-place
    /// folder can never make.
    Covered {
        /// The folder whose tree holds the file. Always one: the ask exists
        /// because a specific tree covers the ground.
        folder_id: String,
    },
    /// The question is the library's rather than the level's: the reader
    /// already has this book, somewhere, and what is asked is whether they
    /// meant to add a second instance or to go to the one they have.
    AlreadyHave,
}

impl AskKind {
    /// The folder whose ledger an answered ask settles, when the ask is one
    /// a folder raised.
    pub fn folder_id(&self) -> Option<&str> {
        match self {
            AskKind::FolderMerge { folder_id, .. } | AskKind::Covered { folder_id } => {
                Some(folder_id)
            }
            AskKind::NameCollision | AskKind::AlreadyHave => None,
        }
    }

    /// Whether the ask belongs to a folder that reads in place: the answer
    /// that lands a file links it rather than copying it, and the sheet's
    /// twin rule withholds *as new* by it.
    pub fn reads_in_place(&self) -> bool {
        matches!(self, AskKind::FolderMerge { mode, .. } if mode.reads_in_place())
    }

    pub fn is_name_question(&self) -> bool {
        matches!(self, AskKind::NameCollision)
    }

    pub fn is_folder_merge(&self) -> bool {
        matches!(self, AskKind::FolderMerge { .. })
    }

    /// The two-answer sheets: one wording shape, one answer batch, whether
    /// the library noticed the folder's ground or its own held copy.
    pub fn is_two_answer(&self) -> bool {
        matches!(self, AskKind::Covered { .. } | AskKind::AlreadyHave)
    }
}

/// The question on screen: the arrival kept whole, because an answer places
/// it and a placement needs the row and the level it was going to, and the
/// name of the thing already there — read once, because the sheet prints it
/// in three places and a heading and two buttons must not each derive their
/// own.
#[derive(Clone, PartialEq, Debug)]
pub struct ConflictAsk {
    pub arrival: Arrival,
    pub existing_id: String,
    pub existing_name: String,
    pub kind: AskKind,
}

impl ConflictAsk {
    pub fn name_collision(arrival: Arrival, existing_id: String, existing_name: String) -> Self {
        Self { arrival, existing_id, existing_name, kind: AskKind::NameCollision }
    }

    /// Raised by the walk's own screen when that screen lands; the two
    /// tests below mint it directly today.
    #[allow(dead_code)] // ported ahead: the walk's screen raises it
    pub fn folder_merge(
        arrival: Arrival,
        existing_id: String,
        existing_name: String,
        mode: FolderMode,
        folder_id: String,
    ) -> Self {
        Self { arrival, existing_id, existing_name, kind: AskKind::FolderMerge { mode, folder_id } }
    }

    pub fn already_have(arrival: Arrival, existing_id: String, existing_name: String) -> Self {
        Self { arrival, existing_id, existing_name, kind: AskKind::AlreadyHave }
    }

    pub fn covered(
        arrival: Arrival,
        existing_id: String,
        existing_name: String,
        folder_id: String,
    ) -> Self {
        Self { arrival, existing_id, existing_name, kind: AskKind::Covered { folder_id } }
    }
}

/// What the note says happened: nothing the import looked for was new, or
/// the folder walked back inside its own tree. The fold's own sentence is
/// the family's business — its raising waits on the fold landing, and the
/// variant stands where the web's words already are.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoteKind {
    NothingNew,
    /// A re-picked folder that had left its family folds back onto the rung
    /// its directory names: raised by the walk's own fold, which has not
    /// landed in the native app yet.
    #[allow(dead_code)] // ported ahead: the fold raises it
    Returned,
}

impl NoteKind {
    /// The line under the shelf's name, in the web sheet's own words.
    pub fn sublabel(self) -> &'static str {
        match self {
            NoteKind::NothingNew => "Nothing new to import",
            NoteKind::Returned => "Back where its folder names",
        }
    }
}

/// The note's full sentence, off the web's own sheet: the closing is part
/// of the promise — the shelf lights up as the note comes down.
pub fn note_sentence(kind: NoteKind, name: &str) -> String {
    match kind {
        NoteKind::NothingNew => {
            "The import walked the folder again and found nothing new — every \
             book is already on the shelf. The shelf lights up when you close \
             this."
                .to_string()
        }
        NoteKind::Returned => format!(
            "“{name}” went back inside the folder it belongs to, onto the \
             shelf its directory names. Nothing was copied, and nothing on disk \
             moved. It lights up where it stands now when you close this."
        ),
    }
}
