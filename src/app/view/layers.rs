//! What floats above the shelf: the scrim, the fold's panel, and the
//! two menus the pointer opens.

use crate::app::Mareader;
use crate::app::message::{ContextTarget, MenuKind, Message};
use crate::chrome::desktop;
use crate::library::{self, bar, menus};
use crate::library::fold::{self, FoldPlan};
use crate::ui::popover;
use iced::{Alignment, Element, Length, Padding, Point, Size};
use iced::widget::container;
use library_core::book;
use library_core::shelf;

impl Mareader {
    /// Whether the pointer is inside the fold panel's box: the same
    /// estimate the placement rides — the packed size anchored under the
    /// ellipsis, clamped into the viewport.
    pub(in crate::app) fn pointer_in_panel(&self) -> bool {
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
}
impl Mareader {
    /// The fold's panel, placed: the popover's own clamp-and-place answer,
    /// anchored under the ellipsis.
    pub(super) fn ellipsis_layer(&self, plan: &FoldPlan) -> Option<Element<'_, Message>> {
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
}
impl Mareader {
    /// The open panel, clamped into the viewport at the pointer's last
    /// position.
    pub(super) fn menu_layer(&self) -> Option<Element<'_, Message>> {
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
}
impl Mareader {
    /// The right-click menu, clamped into the viewport where the pointer
    /// asked for it.
    pub(super) fn context_layer(&self) -> Option<Element<'_, Message>> {
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
}
