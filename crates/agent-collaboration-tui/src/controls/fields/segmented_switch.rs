//! One width projection drives complete switch groups and target reachability.
use crate::controls::fields::button::action_button_width;
use crate::controls::fields::{Button, button_target};
use crate::{
    actions::{ActionDef, ActionId, ActionPresentation},
    foundation::theme::TextRole,
    surfaces::{
        focus_model::{GroupId, TargetId, TargetSpec, TargetVisibility},
        surface_handle::SurfaceContext,
    },
};
use iocraft::prelude::*;
use std::sync::Arc;
#[derive(Clone, Copy, Eq, PartialEq)]
enum SegmentedPresentation {
    Full,
    Compact,
    Hidden,
}
fn segmented_presentation(
    actions: &[ActionPresentation],
    available_width: u16,
) -> SegmentedPresentation {
    if actions.is_empty() {
        return SegmentedPresentation::Hidden;
    }
    let separators = actions.len().saturating_sub(1);
    let width = |show_key| {
        actions.iter().fold(0usize, |sum, action| {
            sum.saturating_add(action_button_width(action, show_key))
        })
    };
    let available = usize::from(available_width);
    if width(true).saturating_add(separators.saturating_mul(3)) <= available {
        SegmentedPresentation::Full
    } else if width(false).saturating_add(separators) <= available {
        SegmentedPresentation::Compact
    } else {
        SegmentedPresentation::Hidden
    }
}
pub fn segmented_targets(
    actions: &[ActionDef],
    group: GroupId,
    available_width: impl Fn() -> u16 + Send + Sync + 'static,
) -> Vec<TargetSpec> {
    let available_width = Arc::new(available_width);
    let definitions = Arc::new(actions.to_vec());
    actions
        .iter()
        .map(|action| {
            let definitions = definitions.clone();
            let available_width = available_width.clone();
            button_target(TargetId::for_action(&action.id), action)
                .in_group(group.clone())
                .with_visibility(TargetVisibility::new(move || {
                    let actions = definitions
                        .iter()
                        .map(ActionDef::presentation)
                        .collect::<Vec<_>>();
                    segmented_presentation(&actions, available_width())
                        != SegmentedPresentation::Hidden
                }))
        })
        .collect()
}
#[derive(Props)]
pub struct SegmentedSwitchProps {
    pub actions: Vec<ActionId>,
    #[iocraft(required)]
    pub available_width: u16,
}
#[component]
pub fn SegmentedSwitch(
    props: &SegmentedSwitchProps,
    hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let context = *hooks.use_context::<SurfaceContext>();
    let snapshot = context.stack.snapshot();
    let Some(surface) = snapshot.surface(context.surface) else {
        return element!(View(width:0,height:0)).into_any();
    };
    let presentations = surface.actions.presentations();
    let Some(actions) = props
        .actions
        .iter()
        .map(|id| {
            presentations
                .iter()
                .find(|action| action.id == *id)
                .cloned()
        })
        .collect::<Option<Vec<_>>>()
    else {
        return element!(View(width:0,height:0)).into_any();
    };
    let presentation = segmented_presentation(&actions, props.available_width);
    if presentation == SegmentedPresentation::Hidden {
        return element!(View(width:0,height:0)).into_any();
    }
    let show_key = presentation == SegmentedPresentation::Full;
    let theme = context.theme();
    let mut children = Vec::new();
    for action in &props.actions {
        if !children.is_empty() {
            children.push(
                element!(Text(content:"|",color:theme.foreground(TextRole::Muted))).into_any(),
            );
        }
        children.push(
            element!(Button(id:TargetId::for_action(action),action:action.clone(),show_key))
                .into_any(),
        );
    }
    element!(View(column_gap:u16::from(show_key),flex_shrink:0.0_f32) {#(children)}).into_any()
}

#[cfg(test)]
#[path = "filter_switch_layout_test.rs"]
mod filter_layout_test;
#[cfg(test)]
#[path = "switch_controls_test.rs"]
mod tests;
