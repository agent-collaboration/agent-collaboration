//! The shipped palette, with a dimmed form for each semantic role and surface.
use iocraft::Color;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiTextRole {
    Body,
    Prominent,
    Label,
    Rule,
    Secondary,
    Disabled,
    Selected,
    Success,
    Warning,
    Error,
}

impl UiTextRole {
    /// Shared semantic palette; yellow belongs only to active focus or selection.
    pub const fn color(self) -> Color {
        match self {
            Self::Body => Color::White,
            Self::Prominent => Color::Rgb {
                r: 237,
                g: 240,
                b: 245,
            },
            Self::Label => Color::Rgb {
                r: 156,
                g: 188,
                b: 251,
            },
            Self::Rule => Color::Rgb {
                r: 119,
                g: 200,
                b: 187,
            },
            Self::Secondary => Color::Grey,
            Self::Disabled => Color::Rgb {
                r: 166,
                g: 175,
                b: 189,
            },
            Self::Selected => Color::Yellow,
            Self::Success => Color::Green,
            Self::Warning => Color::Rgb {
                r: 255,
                g: 165,
                b: 0,
            },
            Self::Error => Color::Red,
        }
    }
}

/// Solid presentation surfaces; no selection or hover state lives in the palette.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiSurface {
    Base,
    Alternate,
    Raised,
    Header,
    Selected,
}
impl UiSurface {
    pub const fn color(self) -> Color {
        match self {
            Self::Base => Color::Rgb {
                r: 39,
                g: 44,
                b: 52,
            },
            Self::Alternate => Color::Rgb {
                r: 43,
                g: 48,
                b: 57,
            },
            Self::Raised => Color::Rgb {
                r: 48,
                g: 54,
                b: 65,
            },
            Self::Header => Color::Rgb {
                r: 54,
                g: 65,
                b: 83,
            },
            Self::Selected => Color::Rgb {
                r: 52,
                g: 59,
                b: 70,
            },
        }
    }
}

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
        let color = match role {
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
        };
        self.foreground_color(color)
    }
    /// Applies the covered-surface foreground to a consumer's custom colour.
    pub const fn foreground_color(self, color: Color) -> Color {
        if self.covered {
            return Color::Rgb {
                r: 142,
                g: 150,
                b: 163,
            };
        }
        color
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
