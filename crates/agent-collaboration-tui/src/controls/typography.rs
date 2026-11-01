//! Visual hierarchy is independent of selection and workflow state.
pub use crate::foundation::theme::{UiSurface, UiTextRole};
use crate::{foundation::theme::Theme, surfaces::surface_handle::SurfaceContext};
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

/// Shared non-interactive group labels for record lists and menu sections.
#[derive(Props)]
pub(crate) struct GroupHeadingProps {
    pub name: String,
    pub count: Option<usize>,
    pub summary: Option<String>,
}
#[component]
pub(crate) fn GroupHeading(
    props: &GroupHeadingProps,
    hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let theme = typography_theme(hooks);
    element!(View(width:100pct,flex_shrink:0.0_f32,column_gap:1,flex_wrap:FlexWrap::Wrap) {
        Text(content:props.name.clone(),weight:Weight::Bold,color:theme.foreground_color(UiTextRole::Prominent.color()),wrap:TextWrap::Wrap)
        #(props.count.map(|count|element!(Text(content:format!("({count})"),color:theme.foreground_color(UiTextRole::Secondary.color()),wrap:TextWrap::NoWrap))))
        #(props.summary.as_ref().map(|summary|element!(Text(content:summary.clone(),color:theme.foreground_color(UiTextRole::Secondary.color()),wrap:TextWrap::Wrap))))
    })
}

#[cfg(test)]
#[path = "typography_coverage_test.rs"]
mod coverage_test;
#[cfg(test)]
#[path = "heading_style_test.rs"]
mod heading_test;
#[cfg(test)]
#[path = "typography_test.rs"]
mod tests;
