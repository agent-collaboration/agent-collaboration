//! ComboBox presentation uses the scoped control model and the shared input owner.
use crate::controls::{
    combo::ComboBoxState,
    list::{ScrollHandle, ScrollList, scroll_list::TargetScrollReveal},
};
use crate::{
    actions::Availability,
    foundation::theme::{SurfaceRole, TextRole, UiGlyph},
    surfaces::{
        focus_model::TargetId,
        hit_regions::{HitRegion, HitRegionKind, use_target_geometry},
        overlay_host::SurfaceSizing,
        surface_handle::SurfaceContext,
    },
};
use iocraft::prelude::*;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
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
    let focused = context.is_focused(&props.id);
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
    let sizing = *hooks.use_context::<SurfaceSizing>();
    let row_width = usize::from(sizing.max_width.saturating_sub(3));
    let state = props.value.read().clone();
    let matching = state.matching();
    let content_rows = props.scroll.position().natural_height;
    // The filter and round border add three rows to the measured list extent;
    // the host's available height remains the cap.
    let height = if content_rows == 0 {
        Size::Auto
    } else {
        Size::Length(u32::from(
            content_rows.saturating_add(3).min(sizing.max_height),
        ))
    };
    element!(View(width:100pct,height,max_height:sizing.max_height,border_style:BorderStyle::Round,background_color:theme.background(SurfaceRole::Raised),border_color:theme.foreground(TextRole::Accent),flex_direction:FlexDirection::Column,overflow:Overflow::Hidden) {
        HitRegion(target:props.field.child("filter"),kind:HitRegionKind::Target) {View(width:100pct,flex_shrink:0.0_f32,column_gap:1) {
            View(flex_shrink:0.0_f32) {Text(content:"Filter:",color:theme.foreground(TextRole::Muted),wrap:TextWrap::NoWrap)}
            View(flex_grow:1.0_f32,flex_shrink:1.0_f32,min_width:0) {crate::controls::fields::text_field::EditorLine(value:state.query().clone(),focused:true,available_width:Some(sizing.max_width.saturating_sub(10)))}
        }}
        ScrollList(id:props.field.child("scroll"),keyboard_target:Some(props.field.child("filter")),scroll:props.scroll.clone(),grow:false) {
        #(matching.is_empty().then(||element!(Text(content:"No match",color:theme.foreground(TextRole::Muted)))))
        #(matching.into_iter().map(|option| {
            let highlighted=Some(&option.key)==state.highlight();
            let target=props.field.child(&format!("option:{}",option.key.as_str()));
            let reason=match &option.availability {Availability::Available=>None,Availability::Unavailable {reason}=>Some(reason.clone())};
            let label=fit_label_text(&option.label,row_width.saturating_sub(2));
            let inline_secondary=option.secondary.as_ref().filter(|secondary|option.label.width()+secondary.width()+3<=row_width);
            let overflow_secondary=option.secondary.as_ref().filter(|_|inline_secondary.is_none());
            element!(TargetScrollReveal(target:target.clone(),focused:highlighted) {View(width:100pct,flex_shrink:0.0_f32) {HitRegion(target,kind:HitRegionKind::Target) {View(width:100pct,flex_shrink:0.0_f32,flex_direction:FlexDirection::Column,background_color:theme.background(if highlighted {SurfaceRole::Selected}else{SurfaceRole::Raised})) {
                View(width:100pct,flex_shrink:0.0_f32,column_gap:1) {
                    View(width:1,flex_shrink:0.0_f32) {Text(content:if Some(&option.key)==state.value(){UiGlyph::Current.text()}else{UiGlyph::Empty.text()},color:theme.foreground(TextRole::Muted))}
                    View(flex_shrink:0.0_f32) {Text(content:label,wrap:TextWrap::NoWrap,color:theme.foreground(if !option.availability.is_available(){TextRole::Disabled}else if highlighted {TextRole::Focused}else{TextRole::Text}))}
                    #(inline_secondary.map(|content|element!(Text(content:content.clone(),wrap:TextWrap::NoWrap,color:theme.foreground(TextRole::Muted)))))
                }
                #(overflow_secondary.map(|content|element!(View(width:100pct,padding_left:2,flex_shrink:0.0_f32) {Text(content:content.clone(),wrap:TextWrap::Wrap,color:theme.foreground(TextRole::Muted))})))
                #(reason.map(|content|element!(View(width:100pct,padding_left:2,flex_shrink:0.0_f32) {Text(content,wrap:TextWrap::Wrap,color:theme.foreground(TextRole::Muted))})))
            }}}}).into_any()
        }))
        }
    })
}

fn fit_label_text(content: &str, width: usize) -> String {
    if content.width() <= width {
        return content.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut result = String::new();
    let mut used = 0;
    for character in content.chars() {
        let cells = character.width().unwrap_or(0);
        if used + cells >= width {
            break;
        }
        result.push(character);
        used += cells;
    }
    result.push_str(UiGlyph::Ellipsis.text());
    result
}

#[cfg(test)]
#[path = "combo_filter_layout_test.rs"]
mod filter_layout_test;
#[cfg(test)]
#[path = "combo_option_layout_test.rs"]
mod option_layout_test;
