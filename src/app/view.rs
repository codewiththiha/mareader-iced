//! The shelf's frame: the bar, the overlay layers a menu or sheet hangs
//! from, and the small widgets the menus borrow.
use iced::time::Instant;
use iced::widget::{button, container, mouse_area, row, stack, text, Column, Space};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Point, Shadow, Size};
use library_core::book::{self};
use library_core::shelf::{self, ALL_SHELF};
use library_core::wire::ImportPhase;
use reader_core::appearance::BaseMode;

use crate::chrome::desktop;
use crate::chrome::icons::{icon, IconName};
use crate::chrome::titlebar::{self};
use crate::library::fold::{self, FoldPlan};
use crate::library::{self, bar, menus};
use crate::platform::{self, Os};
use crate::route::Route;
use crate::theme::{wash, Elevation, Tokens};
use crate::ui::{buttons, menu as popover};
use super::Mareader;
use super::ghosts::ghost_layer;
use super::message::{ContextTarget, MenuKind, Message};
use super::selection::SELECT_POP_W;
use super::walk::{CENTER_FLOOR, FsRun, Stage};

/// The scrim under an open menu: transparent, window-wide, and closing the
/// menu on any press that reaches it.
fn scrim() -> Element<'static, Message> {
    mouse_area(container(Space::new().width(Length::Fill).height(Length::Fill)))
        .on_press(Message::CloseMenu)
        .into()
}

/// The bar's own pill: the count readout and the four answers, in the
/// ActionBar's chrome — the surface, the hairline, the float's shadow.
fn select_pill(tokens: Tokens, count: usize, pop_open: bool) -> Element<'static, Message> {
    let face = row![
        container(text(format!("{count} selected")).size(12).color(tokens.muted))
            .padding(Padding { top: 0.0, right: 8.0, bottom: 0.0, left: 0.0 }),
        pill_button(tokens, "All", false, Some(Message::SelectAll)),
        pill_button(
            tokens,
            "Add to shelf",
            pop_open,
            Some(Message::ToggleSelectPop),
        ),
        pill_button(
            tokens,
            &format!("Remove ({count})"),
            false,
            (count > 0).then_some(Message::AskRemoveSelection),
        ),
        pill_button(tokens, "Done", false, Some(Message::ClearSelection)),
    ]
    .spacing(4)
    .align_y(Alignment::Center);
    container(face)
        .padding(Padding { top: 6.0, right: 6.0, bottom: 6.0, left: 16.0 })
        .style(move |_| container::Style {
            background: Some(Background::Color(tokens.surface)),
            border: Border { color: tokens.line, width: 1.0, radius: 999.0.into() },
            shadow: Elevation::Pill.shadow(),
            ..container::Style::default()
        })
        .into()
}

/// One answer on the bar: quiet text in a round ghost, red for the
/// dangerous one, washed while open for the one holding the popover.
/// `None` for the message renders the answer disabled — listed, but not
/// quietly dropped.
fn pill_button(
    tokens: Tokens,
    label: &str,
    active: bool,
    message: Option<Message>,
) -> Element<'static, Message> {
    let danger = message.as_ref().is_some_and(|message| {
        matches!(message, Message::AskRemoveSelection)
    });
    let ink = if danger { crate::theme::DANGER } else { tokens.ink };
    let action = button(text(label.to_string()).size(12).color(ink))
        .padding(Padding { top: 5.0, right: 12.0, bottom: 5.0, left: 12.0 })
        .style(move |_, status| {
            let wash_of = if danger { crate::theme::DANGER } else { tokens.accent };
            let background = match status {
                button::Status::Hovered => Some(Background::Color(wash(wash_of, 0.12))),
                button::Status::Pressed => Some(Background::Color(wash(wash_of, 0.20))),
                _ => active.then_some(Background::Color(wash(tokens.accent, 0.12))),
            };
            button::Style {
                background,
                border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 999.0.into() },
                text_color: ink,
                shadow: Shadow::default(),
                snap: false,
            }
        });
    match message {
        Some(message) => action.on_press(message).into(),
        None => action.into(),
    }
}

