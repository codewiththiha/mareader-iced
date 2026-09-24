//! The PDF pipeline: the engine that renders pages, and the wire the reading
//! surface speaks to it.
//!
//! The web app had three layers here — a browser worker running pdf.js, the
//! `pdf-engine` crate that called into it through wasm-bindgen, and the views
//! that called the crate. Natively there are two: this module (a thread, a
//! protocol, and the pure arithmetic around them) and the reader, which asks
//! for pages and paints what comes back. Nothing in the reader knows what
//! Pdfium is; nothing here knows what a widget is.
//!
//! * [`channel`] — the sink the worker answers into and the subscription the
//!   Elm loop reads it through.
//! * [`engine`] — the worker thread and the app's handle on it.
//! * [`protocol`] — the requests, the answers, and the geometry both sides
//!   agree on.
//! * [`text`] — Pdfium's per-character boxes grouped into the runs the search
//!   index eats.
//!
//! Where the Pdfium library itself comes from is the platform's business, not
//! this module's: [`crate::platform::pdfium`] owns the search order and the
//! one bind per process.

pub mod channel;
pub mod engine;
pub mod protocol;
pub mod text;

pub use engine::Engine;
pub use protocol::{
    Event, FrameKey, Opened, PageBox, Request, fallback_box, measured,
};
