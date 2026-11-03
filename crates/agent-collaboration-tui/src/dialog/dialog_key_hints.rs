//! Dialog hints are a projection of actual bound actions, never a second key map.
use super::pinned_context::fit_pinned_text;
use crate::{
    actions::{ActionId, ActionPresentation, Availability},
    foundation::theme::TextRole,
    surfaces::surface_handle::SurfaceContext,
};
use iocraft::{ComponentDrawer, Hook, prelude::*};
use unicode_width::UnicodeWidthStr;

struct HintWidth(State<u16>);
impl Hook for HintWidth {
    fn pre_component_draw(&mut self, drawer: &mut ComponentDrawer) {
        if self.0.get() != drawer.size().width {
            self.0.set(drawer.size().width);
        }
    }
}
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
pub fn DialogKeyHints(
    props: &DialogKeyHintsProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let context = *hooks.use_context::<SurfaceContext>();
    let width = hooks.use_state(|| 0u16);
    hooks.use_hook(|| HintWidth(width)).0 = width;
    let budget = if width.get() == 0 {
        usize::MAX
    } else {
        usize::from(width.get())
    };
    let theme = context.theme();
    let actions = context
        .stack
        .snapshot()
        .surface(context.surface)
        .map(|surface| surface.actions.presentations())
        .unwrap_or_default();
    let mut rows = Vec::new();
    let mut children = Vec::new();
    let mut used = 0usize;
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
        let reason = match &action.availability {
            Availability::Available => None,
            Availability::Unavailable { reason } => Some(reason.clone()),
        };
        let role = if reason.is_some() {
            TextRole::Disabled
        } else {
            TextRole::Muted
        };
        let label = match &reason {
            Some(reason) => format!("{} {reason}", action.title),
            None => action.title.clone(),
        };
        let title_budget = budget.saturating_sub(key.width() + 1);
        let (title, reason) = if label.width() <= title_budget {
            (action.title.clone(), reason)
        } else {
            (fit_pinned_text(&label, title_budget), None)
        };
        let group_width = key.width()
            + 1
            + title.width()
            + reason.as_ref().map_or(0, |reason| reason.width() + 1);
        if !children.is_empty() && used.saturating_add(3 + group_width) > budget {
            rows.push(
                element!(View(column_gap:1,flex_shrink:0.0_f32) {#(std::mem::take(&mut children))})
                    .into_any(),
            );
            used = 0;
        }
        if !children.is_empty() {
            children.push(
                element!(Text(content:"·",color:theme.foreground(TextRole::Muted))).into_any(),
            );
            used += 3;
        }
        used += group_width;
        children.push(element!(View(column_gap:1,flex_shrink:0.0_f32) {Text(content:key,wrap:TextWrap::NoWrap,color:theme.foreground(role)) Text(content:title,wrap:TextWrap::NoWrap,color:theme.foreground(role)) #(reason.map(|content|element!(Text(content,wrap:TextWrap::NoWrap,color:theme.foreground(TextRole::Disabled)))))}).into_any());
    }
    if !children.is_empty() {
        rows.push(element!(View(column_gap:1,flex_shrink:0.0_f32) {#(children)}).into_any());
    }
    element!(View(width:100pct,min_width:0,flex_direction:FlexDirection::Column,flex_shrink:0.0_f32) {#(rows)})
}

#[cfg(test)]
#[path = "dialog_key_hints_test.rs"]
mod tests;
