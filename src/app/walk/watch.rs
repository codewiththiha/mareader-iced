//! The watch seats a folder's own rows answer for: which rung of which tree,
//! what that rung watches now, and the labels the toggle speaks.
use library_core::folder::FolderOpts;

#[derive(Debug, Clone)]
pub(in crate::app) struct GroundWatch {
    pub(in crate::app) tree_id: String,
    pub(in crate::app) rung: String,
    pub(in crate::app) on: bool,
    pub(in crate::app) opts: FolderOpts,
}

/// The watch seat a folder shelf answers for: which rung of which tree,
/// what that rung watches now, and the labels the toggle row speaks. The
/// row the folder context menu has and no other menu does, because no
/// other shelf has a rung to answer for.
pub(in crate::app) struct ShelfWatch {
    pub(in crate::app) folder_id: String,
    pub(in crate::app) rung: String,
    pub(in crate::app) on: bool,
    pub(in crate::app) label: String,
    pub(in crate::app) rung_label: Option<String>,
}