/// The runs' live lines: a pill per run at the foot of the shelf, naming
/// the folder or the files and the count so far. Decorations, not controls
/// — a walk is not interruptible, so the pills take no presses and claim no
/// pointer.
fn runs_dock(tokens: Tokens, runs: &[FsRun]) -> Element<'static, Message> {
    let mut pills = Column::new().spacing(8).align_x(Alignment::Center);
    for run in runs {
        pills = pills.push(dock_pill(tokens, run_line(run)));
    }
    container(pills)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding { top: 0.0, right: 0.0, bottom: 20.0, left: 0.0 })
        .align_x(Alignment::Center)
        .align_y(Alignment::End)
        .into()
}

/// One pill: the line in a rounded card, floating over the shelf.
fn dock_pill(tokens: Tokens, line: String) -> Element<'static, Message> {
    container(
        container(text(line).size(12).color(tokens.ink))
            .padding(Padding { top: 8.0, right: 14.0, bottom: 8.0, left: 14.0 })
            .style(move |_| container::Style {
                background: Some(Background::Color(wash(tokens.surface, 0.95))),
                border: Border { color: tokens.line, width: 1.0, radius: 999.0.into() },
                shadow: Elevation::Pill.shadow(),
                ..container::Style::default()
            }),
    )
    .into()
}

/// The pill's line: the phase the last beat carries decides the verb, and
/// the stage decides the waiting wording before the first beat lands.
fn run_line(run: &FsRun) -> String {
    match &run.latest {
        Some(beat) if beat.phase == ImportPhase::Copy && beat.total > 0 => {
            format!("Copying “{}”: {} of {}", run.label, beat.done, beat.total)
        }
        Some(beat) if beat.total > 0 => {
            format!("Scanning “{}”: {} of {} found", run.label, beat.done, beat.total)
        }
        Some(beat) if beat.done > 0 => {
            format!("Scanning “{}”: {} found", run.label, beat.done)
        }
        _ => match run.stage {
            Stage::Measuring { .. } => format!("Measuring “{}”…", run.label),
            Stage::Restoring { .. } => format!("Restoring “{}”…", run.label),
            Stage::Copying { .. }
            | Stage::RestoreCopying { .. }
            | Stage::Duplicating { .. }
            | Stage::Departing { .. } => {
                format!("Copying “{}”…", run.label)
            }
            Stage::Walking { .. } | Stage::Storing { .. } | Stage::CopiesScan { .. } => {
                format!("Scanning “{}”…", run.label)
            }
            Stage::Copies { .. } => format!("Copying “{}”…", run.label),
        },
    }
}

/// A toggle chip: the active one wears the accent's soft bed, the inactive
/// one waits quiet. `None` for the message renders it disabled.
pub(super) fn chip(
    tokens: Tokens,
    label: &'static str,
    active: bool,
    message: Option<Message>,
) -> Element<'static, Message> {
    let action = button(text(label).size(12).color(if active { tokens.ink } else { tokens.muted }))
        .width(Length::Fill)
        .padding(Padding { top: 6.0, right: 10.0, bottom: 6.0, left: 10.0 })
        .style(move |_, status| {
            let background = if active {
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
                    color: if active { tokens.accent } else { tokens.line },
                    width: 1.0,
                    radius: 8.0.into(),
                },
                text_color: if active { tokens.ink } else { tokens.muted },
                shadow: Shadow::default(),
                snap: false,
            }
        });
    match message {
        Some(message) => action.on_press(message).into(),
        None => action.into(),
    }
}

/// The size threshold's round adjuster.
pub(super) fn stepper(tokens: Tokens, glyph: IconName, enabled: bool, message: Message) -> Element<'static, Message> {
    let face = container(icon(glyph, 13, if enabled { tokens.ink } else { tokens.muted }))
        .padding(5.0);
    let action = button(face).style(move |_, status| button::Style {
        background: Some(Background::Color(match status {
            button::Status::Hovered | button::Status::Pressed if enabled => wash(tokens.line, 0.60),
            _ => wash(tokens.line, 0.35),
        })),
        border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 999.0.into() },
        text_color: tokens.ink,
        shadow: Shadow::default(),
        snap: false,
    });
    if enabled {
        action.on_press(message).into()
    } else {
        action.into()
    }
}

/// The appearance button's glyph for the base on screen.
fn appearance_glyph(base: BaseMode) -> IconName {
    match base {
        BaseMode::Light => IconName::Sun,
        BaseMode::Dark => IconName::Moon,
        BaseMode::Dim => IconName::Dim,
    }
}

