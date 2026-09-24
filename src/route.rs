//! The two surfaces of the app.
//!
//! The web app routed `/` to the library and `/reader` to the reader, with
//! the URL following document state (Ready ⇒ reader, otherwise library).
//! Natively the route is the same fact held in the state tree: which surface
//! is on screen decides the titlebar's composition, its pin, and — later —
//! which settings tabs exist at all.

/// The surface currently on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// The shelf: the library is what is on screen when no document is open.
    Library,
    /// A document is open and being read.
    Reader,
}
