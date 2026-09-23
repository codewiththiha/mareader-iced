//! Which desktop the window chrome draws for, and the numbers it draws with.
//!
//! Natively there is no user-agent to probe: `std::env::consts::OS` settles
//! the platform once. The three families the web app's chrome had are the
//! same three here — macOS wears the OS's own traffic lights (the window is
//! created with a transparent, full-size-content titlebar, so AppKit paints
//! them over the content), while Windows and Linux run frameless and wear
//! the app's caption clusters: squares on Windows, circles on GNOME.

/// The desktops this app ships on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    MacOs,
    Windows,
    Linux,
}

/// The desktop this process runs on. Anything unrecognised is treated as
/// Linux: frameless with the app's own caption cluster is the safe default.
pub fn os() -> Os {
    match std::env::consts::OS {
        "macos" => Os::MacOs,
        "windows" => Os::Windows,
        _ => Os::Linux,
    }
}

/// The bar's height — the number both sides of the chrome contract write
/// against (the web app declared it as `TITLE_BAR_H = 48` in its titlebar
/// module and its stylesheet agreed).
pub const TITLE_BAR_H: f32 = 48.0;

/// The invisible band along the top edge that reveals a hidden bar when the
/// pointer enters it.
pub const REVEAL_BAND: f32 = 12.0;

/// How long a bar lingers after the pointer leaves before it hides — the
/// web chrome's `DEFAULT_HOVER_DELAY`, shared so surfaces cannot drift apart.
pub const HIDE_GRACE_MS: u64 = 400;

/// The left reservation on macOS: the traffic lights sit at a 20px inset on
/// ~20px spacing, so bar content starts clear of the third light.
pub const MACOS_LIGHTS_INSET: f32 = 78.0;

/// A Windows caption button's width — 46px full-height hits, the platform
/// convention.
pub const WIN_CAPTION_W: f32 = 46.0;

/// A GNOME caption circle's diameter.
pub const GNOME_BUTTON_D: f32 = 24.0;
