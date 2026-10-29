//! The system's appearance: what `platform.colorScheme()` reports.
//!
//! Extracted from `nsh_core::platform::ColorScheme`, cut to the two schemes
//! v2 has: there is no app-wide override and no automatic token.

/// The system's color scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorScheme {
    /// Light.
    #[default]
    Light,
    /// Dark.
    Dark,
}

impl ColorScheme {
    /// The JS name: `"light"` or `"dark"`.
    pub const fn name(self) -> &'static str {
        match self {
            ColorScheme::Light => "light",
            ColorScheme::Dark => "dark",
        }
    }

    /// The scheme for a system's dark flag.
    pub const fn from_dark(dark: bool) -> Self {
        if dark {
            ColorScheme::Dark
        } else {
            ColorScheme::Light
        }
    }

    /// The system is in its dark appearance.
    pub const fn is_dark(self) -> bool {
        matches!(self, ColorScheme::Dark)
    }
}
