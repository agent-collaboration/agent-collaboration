//! Section spacing has one owner; individual sections add no trailing gap.
use crate::{
    surface_handle::SurfaceContext,
    theme::{SurfaceRole, TextRole, Theme},
};
use iocraft::prelude::*;
#[derive(Props)]
pub struct DialogSectionsProps<'a> {
    pub children: Vec<AnyElement<'a>>,
}
#[component]
pub fn DialogSections<'a>(props: &mut DialogSectionsProps<'a>) -> impl Into<AnyElement<'a>> {
    let children = std::mem::take(&mut props.children);
    element!(View(width:100pct,flex_direction:FlexDirection::Column,flex_shrink:0.0_f32,row_gap:1) {#(children)})
}
#[derive(Props)]
pub struct DialogSectionProps<'a> {
    pub heading: String,
    pub surface: Option<SurfaceRole>,
    pub children: Vec<AnyElement<'a>>,
}
#[component]
pub fn DialogSection<'a>(
    props: &mut DialogSectionProps<'a>,
    hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    let theme = hooks
        .try_use_context::<SurfaceContext>()
        .map_or(Theme::default(), |context| context.theme());
    let children = std::mem::take(&mut props.children);
    element!(View(width:100pct,flex_direction:FlexDirection::Column,flex_shrink:0.0_f32,background_color:props.surface.map(|role|theme.background(role))) {
        #((!props.heading.is_empty()).then(||element!(Text(content:props.heading.clone(),color:theme.foreground(TextRole::Accent)))))
        #(children)
    })
}
