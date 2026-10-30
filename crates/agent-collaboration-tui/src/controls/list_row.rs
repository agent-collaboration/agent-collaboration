//! Row identity drives pointer selection, focus paint and scroll reveal together.
use super::{ListRowKey, ListSelectionControl, scroll_list::use_scroll_reveal};
use crate::{
    hit_regions::{HitRegionKind, use_target_geometry},
    surface_handle::SurfaceContext,
    theme::{SurfaceRole, TextRole, UiGlyph},
};
use iocraft::prelude::*;

#[derive(Props)]
pub struct ListRowProps<'a> {
    #[iocraft(required)]
    pub control: ListSelectionControl,
    #[iocraft(required)]
    pub row: ListRowKey,
    pub children: Vec<AnyElement<'a>>,
}
#[component]
pub fn ListRow<'a>(props: &mut ListRowProps<'a>, mut hooks: Hooks) -> impl Into<AnyElement<'a>> {
    let context = *hooks.use_context::<SurfaceContext>();
    let selected = props.control.selected().as_ref() == Some(&props.row);
    let focused = selected && context.is_focused(props.control.id());
    use_target_geometry(
        &mut hooks,
        context,
        props.control.row_target(&props.row),
        HitRegionKind::Target,
    );
    use_scroll_reveal(&mut hooks, selected);
    let theme = context.theme();
    let children = std::mem::take(&mut props.children);
    element!(View(width:100pct,flex_shrink:0.0_f32,column_gap:1,background_color:theme.background(if selected {SurfaceRole::Selected}else{context.surface_role()})) {
            Text(content:if focused {UiGlyph::Focus.text()}else if selected {UiGlyph::Current.text()}else{" "},color:theme.foreground(if focused {TextRole::Focused}else{TextRole::Muted}))
            View(flex_grow:1.0_f32,flex_shrink:1.0_f32,min_width:0) {#(children)}
    })
}
