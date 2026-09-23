//! Mareader — a desktop document reader for long-form reading, built
//! natively on iced.
//!
//! The binary entry is deliberately the smallest file in the tree: platform
//! probes, the window contract and the Elm loop live in [`app`]; the window
//! chrome in [`chrome`]; the shelf in [`library`]; the two surfaces in
//! [`route`]; the design tokens in [`theme`].

// A release build is a GUI; it has no console to write to on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod chrome;
mod library;
mod platform;
mod route;
mod storage;
mod theme;
mod ui;

fn main() -> iced::Result {
    app::run()
}
