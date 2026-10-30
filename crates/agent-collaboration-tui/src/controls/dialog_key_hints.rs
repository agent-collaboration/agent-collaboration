//! Dialog hints are a projection of actual bound actions, never a second key map.
use crate::{
    actions::{ActionId, ActionPresentation, Availability},
    surface_handle::SurfaceContext,
    theme::TextRole,
};
use iocraft::prelude::*;
pub fn hint_pairs(actions: &[ActionPresentation], wanted: &[ActionId]) -> Vec<(String, String)> {
    wanted
        .iter()
        .filter_map(|id| actions.iter().find(|action| action.id == *id))
        .filter(|action| !action.keys.is_empty())
        .map(|action| {
            (
                action
                    .keys
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" / "),
                action.title.clone(),
            )
        })
        .collect()
}
#[derive(Props)]
pub struct DialogKeyHintsProps {
    pub actions: Vec<ActionId>,
}
#[component]
pub fn DialogKeyHints(props: &DialogKeyHintsProps, hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let context = *hooks.use_context::<SurfaceContext>();
    let theme = context.theme();
    let actions = context
        .stack
        .snapshot()
        .surface(context.surface)
        .map(|surface| surface.actions.presentations())
        .unwrap_or_default();
    let mut children = Vec::new();
    for action in props
        .actions
        .iter()
        .filter_map(|id| actions.iter().find(|action| action.id == *id))
        .filter(|action| !action.keys.is_empty())
    {
        let key = action
            .keys
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" / ");
        let title = action.title.clone();
        let reason = match &action.availability {
            Availability::Available => None,
            Availability::Unavailable { reason } => Some(reason.clone()),
        };
        let role = if reason.is_some() {
            TextRole::Disabled
        } else {
            TextRole::Muted
        };
        if !children.is_empty() {
            children.push(
                element!(Text(content:"·",color:theme.foreground(TextRole::Muted))).into_any(),
            );
        }
        children.push(element!(View(column_gap:1) {Text(content:key,color:theme.foreground(role)) Text(content:title,color:theme.foreground(role)) #(reason.map(|content|element!(Text(content,color:theme.foreground(TextRole::Disabled)))))}).into_any());
    }
    element!(View(column_gap:1,flex_shrink:0.0_f32) {#(children)})
}
