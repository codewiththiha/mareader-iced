//! The bar's numbers: the metrics the chrome draws with, shared so the
//! window, the shelf and the reader cannot drift apart. Which desktop is
//! running is the platform layer's question now (`platform::os()`).

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
