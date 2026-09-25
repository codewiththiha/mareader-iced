//! The import sheet: the walk's orders written before it walks.

use super::{Mareader, Sheet};
use crate::chrome::icons::{IconName, icon};
use crate::library::conflicts::ShelfConflictAsk;
use crate::theme::{Tokens, wash};
use crate::ui::{popover, sheet};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Task};
use iced::widget::{Column, Row, Space, container, row, text};
use library_core::paths;
use library_core::conflict;
use library_core::folder::{FolderMode, FolderOpts, MIN_SIZE_CEIL, MIN_SIZE_FLOOR};
use library_core::scan::selectable_formats;
use library_core::shelf;
use std::path::{Path, PathBuf};
use crate::app::message::Message;
use crate::app::walk::{Asked, Continuation, GroundWatch, RootPlan};

fn import_sheet(
    tokens: Tokens,
    root: &Path,
    opts: &FolderOpts,
    ground: Option<&GroundWatch>,
) -> Element<'static, Message> {
    let section = |label: &'static str| -> Element<'static, Message> {
        container(text(label).size(11).color(tokens.muted))
            .padding(Padding { top: 2.0, right: 0.0, bottom: 6.0, left: 0.0 })
            .into()
    };

    // The folder's address, in a bordered pill.
    let path = root.to_string_lossy().into_owned();
    let folder_pill = container(
        row![
            icon(IconName::Open, 15, tokens.muted),
            container(text(path).size(13).color(tokens.ink)).width(Length::Fill),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 })
    .style(move |_| container::Style {
        border: Border { color: tokens.line, width: 1.0, radius: 10.0.into() },
        ..container::Style::default()
    });

    let include_row = row![
        popover::toggle(
            tokens,
            None,
            "Include selected",
            opts.include_selected,
            Some(Message::SheetImportInclude(true)),
        ),
        popover::toggle(
            tokens,
            None,
            "Exclude selected",
            !opts.include_selected,
            Some(Message::SheetImportInclude(false)),
        ),
    ]
    .spacing(6);

    // The format chips, two to a line.
    let mut format_rows: Vec<Element<'static, Message>> = Vec::new();
    for chunk in selectable_formats().chunks(2) {
        let mut line = Row::new().spacing(6);
        for format in chunk {
            line = line.push(popover::toggle(
                tokens,
                None,
                format.label(),
                opts.formats.contains(format),
                Some(Message::SheetImportFormat(*format)),
            ));
        }
        if chunk.len() == 1 {
            line = line.push(Space::new().width(Length::Fill));
        }
        format_rows.push(line.into());
    }

    // The size threshold, with its adjusters.
    let at_floor = opts.min_size == MIN_SIZE_FLOOR;
    let at_ceil = opts.min_size >= MIN_SIZE_CEIL;
    let size_row = row![
        container(text(opts.min_size_label()).size(12).color(tokens.ink))
            .padding(Padding { top: 3.0, right: 9.0, bottom: 3.0, left: 9.0 })
            .style(move |_| container::Style {
                background: Some(Background::Color(wash(tokens.line, 0.50))),
                border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 6.0.into() },
                ..container::Style::default()
            }),
        Space::new().width(Length::Fill),
        popover::stepper_button(tokens, IconName::Minus,
            (!at_floor).then_some(Message::SheetImportSize(-1))),
        popover::stepper_button(tokens, IconName::Plus,
            (!at_ceil).then_some(Message::SheetImportSize(1))),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    // How the books are held: one control, three answers, because there are
    // three modes — the fourth pair (a watching copy) is not one the sheet
    // can show, and every click writes both switches from the mode picked.
    let mode = opts.mode();
    let books_rows = Column::new()
        .push(popover::toggle(
            tokens,
            None,
            FolderMode::Copy.label(),
            mode == FolderMode::Copy,
            Some(Message::SheetImportMode(FolderMode::Copy)),
        ))
        .push(popover::toggle(
            tokens,
            None,
            FolderMode::LinkInPlace.label(),
            mode == FolderMode::LinkInPlace,
            Some(Message::SheetImportMode(FolderMode::LinkInPlace)),
        ))
        .push(popover::toggle(
            tokens,
            None,
            FolderMode::LinkInPlaceWatched.label(),
            mode == FolderMode::LinkInPlaceWatched,
            Some(Message::SheetImportMode(FolderMode::LinkInPlaceWatched)),
        ))
        .spacing(6);
    let books_note = text(mode_note(mode, ground.is_some())).size(12).color(tokens.muted);

    // The structure answer, and the promise it makes about the tree.
    let structure_rows = Column::new()
        .push(popover::toggle(
            tokens,
            None,
            "A shelf for each folder",
            opts.groups,
            Some(Message::SheetImportGroups(true)),
        ))
        .push(popover::toggle(
            tokens,
            None,
            "One shelf for everything",
            !opts.groups,
            Some(Message::SheetImportGroups(false)),
        ))
        .spacing(6);
    // A pick inside a governed tree answers for its rung alone: the note
    // says so, because "the whole import" would over-promise.
    let in_tree = ground.is_some_and(|watch| !watch.rung.is_empty());
    let structure_note = text(if in_tree {
        "This answer stands for this folder and the ones under it — the rest of the tree keeps its own."
    } else {
        "A shelf for each folder gives every subfolder its own shelf; one shelf keeps the whole import together."
    })
    .size(12)
    .color(tokens.muted);

    Column::new()
        .push(section("FOLDER"))
        .push(folder_pill)
        .push(section("FORMATS"))
        .push(include_row)
        .push(Column::with_children(format_rows).spacing(6))
        .push(section("FILE SIZE LARGER THAN"))
        .push(size_row)
        .push(section("HOW THE BOOKS ARE HELD"))
        .push(books_rows)
        .push(books_note)
        .push(section("FOLDER STRUCTURE"))
        .push(structure_rows)
        .push(structure_note)
        .spacing(10)
        .width(Length::Fill)
        .into()
}

