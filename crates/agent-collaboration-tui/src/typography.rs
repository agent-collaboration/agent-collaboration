//! Visual hierarchy is independent of selection and workflow state.
pub use crate::theme::{UiSurface, UiTextRole};
use crate::{surface_handle::SurfaceContext, theme::Theme};
use iocraft::prelude::*;

fn typography_theme(hooks: Hooks) -> Theme {
    hooks
        .try_use_context::<SurfaceContext>()
        .map_or(Theme::default(), |context| context.theme())
}

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
pub fn UiHeading(props: &UiHeadingProps, hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let theme = typography_theme(hooks);
    let color = if props.inactive {
        Color::Grey
    } else {
        match props.level {
            UiHeadingLevel::Surface => Color::Cyan,
            UiHeadingLevel::Section => UiTextRole::Label.color(),
            UiHeadingLevel::Group => Color::Grey,
        }
    };
    let color = theme.foreground_color(color);
    element!(Text(content: props.content.clone(), color, weight: Weight::Bold, wrap: TextWrap::NoWrap))
}

#[derive(Props)]
pub struct UiTextProps {
    pub content: String,
    #[iocraft(required)]
    pub role: UiTextRole,
    pub wrap: Option<TextWrap>,
}

#[component]
pub fn UiText(props: &UiTextProps, hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let color = typography_theme(hooks).foreground_color(props.role.color());
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

#[derive(Props)]
pub struct UiColumnHeadingProps {
    pub content: String,
}
#[component]
pub fn UiColumnHeading(
    props: &UiColumnHeadingProps,
    hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let theme = typography_theme(hooks);
    element! {
        View(width:100pct,flex_direction:FlexDirection::Column,flex_shrink:0.0_f32,
            border_edges:Edges::Bottom,
            border_style:BorderStyle::Single,border_color:theme.foreground_color(UiTextRole::Rule.color())) {
            Text(content:props.content.clone(),color:theme.foreground_color(UiTextRole::Prominent.color()),weight:Weight::Bold,wrap:TextWrap::Wrap)
        }
    }
}
