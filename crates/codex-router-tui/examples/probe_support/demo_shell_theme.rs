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
        let (r, g, b) = match (self, dimmed) {
            (Self::Base, false) => (12, 17, 25),
            (Self::Base, true) => (6, 9, 14),
            (Self::Surface, false) => (21, 28, 39),
            (Self::Surface, true) => (10, 14, 20),
            (Self::Group, false) => (30, 41, 56),
            (Self::Group, true) => (14, 20, 28),
            (Self::Focused, false) => (43, 79, 120),
            (Self::Focused, true) => (17, 29, 43),
            (Self::Primary, false) => (24, 87, 83),
            (Self::Primary, true) => (11, 30, 29),
        };
        Color::Rgb { r, g, b }
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
            (Self::Available, false) => Color::Green,
            (Self::Available, true) => Color::DarkGreen,
            (Self::Warning, false) => Color::Yellow,
            (Self::Warning, true) => Color::DarkYellow,
            (Self::Error, false) => Color::Red,
            (Self::Error, true) => Color::DarkRed,
        }
    }
}

/// Chrome glyph chosen by the owner; content such as 📂 is deliberately separate.
pub const MENU_ICON: &str = "≡";
