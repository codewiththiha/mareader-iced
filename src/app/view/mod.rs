//! The shelf's frame: the route's own view, and the overlays a bar,
//! a menu or a sheet hangs above it.

use crate::app::Mareader;
use crate::app::ghosts::ghost_layer;
use crate::app::message::Message;
use crate::library;
use crate::library::fold;
use crate::route::Route;
use dock::runs_dock;
use iced::{Element, Length};
use iced::time::Instant;
use iced::widget::{Space, container, mouse_area, stack};

mod bar;
mod dock;
mod layers;
mod select;

/// What the fold keeps for the search before it starts hiding levels.
pub(super) const CENTER_FLOOR: f32 = 400.0;

/// The scrim under an open menu: transparent, window-wide, and closing the
/// menu on any press that reaches it.
fn scrim() -> Element<'static, Message> {
    mouse_area(container(Space::new().width(Length::Fill).height(Length::Fill)))
        .on_press(Message::CloseMenu)
        .into()
}

impl Mareader {
    pub(in crate::app) fn view(&self) -> Element<'_, Message> {
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
}
