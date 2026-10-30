//! The shipped palette, with a dimmed form for each semantic role and surface.
use crate::{UiSurface, UiTextRole};
use iocraft::Color;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextRole {
    Text,
    Muted,
    Accent,
    Focused,
    Primary,
    Disabled,
    Dimmed,
    Good,
    Warning,
    Error,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SurfaceRole {
    Base,
    Raised,
    GroupPanel,
    Header,
    Selected,
    Alternate,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Theme {
    covered: bool,
}
impl Theme {
    pub const fn covered() -> Self {
        Self { covered: true }
    }
    pub const fn foreground(self, role: TextRole) -> Color {
        if self.covered {
            return Color::Rgb {
                r: 142,
                g: 150,
                b: 163,
            };
        }
        match role {
            TextRole::Text => UiTextRole::Body.color(),
            TextRole::Muted => UiTextRole::Secondary.color(),
            TextRole::Accent => Color::Cyan,
            TextRole::Focused => UiTextRole::Selected.color(),
            TextRole::Primary => UiTextRole::Label.color(),
            TextRole::Disabled => UiTextRole::Disabled.color(),
            TextRole::Dimmed => Color::DarkGrey,
            TextRole::Good => UiTextRole::Success.color(),
            TextRole::Warning => UiTextRole::Warning.color(),
            TextRole::Error => UiTextRole::Error.color(),
        }
    }
    pub const fn background(self, role: SurfaceRole) -> Color {
        let color = match role {
            SurfaceRole::Base => UiSurface::Base.color(),
            SurfaceRole::Raised | SurfaceRole::GroupPanel => UiSurface::Raised.color(),
            SurfaceRole::Header => UiSurface::Header.color(),
            SurfaceRole::Selected => UiSurface::Selected.color(),
            SurfaceRole::Alternate => UiSurface::Alternate.color(),
        };
        match color {
            Color::Rgb { r, g, b } if self.covered => Color::Rgb {
                r: r / 4 * 3,
                g: g / 4 * 3,
                b: b / 4 * 3,
            },
            other => other,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiGlyph {
    Menu,
    Dropdown,
    Current,
    Empty,
    Checked,
    Focus,
    ScrollTrack,
    ScrollThumb,
    Up,
    Down,
    Ellipsis,
    Plus,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Horizontal,
}
impl UiGlyph {
    pub const ALL: [Self; 17] = [
        Self::Menu,
        Self::Dropdown,
        Self::Current,
        Self::Empty,
        Self::Checked,
        Self::Focus,
        Self::ScrollTrack,
        Self::ScrollThumb,
        Self::Up,
        Self::Down,
        Self::Ellipsis,
        Self::Plus,
        Self::TopLeft,
        Self::TopRight,
        Self::BottomLeft,
        Self::BottomRight,
        Self::Horizontal,
    ];
    pub const fn text(self) -> &'static str {
        match self {
            Self::Menu => "≡",
            Self::Dropdown => "▾",
            Self::Current => "●",
            Self::Empty => "○",
            Self::Checked => "✓",
            Self::Focus => "❯",
            Self::ScrollTrack => "│",
            Self::ScrollThumb => "█",
            Self::Up => "↑",
            Self::Down => "↓",
            Self::Ellipsis => "…",
            Self::Plus => "+",
            Self::TopLeft => "┌",
            Self::TopRight => "┐",
            Self::BottomLeft => "└",
            Self::BottomRight => "┘",
            Self::Horizontal => "─",
        }
    }
}
