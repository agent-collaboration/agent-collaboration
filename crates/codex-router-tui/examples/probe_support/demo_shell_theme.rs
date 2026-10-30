use codex_router_tui::{UiSurface, UiTextRole};
use iocraft::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackgroundRole {
    Base,
    Surface,
    Group,
    Focused,
    Primary,
}
impl BackgroundRole {
    pub fn color(self, dimmed: bool) -> Color {
        let surface = match self {
            Self::Base | Self::Surface => UiSurface::Base,
            Self::Group => UiSurface::Raised,
            Self::Focused => UiSurface::Selected,
            Self::Primary => UiSurface::Header,
        };
        // Covered backgrounds still dim, deriving from the shipped palette
        // rather than maintaining a second, independently chosen palette.
        match (surface.color(), dimmed) {
            (Color::Rgb { r, g, b }, true) => Color::Rgb {
                r: r / 2,
                g: g / 2,
                b: b / 2,
            },
            (color, _) => color,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusRole {
    Available,
    Warning,
    Error,
}
impl StatusRole {
    pub fn color(self, dimmed: bool) -> Color {
        match (self, dimmed) {
            (Self::Available, false) => UiTextRole::Success.color(),
            (Self::Available, true) => Color::DarkGreen,
            (Self::Warning, false) => UiTextRole::Warning.color(),
            (Self::Warning, true) => match UiTextRole::Warning.color() {
                Color::Rgb { r, g, b } => Color::Rgb {
                    r: r / 2,
                    g: g / 2,
                    b: b / 2,
                },
                color => color,
            },
            (Self::Error, false) => UiTextRole::Error.color(),
            (Self::Error, true) => Color::DarkRed,
        }
    }
}

/// Chrome glyph chosen by the owner; content such as 📂 is deliberately separate.
pub const MENU_ICON: &str = "≡";
