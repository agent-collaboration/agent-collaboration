//! The same width calculation drives drawing and keyboard reachability.
use super::{ActionProjections, Button, button_target};
use crate::{
    actions::{ActionPresentation, ActionRegistry, Availability, AvailabilityFn},
    focus_model::{TargetId, TargetSpec},
    surface_handle::SurfaceContext,
};
use iocraft::prelude::*;
use unicode_width::UnicodeWidthStr;
pub fn toolbar_actions(registry: &ActionRegistry, available: u16) -> Vec<ActionPresentation> {
    let mut remaining = usize::from(available);
    let mut actions = Vec::new();
    for action in ActionProjections::from_registry(registry).toolbar {
        let key = action.keys.first().map_or(0, |key| {
            UnicodeWidthStr::width(key.to_string().as_str()) + 1
        });
        let reason = match &action.availability {
            Availability::Available => 0,
            Availability::Unavailable { reason } => reason.width() + 1,
        };
        let checked = usize::from(action.checked == Some(true)) * 2;
        let label = action
            .toolbar_label
            .as_deref()
            .unwrap_or(action.title.as_str());
        let width = UnicodeWidthStr::width(label) + key + reason + checked + 3;
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
    let availability = action.availability.clone();
    button_target(TargetId::for_action(&id), action).with_availability(AvailabilityFn::new(
        move || {
            if !toolbar_actions(&registry, width())
                .iter()
                .any(|action| action.id == id)
            {
                Availability::Unavailable {
                    reason: "Available in Menu".to_owned(),
                }
            } else {
                availability.get()
            }
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
