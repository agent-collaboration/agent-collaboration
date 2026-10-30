//! A fixed nested viewport supplies visible, partial and fully clipped public buttons.
use super::sample_model::{SampleData, action, action_id};
use agent_collaboration_tui::{
    actions::{ActionDef, Placement},
    controls::Button,
    focus_model::TargetId,
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
        action(name, title, &[], Placement::Hidden, move |()| {
            data.record(name)
        })
    })
    .collect()
}
#[component]
pub fn ClippingFixture() -> impl Into<AnyElement<'static>> {
    element!(View(width:12,height:3,flex_shrink:0.0_f32,border_style:BorderStyle::Round,overflow:Overflow::Hidden) {
        View(width:40,height:1,flex_shrink:0.0_f32,column_gap:1) {
            #( ["clip-hit","clip-partial","clip-hidden"].into_iter().map(|name|element!(Button(id:TargetId::for_action(&action_id(name)),action:action_id(name)))) )
        }
    })
}
