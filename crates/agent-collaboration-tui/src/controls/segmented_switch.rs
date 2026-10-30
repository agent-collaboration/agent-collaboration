//! The same checked actions drive the switch, menu marks and keyboard effects.
use super::{Button, button_target};
use crate::{
    actions::{ActionDef, ActionId},
    focus_model::{GroupId, TargetId, TargetSpec},
    surface_handle::SurfaceContext,
    theme::TextRole,
};
use iocraft::prelude::*;
pub fn segmented_targets(actions: &[ActionDef], group: GroupId) -> Vec<TargetSpec> {
    actions
        .iter()
        .map(|action| {
            button_target(TargetId::for_action(&action.id), action).in_group(group.clone())
        })
        .collect()
}
#[derive(Props)]
pub struct SegmentedSwitchProps {
    pub actions: Vec<ActionId>,
}
#[component]
pub fn SegmentedSwitch(
    props: &SegmentedSwitchProps,
    hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let context = *hooks.use_context::<SurfaceContext>();
    let theme = context.theme();
    let mut children = Vec::new();
    for action in &props.actions {
        if !children.is_empty() {
            children.push(
                element!(Text(content:"|",color:theme.foreground(TextRole::Muted))).into_any(),
            );
        }
        children.push(
            element!(Button(id:TargetId::for_action(action),action:action.clone())).into_any(),
        );
    }
    element!(View(column_gap:1,flex_shrink:0.0_f32) {#(children)})
}
