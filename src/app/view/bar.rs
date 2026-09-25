//! The titlebar: the route's slots on the ground's own bar, and the
//! fold that keeps them inside the window.

use super::CENTER_FLOOR;
use crate::app::Mareader;
use crate::app::message::{MenuKind, Message};
use crate::chrome::desktop;
use crate::chrome::icons::{IconName, icon};
use crate::chrome::titlebar;
use crate::library::bar;
use crate::library::fold::FoldPlan;
use crate::platform::{self, Os};
use crate::route::Route;
use crate::theme::wash;
use crate::ui::buttons;
use iced::Element;
use iced::widget::button;
use library_core::book;
use reader_core::appearance::BaseMode;

/// The appearance button's glyph for the base on screen.
fn appearance_glyph(base: BaseMode) -> IconName {
    match base {
        BaseMode::Light => IconName::Sun,
        BaseMode::Dark => IconName::Moon,
        BaseMode::Dim => IconName::Dim,
    }
}

impl Mareader {
    /// The titlebar with the route's slots hung on it.
    pub(in crate::app) fn bar(&self, factor: f32, plan: Option<&FoldPlan>) -> Element<'_, Message> {
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
}
impl Mareader {
    /// The inset the cluster starts at: AppKit paints the traffic lights
    /// over the content on macOS, and the bar keeps clear of them.
    pub(super) fn bar_left_inset(&self) -> f32 {
        match platform::os() {
            Os::Mac => desktop::MACOS_LIGHTS_INSET + 8.0,
            _ => 8.0,
        }
    }
}
impl Mareader {
    /// The fold's budget: what the row leaves the left cluster once the
    /// right cluster's chrome and the centre pill's usable floor are
    /// reserved. The web bar measured this box live; the native bar
    /// estimates it — two route triggers and the pin, the OS's own
    /// captions, and the pill's floor — and the fold's arithmetic is the
    /// same.
    pub(super) fn crumb_avail(&self) -> f32 {
        let right = match platform::os() {
            Os::Mac => 114.0,
            Os::Windows => 236.0,
            Os::Linux => 248.0,
        };
        (self.viewport.width - self.bar_left_inset() - right - CENTER_FLOOR).max(160.0)
    }
}
