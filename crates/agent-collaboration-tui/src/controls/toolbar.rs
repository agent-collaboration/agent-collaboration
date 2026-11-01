//! The same width calculation drives drawing and keyboard reachability.
use crate::controls::{
    ActionProjections,
    fields::{Button, button_target},
};
use crate::{
    actions::{ActionPresentation, ActionRegistry},
    surfaces::{
        focus_model::{TargetId, TargetSpec, TargetVisibility},
        surface_handle::SurfaceContext,
    },
};
use iocraft::prelude::*;
pub fn toolbar_actions(registry: &ActionRegistry, available: u16) -> Vec<ActionPresentation> {
    let mut remaining = usize::from(available);
    let mut actions = Vec::new();
    for action in ActionProjections::from_registry(registry).toolbar {
        let width =
            crate::controls::fields::button::action_button_width(&action, true).saturating_add(1);
        if width > remaining {
            break;
        }
        remaining -= width;
        actions.push(action);
    }
    actions
}
pub fn toolbar_target(
    action: &crate::actions::ActionDef,
    registry: ActionRegistry,
    width: impl Fn() -> u16 + Send + Sync + 'static,
) -> TargetSpec {
    let id = action.id.clone();
    button_target(TargetId::for_action(&id), action).with_visibility(TargetVisibility::new(
        move || {
            toolbar_actions(&registry, width())
                .iter()
                .any(|action| action.id == id)
        },
    ))
}
#[derive(Props)]
pub struct ToolbarProps<'a> {
    pub available_width: u16,
    pub children: Vec<AnyElement<'a>>,
}
#[component]
pub fn Toolbar<'a>(props: &mut ToolbarProps<'a>, hooks: Hooks) -> impl Into<AnyElement<'a>> {
    let context = *hooks.use_context::<SurfaceContext>();
    let actions = context
        .stack
        .snapshot()
        .surface(context.surface)
        .map(|surface| toolbar_actions(&surface.actions, props.available_width))
        .unwrap_or_default();
    let children = std::mem::take(&mut props.children);
    element!(View(width:100pct,flex_shrink:0.0_f32,column_gap:1,overflow:Overflow::Hidden) {#(children) #(actions.into_iter().map(|action|element!(Button(id:TargetId::for_action(&action.id),action:action.id))))})
}

#[cfg(test)]
#[path = "toolbar_labels_test.rs"]
mod tests;
