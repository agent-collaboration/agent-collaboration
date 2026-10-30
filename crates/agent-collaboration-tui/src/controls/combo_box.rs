//! ComboBox presentation uses the scoped control model and the shared input owner.
use super::{ComboBoxState, ScrollHandle, ScrollList, ScrollReveal};
use crate::{
    actions::Availability,
    focus_model::TargetId,
    hit_regions::{HitRegion, HitRegionKind, use_target_geometry},
    surface_handle::SurfaceContext,
    theme::{SurfaceRole, TextRole, UiGlyph},
};
use iocraft::prelude::*;
#[derive(Props)]
pub struct ComboBoxProps {
    #[iocraft(required)]
    pub id: TargetId,
    #[iocraft(required)]
    pub value: State<ComboBoxState>,
}
#[component]
pub fn ComboBox(props: &ComboBoxProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let context = *hooks.use_context::<SurfaceContext>();
    use_target_geometry(&mut hooks, context, props.id.clone(), HitRegionKind::Target);
    let theme = context.theme();
    let availability = context
        .stack
        .snapshot()
        .surface(context.surface)
        .and_then(|surface| {
            surface
                .focus
                .targets()
                .iter()
                .find(|target| target.id == props.id)
        })
        .map(|target| target.availability.get())
        .unwrap_or(Availability::Unavailable {
            reason: "Unavailable".into(),
        });
    let focused = context.is_focused(&props.id) && availability.is_available();
    let state = props.value.read();
    let label = state
        .options()
        .iter()
        .find(|option| Some(&option.key) == state.value())
        .map_or("Choose…", |option| option.label.as_str());
    element!(View(width:100pct,column_gap:1,background_color:theme.background(if focused {SurfaceRole::Selected}else if context.is_hovered(&props.id) {SurfaceRole::Raised}else{context.surface_role()})) {
        Text(content:label,color:theme.foreground(if !availability.is_available(){TextRole::Disabled}else if focused {TextRole::Focused}else{TextRole::Text}))
        Text(content:UiGlyph::Dropdown.text(),color:theme.foreground(TextRole::Muted))
        #(match availability {Availability::Available=>None,Availability::Unavailable {reason}=>Some(element!(Text(content:reason,color:theme.foreground(TextRole::Disabled))))})
    })
}
#[derive(Props)]
pub(crate) struct ComboDropdownProps {
    #[iocraft(required)]
    pub(crate) field: TargetId,
    #[iocraft(required)]
    pub(crate) value: State<ComboBoxState>,
    #[iocraft(required)]
    pub(crate) scroll: ScrollHandle,
}
#[component]
pub(crate) fn ComboDropdown(
    props: &ComboDropdownProps,
    hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let context = *hooks.use_context::<SurfaceContext>();
    let theme = context.theme();
    let state = props.value.read().clone();
    let matching = state.matching();
    element!(View(width:100pct,height:100pct,border_style:BorderStyle::Round,background_color:theme.background(SurfaceRole::Raised),border_color:theme.foreground(TextRole::Accent),flex_direction:FlexDirection::Column,overflow:Overflow::Hidden) {
        HitRegion(target:props.field.child("filter"),kind:HitRegionKind::Target) {View(width:100pct,flex_shrink:0.0_f32,column_gap:1) {
            View(flex_shrink:0.0_f32) {Text(content:"Filter:",color:theme.foreground(TextRole::Muted),wrap:TextWrap::NoWrap)}
            View(flex_grow:1.0_f32,flex_shrink:1.0_f32,min_width:0) {super::text_field::EditorLine(value:state.query().clone(),focused:true)}
        }}
        ScrollList(id:props.field.child("scroll"),keyboard_target:Some(props.field.child("filter")),scroll:props.scroll.clone(),grow:true) {
        #(matching.is_empty().then(||element!(Text(content:"No match",color:theme.foreground(TextRole::Muted)))))
        #(matching.into_iter().map(|option|{let highlighted=Some(&option.key)==state.highlight();let reason=match &option.availability {Availability::Available=>None,Availability::Unavailable {reason}=>Some(reason.clone())};element!(ScrollReveal(focused:highlighted) {HitRegion(target:props.field.child(&format!("option:{}",option.key.as_str())),kind:HitRegionKind::Target) {View(width:100pct,flex_shrink:0.0_f32,column_gap:1,background_color:theme.background(if highlighted {SurfaceRole::Selected}else{SurfaceRole::Raised})) {
            View(width:1) {Text(content:if Some(&option.key)==state.value(){UiGlyph::Current.text()}else{UiGlyph::Empty.text()},color:theme.foreground(TextRole::Muted))}
            Text(content:option.label.clone(),color:theme.foreground(if !option.availability.is_available(){TextRole::Disabled}else if highlighted {TextRole::Focused}else{TextRole::Text}))
            #(reason.map(|content|element!(Text(content,color:theme.foreground(TextRole::Disabled)))))
        }}}).into_any()}))
        }
    })
}
