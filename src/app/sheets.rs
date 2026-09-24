//! The sheets: every question the app raises as a modal, the answers it
//! writes back, and the import sheet's own controls.
use std::path::{Path, PathBuf};

use iced::widget::{button, container, operation, row, text, text_input, Column, Id, Row, Space};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Shadow, Task};
use library_core::book::{self};
use library_core::conflict::{self, Placement};
use library_core::folder::{FolderMode, FolderOpts, MIN_SIZE_CEIL, MIN_SIZE_FLOOR};
use library_core::scan::selectable_formats;
use library_core::shelf::{self};
use library_core::{paths, text as lib_text};

use crate::chrome::icons::{icon, IconName};
use crate::library::conflicts::{self, ConflictAsk, ShelfConflictAsk};
use crate::library::departure::CopyAsk;
use crate::theme::{wash, Tokens};
use crate::ui::sheet;
use super::Mareader;
use super::message::Message;
use super::view::{chip, stepper};
use super::walk::{Continuation, GroundWatch, RootPlan};

/// The sheet's rename field's identity — focus lands on it the moment the
/// sheet appears.
const SHEET_INPUT: Id = Id::new("sheet-input");

/// What the two sheets rename, so one sheet shape serves both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RenameKind {
    Row,
    Shelf,
}

/// The question on screen, if one: a modal the shelf waits on.
#[derive(Debug, Clone)]
pub(super) enum Sheet {
    /// A name being written: rename a row or a shelf.
    Rename { kind: RenameKind, id: String, draft: String },
    /// A row about to leave the library.
    Remove { id: String, name: String },
    /// The chosen set about to leave the library: its books, and the
    /// folder shelves themselves.
    RemoveMany { books: Vec<String>, shelves: Vec<String> },
    /// A folder about to be imported: the options sheet decides what the
    /// walk admits, and how the books are held. `ground` is the tree the
    /// pick belongs to, when one governs it — the sheet seeds its answers
    /// from the tree's own, and its Import writes the watch answer back
    /// onto the rung the pick names.
    Import { root: PathBuf, ground: Option<GroundWatch> },
    /// A move about to store copies: the departure's question, carrying the
    /// gesture it interrupted.
    Copy { ask: CopyAsk },
    /// A level that already holds the arriving name: the question, and the
    /// arrival the answer places.
    Conflict { ask: ConflictAsk },
    /// A folder whose name the root level already holds: asked before the
    /// walk, because the answer decides what the walk is for.
    ShelfConflict { ask: ShelfConflictAsk },
    /// Not a question — an answer: a folder the library already reads in
    /// place is named, and closing the note is every way out's promise: the
    /// shelf lights up.
    AlreadyImported { note: LitNote },
}

/// Which question a folder run answers; a boolean at the signature could
/// not say. An ask is the reader's own import; a walk is the app keeping a
/// watched folder's promise to itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Asked {
    Explicitly,
    OnFocus,
}

/// The covered walk's own seat: the rung shelf the ground named, and the
/// tree that walks it.
pub(super) struct Covered {
    pub(super) tree_root: String,
    pub(super) shelf_id: String,
    pub(super) shelf_name: String,
}

/// The already-imported answer, kept with its close: where the light
/// stands, which shelf the sheet names, and what the import found.
/// The already-imported answer kept with its close: it is the modal's own
/// modal, because the sheet's panel and the handler both read it whole.
#[derive(Clone, Debug)]
pub(super) struct LitNote {
    pub(super) shelf_id: String,
    pub(super) name: String,
    pub(super) kind: conflicts::NoteKind,
}

/// The Moved row the add menu is confirming: a book still inside the
/// folder on disk but filed on shelves elsewhere. The confirm face offers
/// two answers — show it here as well, or go look at where it went.
#[derive(Debug, Clone)]
pub struct MovedAsk {
    pub(super) book_id: String,
    pub(super) title: Option<String>,
    pub(super) path: String,
    pub(super) home_shelf: Option<String>,
}

