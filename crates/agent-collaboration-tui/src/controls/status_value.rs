//! Status always has a word as well as its semantic color.
use crate::{
    surface_handle::SurfaceContext,
    theme::{TextRole, Theme},
};
use iocraft::prelude::*;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusKind {
    Available,
    Warning,
    Error,
}
impl StatusKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Available => "Available",
            Self::Warning => "Warning",
            Self::Error => "Error",
        }
    }
    pub const fn role(self) -> TextRole {
        match self {
            Self::Available => TextRole::Good,
            Self::Warning => TextRole::Warning,
            Self::Error => TextRole::Error,
        }
    }
}
#[derive(Props)]
pub struct StatusValueProps {
    #[iocraft(required)]
    pub status: StatusKind,
    pub detail: Option<String>,
}
#[component]
pub fn StatusValue(props: &StatusValueProps, hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let theme = hooks
        .try_use_context::<SurfaceContext>()
        .map_or(Theme::default(), |context| context.theme());
    element!(View(max_width:100pct,column_gap:1,flex_wrap:FlexWrap::Wrap) {View(flex_shrink:0.0_f32) {Text(content:props.status.label(),wrap:TextWrap::NoWrap,color:theme.foreground(props.status.role()))} #(props.detail.as_ref().map(|detail|element!(View(max_width:100pct,flex_shrink:0.0_f32) {Text(content:detail.clone(),color:theme.foreground(TextRole::Text))})))})
}
