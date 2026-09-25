//! One sheet of paper: the raster when it has landed, and blank paper in the box
//! it will land in until then.

use iced::widget::{container, image, Space};
use iced::{Background, Border, Color, ContentFit, Element, Length};

use crate::theme::{Elevation, Tokens};
use super::super::frame::Frame;
use super::super::Message;

/// A page: white paper in every base mode, deliberately. A PDF page IS white
/// paper, and the appearance system that tints a page tints the raster rather
/// than the card behind it, so a dark base reads as a white sheet on a dark
/// desk.
///
/// The box is exactly the one the scale resolved, whether or not a raster has
/// landed: a page that grew the moment its pixels arrived would make every turn
/// a twitch.
pub(super) fn view<'a>(
    width: f32,
    height: f32,
    frame: Option<&Frame>,
    tokens: Tokens,
) -> Element<'a, Message> {
    let (width, height) = (width.max(1.0), height.max(1.0));
    let content: Element<'a, Message> = match frame {
        Some(frame) => image(frame.handle.clone())
            .width(Length::Fixed(width))
            .height(Length::Fixed(height))
            // The raster is the page at the right size: filling the box rather
            // than containing it keeps the page's edge exactly on the box's
            // edge, which is what a page turn must not shift. A capped raster
            // comes back smaller and is drawn up to this box.
            .content_fit(ContentFit::Fill)
            .into(),
        None => Space::new()
            .width(Length::Fixed(width))
            .height(Length::Fixed(height))
            .into(),
    };
    container(content)
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .style(move |_| container::Style {
            background: Some(Background::Color(Color::WHITE)),
            border: Border {
                color: tokens.line,
                width: 1.0,
                radius: 2.0.into(),
            },
            shadow: Elevation::Page.shadow(),
            ..container::Style::default()
        })
        .into()
}