impl Mareader {
    pub(super) fn view(&self) -> Element<'_, Message> {
        let now = Instant::now();
        let factor = self.titlebar.factor(now);
        // The drag's standing answer, recomputed per frame: the effect the
        // cells dress for and the fold preview the ghost would wear. The
        // table itself answers None when no drag is in flight.
        let answer = self.drag_answer();
        // The breadcrumb's fold, decided against the bar's estimated
        // budget: the same arithmetic the web bar ran against its
        // measured one.
        let plan = (self.route == Route::Library)
            .then(|| fold::plan(&self.library, &self.shelf, self.crumb_avail()));

        let content: Element<'_, Message> = match self.route {
            Route::Library => library::view(
                self.tokens,
                &self.library,
                &self.shelf,
                &self.query,
                self.hovered_card.as_deref(),
                self.viewport.width,
                library::SelectionFacts {
                    selecting: self.selecting,
                    selected: &self.selected,
                    lit: self.reveal.as_ref().map(|(revealed, _)| revealed.id.as_str()),
                },
                library::DragFacts {
                    payload: self.drag.as_ref().map(|drag| &drag.payload),
                    effect: answer.as_ref().map(|(effect, _)| effect),
                },
            ),
            Route::Reader => self.reader.view(self.tokens).map(Message::Reader),
        };

        let mut layers: Vec<Element<'_, Message>> = vec![content];
        // An open panel: the scrim that closes it on any outside press, and
        // the panel itself placed at the pointer. The bar stays above the
        // scrim, so its trigger still toggles the panel shut.
        if self.menu.is_some() {
            layers.push(scrim());
            if let Some(panel) = self.menu_layer() {
                layers.push(panel);
            }
        }
        // A right-click menu: the same scrim-and-panel answer, placed where
        // the pointer asked.
        if self.context.is_some() {
            layers.push(scrim());
            if let Some(panel) = self.context_layer() {
                layers.push(panel);
            }
        }
        // The runs' live lines, while any are in flight.
        if self.route == Route::Library && !self.runs.is_empty() {
            layers.push(runs_dock(self.tokens, &self.runs));
        }
        // The fold's panel, while open: the elided chain packed into rows,
        // hanging under the ellipsis. The ghost rides above it — the web's
        // own lane order puts both fold lanes below the drag overlay.
        if self.ellipsis_open
            && let Some(plan) = plan.as_ref().filter(|plan| plan.split > 0)
            && let Some(panel) = self.ellipsis_layer(plan)
        {
            layers.push(panel);
        }
        // The selection's bar, while the mode is on. Bottom-right, where
        // the web ActionBar stood; the runs' dock holds the centre, so the
        // two never argue over one corner.
        if self.route == Route::Library && self.selecting {
            layers.push(self.select_bar());
        }
        // The drag's ghost rides above the level and its bars — what the
        // pointer carries, drawn at the pointer, until the release answers.
        // The titlebar stays above it: chrome is chrome, even mid-drag.
        if let Some(drag) = &self.drag {
            layers.push(ghost_layer(
                self.tokens,
                drag,
                &self.library,
                answer.as_ref().and_then(|(_, fold)| fold.as_ref()),
                self.cursor,
            ));
        }
        // A hidden bar is not in the tree at all: nothing to hit, nothing
        // to hover — the reveal band lives in the cursor subscription.
        if factor > 0.0 {
            layers.push(self.bar(factor, plan.as_ref()));
        }
        // A sheet covers the whole window, bar included: the shelf waits on
        // its answer.
        if self.sheet.is_some()
            && let Some(panel) = self.sheet_layer()
        {
            layers.push(panel);
        }
        if let Some(toast) = self.toasts.view(self.tokens) {
            layers.push(toast);
        }

        stack(layers).width(Length::Fill).height(Length::Fill).into()
    }

    /// The titlebar with the route's slots hung on it.
    fn bar(&self, factor: f32, plan: Option<&FoldPlan>) -> Element<'_, Message> {
        let (left, center, right) = match self.route {
            Route::Library => {
                let book_count = book::book_rows(&self.library.books).count();
                // The crumb under the drag, and only while one is live:
                // the hot fact the bar dresses and the sink counts.
                let hot_crumb =
                    if self.drag.is_some() { self.hovered_crumb.as_deref() } else { None };
                let view_trigger = button(icon(IconName::More, 15, wash(self.tokens.ink, factor)))
                    .padding(7.0)
                    .style(move |_, status| buttons::ghost(self.tokens, factor, status))
                    .on_press(Message::ToggleMenu(MenuKind::View));
                let appearance =
                    button(icon(appearance_glyph(self.settings.appearance.base), 15, wash(self.tokens.ink, factor)))
                        .padding(7.0)
                        .style(move |_, status| {
                            buttons::ghost(self.tokens, factor, status)
                        })
                        .on_press(Message::CycleAppearance);
                (
                    plan.map(|plan| {
                        bar::breadcrumb(
                            self.tokens,
                            plan,
                            factor,
                            self.renaming,
                            &self.rename_draft,
                            bar::CrumbFacts {
                                hot: hot_crumb,
                                ellipsis_open: self.ellipsis_open,
                            },
                        )
                    }),
                    Some(bar::search(self.tokens, book_count, &self.query, factor)),
                    vec![view_trigger.into(), appearance.into()],
                )
            }
            Route::Reader => (None, None, Vec::new()),
        };

        let title = self.shown_title();

        titlebar::view(
            &self.titlebar,
            titlebar::ViewContext {
                tokens: self.tokens,
                route: self.route,
                maximized: self.maximized,
                factor,
                title,
                left,
                center,
                right,
                chrome: Message::Chrome,
            },
        )
    }

    /// The inset the cluster starts at: AppKit paints the traffic lights
    /// over the content on macOS, and the bar keeps clear of them.
    fn bar_left_inset(&self) -> f32 {
        match platform::os() {
            Os::Mac => desktop::MACOS_LIGHTS_INSET + 8.0,
            _ => 8.0,
        }
    }

    /// The fold's budget: what the row leaves the left cluster once the
    /// right cluster's chrome and the centre pill's usable floor are
    /// reserved. The web bar measured this box live; the native bar
    /// estimates it — two route triggers and the pin, the OS's own
    /// captions, and the pill's floor — and the fold's arithmetic is the
    /// same.
    fn crumb_avail(&self) -> f32 {
        let right = match platform::os() {
            Os::Mac => 114.0,
            Os::Windows => 236.0,
            Os::Linux => 248.0,
        };
        (self.viewport.width - self.bar_left_inset() - right - CENTER_FLOOR).max(160.0)
    }

    /// Whether the pointer is inside the fold panel's box: the same
    /// estimate the placement rides — the packed size anchored under the
    /// ellipsis, clamped into the viewport.
    pub(super) fn pointer_in_panel(&self) -> bool {
        let plan = fold::plan(&self.library, &self.shelf, self.crumb_avail());
        if plan.split == 0 {
            return false;
        }
        let budget = (self.viewport.width - 24.0).max(160.0);
        let size = bar::panel_size(&plan, budget);
        let at = popover::place(bar::ellipsis_anchor(self.bar_left_inset()), size, self.viewport);
        self.cursor.x >= at.x
            && self.cursor.x <= at.x + size.width
            && self.cursor.y >= at.y
            && self.cursor.y <= at.y + size.height
    }

    /// The fold's panel, placed: the popover's own clamp-and-place answer,
    /// anchored under the ellipsis.
    fn ellipsis_layer(&self, plan: &FoldPlan) -> Option<Element<'_, Message>> {
        // The panel's packing budget, straight off the web: the window's
        // width less its own air, floored so a sliver of a window still
        // gives every crumb a row.
        let budget = (self.viewport.width - 24.0).max(160.0);
        let hot = if self.drag.is_some() { self.hovered_crumb.as_deref() } else { None };
        let (panel, size) = bar::ellipsis_panel(self.tokens, plan, hot, budget);
        let at = popover::place(bar::ellipsis_anchor(self.bar_left_inset()), size, self.viewport);
        Some(
            container(panel)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(Padding {
                    top: at.y.max(0.0),
                    right: 0.0,
                    bottom: 0.0,
                    left: at.x.max(0.0),
                })
                .align_x(Alignment::Start)
                .align_y(Alignment::Start)
                .into(),
        )
    }

    /// The open panel, clamped into the viewport at the pointer's last
    /// position.
    fn menu_layer(&self) -> Option<Element<'_, Message>> {
        let kind = self.menu?;
        let (panel, size) = match kind {
            MenuKind::Add => menus::add_menu(self.tokens, &self.add_facts()),
            MenuKind::View => menus::view_menu(self.tokens, &self.library.view),
            MenuKind::Shelf => menus::shelf_menu(self.tokens, &self.shelf),
        };
        let anchor = Point::new(self.cursor.x, desktop::TITLE_BAR_H + 2.0);
        let at = popover::place(anchor, size, self.viewport);
        Some(
            container(panel)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(Padding {
                    top: at.y.max(0.0),
                    right: 0.0,
                    bottom: 0.0,
                    left: at.x.max(0.0),
                })
                .align_x(Alignment::Start)
                .align_y(Alignment::Start)
                .into(),
        )
    }

    /// The right-click menu, clamped into the viewport where the pointer
    /// asked for it.
    fn context_layer(&self) -> Option<Element<'_, Message>> {
        let request = self.context.as_ref()?;
        let (panel, size): (Element<'static, Message>, Size) = match &request.target {
            ContextTarget::Row(id) => {
                let row = book::find_row(&self.library.books, id)?;
                menus::row_menu(self.tokens, row)
            }
            ContextTarget::Folder(id) => {
                let shelf = shelf::find(&self.library.shelves, id)?;
                menus::folder_menu(self.tokens, shelf, self.watch_facts(id))
            }
            ContextTarget::Selection => menus::selection_menu(self.tokens, self.selected.len()),
            ContextTarget::Level => {
                let anything = !library::level_rows(&self.library, &self.shelf, &self.query)
                    .is_empty()
                    || !library::level_folders(&self.library, &self.shelf, &self.query)
                        .is_empty();
                menus::level_menu(self.tokens, self.selecting, anything)
            }
        };
        let at = popover::place(request.at, size, self.viewport);
        Some(
            container(panel)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(Padding {
                    top: at.y.max(0.0),
                    right: 0.0,
                    bottom: 0.0,
                    left: at.x.max(0.0),
                })
                .align_x(Alignment::Start)
                .align_y(Alignment::Start)
                .into(),
        )
    }

    /// The selection's bar: the count, All, the shelf answer, Remove, and
    /// Done in a pill at the shelf's bottom-right — the ActionBar's shape,
    /// surface and hairline and fully round. The bar and its popover sit in
    /// mouse areas that capture and answer nothing, so a press on their
    /// chrome cannot fall through to the floor underneath and leave the
    /// mode the bar is for.
    fn select_bar(&self) -> Element<'_, Message> {
        let count = self.selected.len();
        let mut face = Column::new().spacing(8).align_x(Alignment::End);
        if self.select_pop {
            face = face
                .push(mouse_area(self.select_pop_panel()).on_press(Message::KeepSelection));
        }
        face = face.push(
            mouse_area(select_pill(self.tokens, count, self.select_pop))
                .on_press(Message::KeepSelection),
        );
        container(face)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(Padding { top: 0.0, right: 20.0, bottom: 20.0, left: 0.0 })
            .align_x(Alignment::End)
            .align_y(Alignment::End)
            .into()
    }

    /// The bar's "Add to shelf" panel: every shelf the WHOLE set may land
    /// on — one any chosen folder would end up inside itself on answers
    /// for none of them, and renders disabled rather than vanishing — and
    /// the door to a shelf minted for the occasion.
    fn select_pop_panel(&self) -> Element<'static, Message> {
        let (_, folder_ids) = self.split_selection();
        let mut rows: Vec<Element<'static, Message>> = Vec::new();
        rows.push(popover::section(self.tokens, "Add to shelf"));
        for shelf in &self.library.shelves {
            if shelf.id == ALL_SHELF {
                continue;
            }
            let nestable = folder_ids
                .iter()
                .all(|id| shelf::can_nest(&self.library.shelves, id, &shelf.id));
            rows.push(popover::item(
                self.tokens,
                Some(IconName::Folder),
                shelf.name.clone(),
                None,
                false,
                nestable.then(|| Message::FileSelection(shelf.id.clone())),
            ));
        }
        rows.push(popover::separator(self.tokens));
        rows.push(popover::item(
            self.tokens,
            Some(IconName::Plus),
            "New shelf",
            None,
            false,
            Some(Message::FileSelectionOnNewShelf),
        ));
        popover::popover(self.tokens, rows, SELECT_POP_W)
    }
}