/// The mode's own sentence under the sheet's control — one wording per
/// mode, so a mode described two ways never reads as two modes. A watched
/// pick inside a governed tree gets the rung's wording: the promise is the
/// subfolder's, and the rest of the tree keeps its own answer.
fn mode_note(mode: FolderMode, in_ground: bool) -> &'static str {
    match mode {
        FolderMode::Copy => {
            "Books are copied into the app's own files, so they keep working even if the folder moves or is deleted."
        }
        FolderMode::LinkInPlace => {
            "Books stay where they are — the library just remembers where they live. Books added to the folder later are not picked up."
        }
        FolderMode::LinkInPlaceWatched if in_ground => {
            "Books stay where they are, and this subfolder is checked for new ones. The rest of the tree keeps its own answer."
        }
        FolderMode::LinkInPlaceWatched => {
            "Books stay where they are, and the folder is checked for new ones when the app opens or you come back to it."
        }
    }
}

impl Mareader {
    pub(super) fn import_panel(&self, root: &Path, ground: &Option<GroundWatch>) -> Element<'_, Message> {
        sheet::panel_sized(
                self.tokens,
                sheet::IMPORT_W,
                "Import books",
                import_sheet(self.tokens, root, &self.import_opts, ground.as_ref()),
                vec![
                    sheet::cancel_button(self.tokens, "Cancel", Message::SheetCancel),
                    sheet::confirm_button(self.tokens, "Import", Message::SheetSave, false),
                ],
            )
    }

    pub(super) fn save_import(&mut self, root: PathBuf, ground: Option<GroundWatch>) -> Task<Message> {
        let opts = self.import_opts.clone();
        let root_str = root.to_string_lossy().into_owned();
        // The sheet's watch answer belongs to the picked ground's
        // rung — a subfolder's import must not write the tree's
        // root — and only a read-at-place import watches at all.
        let mut persisted = Task::none();
        if opts.mode().reads_in_place()
            && let Some(mut watch) = ground
        {
            watch.on = opts.watch;
            persisted = self.write_rung_tracking(&watch);
        }
        // A covered ground walks the covering tree: a rung cannot
        // mint a second instance of itself, and removed books come
        // back wherever in the tree they stood. The walk's own
        // shelf map seats the pick on the shelf it named, and the
        // continuation keeps the rung's own light for the landing.
        if opts.mode().reads_in_place()
            && let Some(covered) = self.covered_shelf(&root_str)
        {
            let walk = self.begin_folder_walk(
                PathBuf::from(covered.tree_root),
                opts,
                Asked::Explicitly,
                RootPlan {
                    continuation: Some(Continuation {
                        shelf_id: covered.shelf_id,
                        name: covered.shelf_name,
                    }),
                    ..RootPlan::default()
                },
            );
            return Task::batch([persisted, walk]);
        }
        // A folder whose name the root level already holds is a
        // question before it is an import — two shelves of one name
        // are two doors a reader cannot tell apart. A shelf the
        // arriving folder's own row named is a continuation: the
        // same question, the sheet's own words.
        let incoming = paths::dir_label(&root_str);
        if let Some(existing_id) =
            conflict::collide_shelf(&self.library.shelves, None, &incoming)
        {
            let own = self
                .library
                .folders
                .iter()
                .find(|f| f.root == root_str)
                .and_then(|f| f.shelf_map.get("").cloned())
                .is_some_and(|root_rung| root_rung == existing_id);
            let existing_name = shelf::find(&self.library.shelves, &existing_id)
                .map(|each| each.name.clone())
                .unwrap_or_else(|| incoming.clone());
            self.sheet = Some(Sheet::ShelfConflict {
                ask: ShelfConflictAsk {
                    incoming_name: incoming,
                    existing_id,
                    existing_name,
                    root: root_str,
                    opts,
                    own,
                },
            });
            return persisted;
        }
        let walk = self.begin_folder_walk(root, opts, Asked::Explicitly, RootPlan::default());
        Task::batch([persisted, walk])
    }
}
