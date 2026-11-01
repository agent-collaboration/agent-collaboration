//! Plain consumer Views independently clip real buttons along each axis.
use super::sample_model::{ClippingCase, SampleData, action, action_id};
use agent_collaboration_tui::{
    actions::{ActionDef, ActionTitle, Placement},
    controls::fields::Button,
    surfaces::focus_model::TargetId,
};
use iocraft::prelude::*;

pub fn clipping_actions(data: &SampleData) -> Vec<ActionDef> {
    let data = *data;
    [
        ("clip-hit", "Hit"),
        ("clip-partial", "Partial"),
        ("clip-hidden", "Hidden"),
    ]
    .into_iter()
    .map(|(name, title)| {
        let mut definition = action(name, title, &[], Placement::Hidden, move |()| {
            data.record(name)
        });
        if name == "clip-partial" {
            definition.title = ActionTitle::dynamic(move || {
                if data.model.read().clipping_case.uses_multiline_label() {
                    "Partial\nsecond".to_owned()
                } else {
                    "Partial".to_owned()
                }
            });
        }
        definition
    })
    .collect()
}
#[derive(Props)]
pub struct ClippingFixtureProps {
    #[iocraft(required)]
    pub case: ClippingCase,
    pub centered: bool,
}
#[component]
pub fn ClippingFixture(props: &ClippingFixtureProps) -> impl Into<AnyElement<'static>> {
    let vertical = props.case.is_vertical();
    let buttons = element!(View(width:40,height:if vertical {6} else if props.case.uses_multiline_label() {2} else {1},flex_shrink:0.0_f32,flex_direction:if vertical {FlexDirection::Column} else {FlexDirection::Row},align_items:AlignItems::FlexStart,column_gap:if vertical {0} else {1}) {
        #(["clip-hit","clip-partial","clip-hidden"].into_iter().map(|name|element!(Button(id:TargetId::for_action(&action_id(name)),action:action_id(name)))))
    }).into_any();
    let clipped = match props.case {
        ClippingCase::PlainHorizontal => element!(View(width:9,height:1,flex_shrink:0.0_f32,overflow:Overflow::Hidden) {#(Some(buttons))}).into_any(),
        ClippingCase::PlainVertical => element!(View(width:9,height:2,flex_shrink:0.0_f32,overflow:Overflow::Hidden) {#(Some(buttons))}).into_any(),
        ClippingCase::BorderedHorizontal => element!(View(width:12,height:3,flex_shrink:0.0_f32,border_style:BorderStyle::Round,overflow:Overflow::Hidden) {#(Some(buttons))}).into_any(),
        ClippingCase::BorderedVertical => element!(View(width:11,height:4,flex_shrink:0.0_f32,border_style:BorderStyle::Round,overflow:Overflow::Hidden) {#(Some(buttons))}).into_any(),
        ClippingCase::NestedHorizontal => element!(View(width:11,height:4,flex_shrink:0.0_f32,border_style:BorderStyle::Round,overflow:Overflow::Hidden) {View(width:12,height:1,flex_shrink:0.0_f32,overflow:Overflow::Hidden) {#(Some(buttons))}}).into_any(),
        ClippingCase::NestedVertical => element!(View(width:11,height:4,flex_shrink:0.0_f32,border_style:BorderStyle::Round,overflow:Overflow::Hidden) {View(width:6,height:4,flex_shrink:0.0_f32,overflow:Overflow::Hidden) {#(Some(buttons))}}).into_any(),
    };
    element!(View(width:100pct,flex_shrink:0.0_f32,flex_direction:FlexDirection::Column,align_items:if props.centered {AlignItems::Center}else{AlignItems::FlexStart}) {#(Some(clipped))})
}
