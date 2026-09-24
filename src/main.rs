//! Mareader — a desktop document reader for long-form reading, built
//! natively on iced.
//!
//! The binary entry is deliberately the smallest file in the tree: platform
//! probes, the window contract and the Elm loop live in [`app`]; the window
//! chrome in [`chrome`]; the shelf in [`library`]; the reading surface in
//! [`reader`]; the document engines in [`formats`]; the two routes in
//! [`route`]; the design tokens in [`theme`].

// A release build is a GUI; it has no console to write to on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod chrome;
mod formats;
mod library;
mod platform;
mod reader;
mod route;
mod storage;
mod theme;
mod ui;

fn main() -> iced::Result {
    // `--smoke` is the boot proof the build lane runs on every OS: the whole
    // app state is built in-process — settings, the library blob, both
    // surfaces as widget trees — and the process exits 0 without opening a
    // window or asking for a display. That is what lets CI prove the app
    // *starts* where it ships, on runners that have no window server and no
    // PDF engine at all.
    if std::env::args().any(|arg| arg == "--smoke") {
        return app::smoke();
    }
    app::run()
}
