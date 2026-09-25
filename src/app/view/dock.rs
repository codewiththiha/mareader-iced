//! The dock of runs in flight: one pill per run, its line the stage's
//! own wording.

use crate::app::message::Message;
use crate::app::walk::{FsRun, Stage};
use crate::theme::{Elevation, Tokens, wash};
use iced::{Alignment, Background, Border, Element, Length, Padding};
use iced::widget::{Column, container, text};
use library_core::wire::ImportPhase;

/// The runs' live lines: a pill per run at the foot of the shelf, naming
/// the folder or the files and the count so far. Decorations, not controls
/// — a walk is not interruptible, so the pills take no presses and claim no
/// pointer.
pub(super) fn runs_dock(tokens: Tokens, runs: &[FsRun]) -> Element<'static, Message> {
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

