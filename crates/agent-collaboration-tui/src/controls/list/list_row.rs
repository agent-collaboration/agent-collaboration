//! Row identity drives pointer selection, focus paint and scroll reveal together.
use crate::controls::list::{ListRowKey, ListSelectionControl, scroll_list::use_scroll_reveal};
use crate::{
    foundation::theme::{SurfaceRole, TextRole, UiGlyph},
    surfaces::{
        hit_regions::{HitRegionKind, use_target_geometry},
        surface_handle::SurfaceContext,
    },
};
use iocraft::prelude::*;

#[derive(Props)]
pub struct ListRowProps<'a> {
    #[iocraft(required)]
    pub control: ListSelectionControl,
    #[iocraft(required)]
    pub row: ListRowKey,
    pub marker_offset: u16,
    pub children: Vec<AnyElement<'a>>,
}
#[component]
pub fn ListRow<'a>(props: &mut ListRowProps<'a>, mut hooks: Hooks) -> impl Into<AnyElement<'a>> {
    let context = *hooks.use_context::<SurfaceContext>();
    let selected = props.control.selected().as_ref() == Some(&props.row);
    let focused = selected && context.is_focused(props.control.id());
    let row_target = props.control.row_target(&props.row);
    use_target_geometry(
        &mut hooks,
        context,
        row_target.clone(),
        HitRegionKind::Target,
    );
    use_scroll_reveal(
        &mut hooks,
        selected && props.control.reveals_selection(),
        Some(row_target),
    );
    let theme = context.theme();
    let children = std::mem::take(&mut props.children);
    element!(View(width:100pct,flex_shrink:0.0_f32,column_gap:1,background_color:theme.background(if selected {SurfaceRole::Selected}else{context.surface_role()})) {
            View(width:1,flex_shrink:0.0_f32,flex_direction:FlexDirection::Column,padding_top:props.marker_offset) {
                Text(content:if focused {UiGlyph::Focus.text()}else if selected {UiGlyph::Current.text()}else{" "},color:theme.foreground(if focused {TextRole::Focused}else{TextRole::Muted}))
            }
            View(flex_grow:1.0_f32,flex_shrink:1.0_f32,min_width:0) {#(children)}
    })
}
