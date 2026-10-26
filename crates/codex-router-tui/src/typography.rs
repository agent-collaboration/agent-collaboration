//! Visual hierarchy is independent of selection and workflow state.
use iocraft::prelude::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiHeadingLevel {
    Surface,
    Section,
    Group,
}

#[derive(Props)]
pub struct UiHeadingProps {
    pub content: String,
    #[iocraft(required)]
    pub level: UiHeadingLevel,
    pub inactive: bool,
}

#[component]
pub fn UiHeading(props: &UiHeadingProps) -> impl Into<AnyElement<'static>> {
    let color = if props.inactive {
        Color::Grey
    } else {
        match props.level {
            UiHeadingLevel::Surface => Color::Cyan,
            UiHeadingLevel::Section => UiTextRole::Label.color(),
            UiHeadingLevel::Group => Color::Grey,
        }
    };
    element!(Text(content: props.content.clone(), color, weight: Weight::Bold, wrap: TextWrap::NoWrap))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiTextRole {
    Body,
    WeeklyWindow,
    ShortWindow,
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
            Self::WeeklyWindow => Color::Rgb {
                r: 237,
                g: 240,
                b: 245,
            },
            Self::ShortWindow => Color::Rgb {
                r: 206,
                g: 212,
                b: 221,
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

#[derive(Props)]
pub struct UiTextProps {
    pub content: String,
    #[iocraft(required)]
    pub role: UiTextRole,
    pub wrap: Option<TextWrap>,
}

#[component]
pub fn UiText(props: &UiTextProps) -> impl Into<AnyElement<'static>> {
    let color = props.role.color();
    let weight = if props.role == UiTextRole::Selected {
        Weight::Bold
    } else {
        Weight::Normal
    };
    element!(Text(content: props.content.clone(), color, weight, wrap: props.wrap.unwrap_or(TextWrap::NoWrap)))
}

#[derive(Props)]
pub struct SurfaceHeaderProps<'a> {
    pub title: String,
    pub inactive: bool,
    pub column_gap: Option<u16>,
    pub help_action: Option<AnyElement<'a>>,
    pub children: Vec<AnyElement<'a>>,
}

/// A clipped title, optional status and Help share one terminal row.
#[component]
pub fn SurfaceHeader<'a>(props: &mut SurfaceHeaderProps<'a>) -> impl Into<AnyElement<'a>> {
    let help_action = props.help_action.take();
    let children = std::mem::take(&mut props.children);
    element! {
        View(width: 100pct, column_gap: props.column_gap.unwrap_or(2), flex_shrink: 0.0_f32) {
            View(flex_grow: 1.0_f32, flex_shrink: 1.0_f32, min_width: 0, overflow: Overflow::Hidden) {
                UiHeading(content: props.title.clone(), level: UiHeadingLevel::Surface, inactive: props.inactive)
            }
            #(children)
            View(flex_shrink: 0.0_f32) { #(help_action) }
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

#[derive(Props)]
pub struct UiColumnHeadingProps {
    pub content: String,
}
#[component]
pub fn UiColumnHeading(props: &UiColumnHeadingProps) -> impl Into<AnyElement<'static>> {
    element! {
        View(width:100pct,flex_direction:FlexDirection::Column,flex_shrink:0.0_f32,
            border_edges:Edges::Bottom,
            border_style:BorderStyle::Single,border_color:UiTextRole::Rule.color()) {
            Text(content:props.content.clone(),color:UiTextRole::WeeklyWindow.color(),weight:Weight::Bold,wrap:TextWrap::Wrap)
        }
    }
}