/// The import sheet's body: the folder, the formats, the size threshold,
/// how the books are held and the structure answer — the walk's orders,
/// written before it walks. `ground` is the tree the pick belongs to, when
/// one governs it: the notes speak the rung's own promise then, not the
/// whole import's.
#[allow(clippy::too_many_lines)]
/// The "apply to all" row: one switch that gives every waiting question of
/// the same kind the answer being clicked.
fn apply_all_row(tokens: Tokens, waiting: usize, on: bool) -> Element<'static, Message> {
    let knob = button(
        text(if on { "On" } else { "Off" })
            .size(12)
            .color(if on { tokens.ink } else { tokens.muted }),
    )
    .padding(Padding { top: 5.0, right: 14.0, bottom: 5.0, left: 14.0 })
    .style(move |_, status| {
        let background = if on {
            tokens.accent_soft
        } else {
            match status {
                button::Status::Hovered | button::Status::Pressed => wash(tokens.line, 0.45),
                _ => Color::TRANSPARENT,
            }
        };
        button::Style {
            background: Some(Background::Color(background)),
            border: Border {
                color: if on { tokens.accent } else { tokens.line },
                width: 1.0,
                radius: 999.0.into(),
            },
            text_color: if on { tokens.ink } else { tokens.muted },
            shadow: Shadow::default(),
            snap: false,
        }
    })
    .on_press(Message::ToggleApplyAll);
    container(
        row![
            container(
                text(format!("Apply to all {}", waiting + 1)).size(12).color(tokens.muted)
            )
            .width(Length::Fill),
            knob,
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 })
    .style(move |_| container::Style {
        border: Border { color: tokens.line, width: 1.0, radius: 10.0.into() },
        ..container::Style::default()
    })
    .into()
}

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
        chip(tokens, "Include selected", opts.include_selected, Some(Message::SheetImportInclude(true))),
        chip(tokens, "Exclude selected", !opts.include_selected, Some(Message::SheetImportInclude(false))),
    ]
    .spacing(6);

    // The format chips, two to a line.
    let mut format_rows: Vec<Element<'static, Message>> = Vec::new();
    for chunk in selectable_formats().chunks(2) {
        let mut line = Row::new().spacing(6);
        for format in chunk {
            line = line.push(chip(
                tokens,
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
        stepper(tokens, IconName::Minus, !at_floor, Message::SheetImportSize(-1)),
        stepper(tokens, IconName::Plus, !at_ceil, Message::SheetImportSize(1)),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    // How the books are held: one control, three answers, because there are
    // three modes — the fourth pair (a watching copy) is not one the sheet
    // can show, and every click writes both switches from the mode picked.
    let mode = opts.mode();
    let books_rows = Column::new()
        .push(chip(
            tokens,
            FolderMode::Copy.label(),
            mode == FolderMode::Copy,
            Some(Message::SheetImportMode(FolderMode::Copy)),
        ))
        .push(chip(
            tokens,
            FolderMode::LinkInPlace.label(),
            mode == FolderMode::LinkInPlace,
            Some(Message::SheetImportMode(FolderMode::LinkInPlace)),
        ))
        .push(chip(
            tokens,
            FolderMode::LinkInPlaceWatched.label(),
            mode == FolderMode::LinkInPlaceWatched,
            Some(Message::SheetImportMode(FolderMode::LinkInPlaceWatched)),
        ))
        .spacing(6);
    let books_note = text(mode_note(mode, ground.is_some())).size(12).color(tokens.muted);

    // The structure answer, and the promise it makes about the tree.
    let structure_rows = Column::new()
        .push(chip(
            tokens,
            "A shelf for each folder",
            opts.groups,
            Some(Message::SheetImportGroups(true)),
        ))
        .push(chip(
            tokens,
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
    /// Ask the rename sheet for a name, prefilled with the one on show.
    pub(super) fn ask_rename(&mut self, kind: RenameKind, id: String) -> Task<Message> {
        self.context = None;
        let draft = match kind {
            RenameKind::Row => book::find_row(&self.library.books, &id)
                .map(|row| match row {
                    book::Row::Book(book) => book.title(),
                    book::Row::Link { name, .. } => name.clone(),
                })
                .unwrap_or_default(),
            RenameKind::Shelf => shelf::find(&self.library.shelves, &id)
                .map(|shelf| shelf.name.clone())
                .unwrap_or_default(),
        };
        self.sheet = Some(Sheet::Rename { kind, id, draft });
        // The field appears in the next view; focus lands with it.
        operation::focus(SHEET_INPUT)
    }

    /// The sheet's affirmative answer, carried out.
    pub(super) fn save_sheet(&mut self) -> Task<Message> {
        let Some(sheet) = self.sheet.take() else {
            return Task::none();
        };
        match sheet {
            Sheet::Rename { kind, id, draft } => {
                let name = draft.trim().to_string();
                if name.is_empty() {
                    return Task::none();
                }
                match kind {
                    RenameKind::Row => {
                        if let Some(row) = book::find_row_mut(&mut self.library.books, &id) {
                            match row {
                                book::Row::Book(book) => {
                                    book.title = Some(name);
                                    book.title_locked = true;
                                }
                                book::Row::Link { name: own, .. } => *own = name,
                            }
                        }
                    }
                    RenameKind::Shelf => {
                        if let Some(shelf) = shelf::find_mut(&mut self.library.shelves, &id) {
                            shelf.name = name;
                        }
                    }
                }
                self.persist_library()
            }
            Sheet::Remove { id, .. } => self.remove_row(&id),
            Sheet::RemoveMany { books, shelves } => self.remove_entries(books, shelves),
            // The copy sheet's buttons carry their own answers; its
            // affirmative one — the sheet's "Save", an Enter on the panel —
            // is the ask's primary: buy the copies and finish the gesture.
            Sheet::Copy { ask } => self.copy_and_finish(ask),
            // The name question has no default yes: its answers are the
            // placements, and its one close skips the queue with it.
            Sheet::Conflict { .. } => {
                self.conflict_waiting.clear();
                Task::none()
            }
            Sheet::ShelfConflict { .. } => Task::none(),
            Sheet::AlreadyImported { note } => self.reveal_shelf(&note.shelf_id),
            Sheet::Import { root, ground } => {
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
    }

    /// A sheet already up takes new asks onto its queue rather than being
    /// replaced: two drops in flight owe two answers.
    pub(super) fn raise_conflict(&mut self, asks: Vec<ConflictAsk>) {
        if asks.is_empty() {
            return;
        }
        if self.sheet.is_some() {
            self.conflict_waiting.extend(asks);
            return;
        }
        let mut asks = asks;
        let first = asks.remove(0);
        self.conflict_waiting.extend(asks);
        self.sheet = Some(Sheet::Conflict { ask: first });
        self.apply_all = false;
    }

    /// The next question of the queue, when the slot is free: an answered
    /// sheet stays up while questions wait, an answer that raised a sheet of
    /// its OWN — a replace's copy question — keeps it, and a queue nothing
    /// drained shows the moment a slot opens.
    pub(super) fn advance_conflict(&mut self) {
        if self.sheet.is_some() || self.conflict_waiting.is_empty() {
            return;
        }
        let ask = self.conflict_waiting.remove(0);
        self.sheet = Some(Sheet::Conflict { ask });
        self.apply_all = false;
    }

    /// What dropping this question means: the conflict sheet's close — its
    /// Cancel, its scrim, its Escape — skips the question on screen and
    /// every one behind it; placements already answered keep their answers.
    pub(super) fn dismiss_sheet(&mut self) {
        if matches!(self.sheet, Some(Sheet::Conflict { .. })) {
            self.conflict_waiting.clear();
        }
        self.sheet = None;
    }

    /// The click's own dispatch: the question's offers are the guard, so a
    /// row the sheet rendered is a row the answer takes. The shelf scope —
    /// a folder's own name collision — arrives with the import's screen;
    /// every ask a move raises is about a row.
    pub(super) fn apply_placement(&mut self, ask: &ConflictAsk, choice: Placement) -> Task<Message> {
        let offers = conflicts::offers_for(&self.library.books, &self.library.folders, ask);
        if !offers.contains(&choice) {
            return Task::none();
        }
        match choice {
            Placement::Open => self.reveal_existing(&ask.existing_id),
            Placement::KeepBoth => self.as_new(ask),
            Placement::LinkOnly => self.link_to_row(ask),
            Placement::Merge => self.merge_into_row(ask),
            Placement::Replace => self.replace_row(ask),
        }
    }

    /// The modal question in flight: the rename sheet or the remove sheet.
    pub(super) fn sheet_layer(&self) -> Option<Element<'_, Message>> {
        let sheet_state = self.sheet.as_ref()?;
        let panel: Element<'_, Message> = match sheet_state {
            Sheet::Rename { draft, .. } => {
                let input = text_input("Name", draft)
                    .id(SHEET_INPUT)
                    .on_input(Message::SheetDraft)
                    .on_submit(Message::SheetSave)
                    .size(13)
                    .width(Length::Fill)
                    .padding(Padding { top: 7.0, right: 10.0, bottom: 7.0, left: 10.0 })
                    .style(move |_theme, _status| text_input::Style {
                        background: Background::Color(self.tokens.paper),
                        border: Border {
                            color: self.tokens.line,
                            width: 1.0,
                            radius: 8.0.into(),
                        },
                        icon: self.tokens.muted,
                        placeholder: self.tokens.muted,
                        value: self.tokens.ink,
                        selection: self.tokens.accent_soft,
                    });
                sheet::panel(
                    self.tokens,
                    "Rename",
                    input.into(),
                    vec![
                        sheet::cancel_button(self.tokens, "Cancel", Message::SheetCancel),
                        sheet::confirm_button(self.tokens, "Save", Message::SheetSave, false),
                    ],
                )
            }
            Sheet::Remove { name, .. } => {
                let body = text(format!(
                    "Remove “{name}” from the library? {}",
                    "The file on disk stays where it is."
                ))
                .size(13)
                .color(self.tokens.muted);
                sheet::panel(
                    self.tokens,
                    "Remove",
                    body.into(),
                    vec![
                        sheet::cancel_button(self.tokens, "Cancel", Message::SheetCancel),
                        sheet::confirm_button(self.tokens, "Remove", Message::SheetSave, true),
                    ],
                )
            }
            Sheet::RemoveMany { books, shelves } => {
                let mut parts: Vec<String> = Vec::new();
                if !books.is_empty() {
                    parts.push(lib_text::plural(books.len(), "book", "books"));
                }
                if !shelves.is_empty() {
                    parts.push(lib_text::plural(shelves.len(), "shelf", "shelves"));
                }
                let body = text(format!(
                    "Remove {} from the library? {}",
                    parts.join(" and "),
                    "The files on disk stay where they are."
                ))
                .size(13)
                .color(self.tokens.muted);
                sheet::panel(
                    self.tokens,
                    "Remove",
                    body.into(),
                    vec![
                        sheet::cancel_button(self.tokens, "Cancel", Message::SheetCancel),
                        sheet::confirm_button(self.tokens, "Remove", Message::SheetSave, true),
                    ],
                )
            }
            Sheet::Import { root, ground } => sheet::panel_sized(
                self.tokens,
                sheet::IMPORT_W,
                "Import books",
                import_sheet(self.tokens, root, &self.import_opts, ground.as_ref()),
                vec![
                    sheet::cancel_button(self.tokens, "Cancel", Message::SheetCancel),
                    sheet::confirm_button(self.tokens, "Import", Message::SheetSave, false),
                ],
            ),
            // The departure's question: the action as the heading, the
            // subject and the cost as the body's own lines, and one button
            // per answer the ask was raised with — Cancel always first, the
            // way the web sheet's footer stood them.
            Sheet::Copy { ask } => {
                let mut body = Column::new().spacing(6);
                body = body.push(text(ask.subject.clone()).size(13).color(self.tokens.muted));
                for line in &ask.lines {
                    body = body.push(text(line.clone()).size(12).color(self.tokens.muted));
                }
                let mut actions =
                    vec![sheet::cancel_button(self.tokens, "Cancel", Message::SheetCancel)];
                for option in &ask.options {
                    actions.push(if option.primary {
                        sheet::confirm_button(
                            self.tokens,
                            &option.label,
                            Message::AnswerCopy(option.answer),
                            false,
                        )
                    } else {
                        sheet::cancel_button(
                            self.tokens,
                            &option.label,
                            Message::AnswerCopy(option.answer),
                        )
                    });
                }
                sheet::panel(self.tokens, ask.action.as_str(), body.into(), actions)
            }
            // The name question: the arriving name as the heading, where the
            // collision is — and what waits behind it — under that, the
            // question in one sentence, and the answers as rows that each
            // promise what choosing them does. Cancel means the same thing
            // on every sheet: leave the shelf as it is, and skip the queue.
            Sheet::Conflict { ask } => {
                let spec = conflicts::describe(
                    &self.library.books,
                    &self.library.shelves,
                    &self.library.folders,
                    ask,
                    &self.conflict_waiting,
                );
                let apply_all = self.apply_all;
                let choices = spec
                    .choices
                    .iter()
                    .map(|choice| {
                        sheet::choice_row(
                            self.tokens,
                            choice.label,
                            choice.note.clone(),
                            Message::AnswerPlacement(choice.placement, apply_all),
                        )
                    })
                    .collect();
                let mut body = Column::new().spacing(10);
                body = body.push(text(spec.subtitle.clone()).size(12).color(self.tokens.muted));
                body = body.push(text(spec.question.clone()).size(12).color(self.tokens.muted));
                body = body.push(sheet::choice_group(self.tokens, choices));
                // Rendered only when questions are actually waiting — a
                // switch offering to answer nothing is a control that lies
                // about its reach.
                if spec.apply_all && spec.waiting > 0 {
                    body = body.push(apply_all_row(self.tokens, spec.waiting, apply_all));
                }
                sheet::panel_sized(
                    self.tokens,
                    sheet::CONFLICT_W,
                    spec.heading.clone(),
                    body.into(),
                    vec![sheet::cancel_button(self.tokens, "Cancel", Message::SheetCancel)],
                )
            }
            Sheet::ShelfConflict { ask } => {
                // Asked before the walk rather than after it: the answers
                // name a run rather than a placement, so the sheet renders
                // the same chrome and routes its rows down a second lane.
                let spec = conflicts::describe_shelf(
                    &self.library.books,
                    &self.library.shelves,
                    &self.library.folders,
                    ask,
                );
                let choices = spec
                    .choices
                    .iter()
                    .map(|choice| {
                        sheet::choice_row(
                            self.tokens,
                            choice.label,
                            choice.note.clone(),
                            Message::AnswerShelf(choice.placement),
                        )
                    })
                    .collect();
                let mut body = Column::new().spacing(10);
                body = body.push(text(spec.subtitle.clone()).size(12).color(self.tokens.muted));
                body = body.push(text(spec.question.clone()).size(12).color(self.tokens.muted));
                body = body.push(sheet::choice_group(self.tokens, choices));
                sheet::panel_sized(
                    self.tokens,
                    sheet::CONFLICT_W,
                    spec.heading.clone(),
                    body.into(),
                    vec![sheet::cancel_button(self.tokens, "Cancel", Message::SheetCancel)],
                )
            }
            Sheet::AlreadyImported { note } => {
                // Not a question — an answer: the shelf is named why, and
                // closing is the label on the button.
                let sentence = conflicts::note_sentence(note.kind, &note.name);
                let mut body = Column::new().spacing(10);
                body = body.push(
                    text(note.kind.sublabel().to_string()).size(12).color(self.tokens.muted),
                );
                body = body.push(text(sentence).size(12).color(self.tokens.muted));
                sheet::panel_sized(
                    self.tokens,
                    sheet::CONFLICT_W,
                    note.name.clone(),
                    body.into(),
                    vec![sheet::confirm_button(
                        self.tokens,
                        "Show the shelf",
                        Message::CloseAlreadyImported,
                        false,
                    )],
                )
            }
        };
        Some(sheet::overlay(panel, Message::SheetCancel))
    }
}
