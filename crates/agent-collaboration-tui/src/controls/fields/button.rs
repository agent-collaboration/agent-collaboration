//! Buttons project an action; labels, bindings and availability have one owner.
use crate::{
    actions::{ActionDef, ActionId, ActionPresentation, Availability},
    foundation::theme::{SurfaceRole, TextRole, UiGlyph},
    surfaces::{
        focus_model::{TargetId, TargetSpec},
        hit_regions::{HitRegionKind, use_target_geometry},
        surface_handle::SurfaceContext,
    },
};
use iocraft::{ComponentDrawer, Hook, prelude::*};
use unicode_width::UnicodeWidthStr;
struct ButtonWidth {
    measured: State<(usize, u16)>,
    requested: usize,
}
impl Hook for ButtonWidth {
    fn pre_component_draw(&mut self, drawer: &mut ComponentDrawer) {
        let measurement = (self.requested, drawer.size().width);
        if self.measured.get() != measurement {
            self.measured.set(measurement);
        }
    }
}
/// Width of a registry-driven button, including its native padding and gaps.
pub(crate) fn action_button_width(action: &ActionPresentation, show_key: bool) -> usize {
    let label = action
        .toolbar_label
        .as_deref()
        .unwrap_or(&action.title)
        .width();
    let key = if show_key {
        action
            .keys
            .first()
            .map_or(0, |key| key.to_string().width() + 1)
    } else {
        0
    };
    let reason = match &action.availability {
        Availability::Available => 0,
        Availability::Unavailable { reason } => reason.width() + 1,
    };
    label
        .saturating_add(key)
        .saturating_add(reason)
        .saturating_add(usize::from(action.checked == Some(true)) * 2)
        .saturating_add(2)
}
pub fn button_target(id: TargetId, action: &ActionDef) -> TargetSpec {
    TargetSpec::new(id)
        .with_activation(action.id.clone())
        .with_availability(action.availability.clone())
}
pub(crate) fn presentation(context: SurfaceContext, id: &ActionId) -> Option<ActionPresentation> {
    context
        .stack
        .snapshot()
        .surface(context.surface)?
        .actions
        .presentations()
        .into_iter()
        .find(|action| action.id == *id)
}
#[derive(Props)]
pub struct ButtonProps {
    #[iocraft(required)]
    pub id: TargetId,
    #[iocraft(required)]
    pub action: ActionId,
    pub glyph: Option<UiGlyph>,
    /// Hides only this button's printed key hint; unset keeps the default hint.
    pub show_key: Option<bool>,
}
#[component]
pub fn Button(props: &ButtonProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let context = *hooks.use_context::<SurfaceContext>();
    use_target_geometry(&mut hooks, context, props.id.clone(), HitRegionKind::Target);
    let Some(action) = presentation(context, &props.action) else {
        return element!(View).into_any();
    };
    let width = hooks.use_state(|| (0usize, 0u16));
    let theme = context.theme();
    let available = action.availability.is_available();
    let focused = context.is_focused(&props.id);
    let checked = action.checked == Some(true);
    let label_emphasized = available && (focused || checked);
    let primary = context
        .stack
        .snapshot()
        .surface(context.surface)
        .is_some_and(|surface| surface.primary_action.as_ref() == Some(&props.action));
    let natural_width = action_button_width(&action, props.show_key.unwrap_or(true))
        + props.glyph.map_or(0, |glyph| glyph.text().width() + 1)
        + usize::from(primary) * 2;
    let measurement = hooks.use_hook(|| ButtonWidth {
        measured: width,
        requested: natural_width,
    });
    measurement.measured = width;
    measurement.requested = natural_width;
    let (measured_request, available_width) = width.get();
    let stacked = !available
        && measured_request == natural_width
        && available_width > 0
        && natural_width > usize::from(available_width);
    let role = if !available {
        TextRole::Disabled
    } else if focused || checked {
        TextRole::Focused
    } else if primary {
        TextRole::Primary
    } else {
        TextRole::Text
    };
    let reason = match action.availability {
        Availability::Available => None,
        Availability::Unavailable { reason } => Some(reason),
    };
    element! {View(padding_left:1,padding_right:1,width:if stacked {Size::Percent(100.0)}else{Size::Auto},max_width:100pct,min_width:0,flex_shrink:0.0_f32,column_gap:1,flex_direction:if stacked {FlexDirection::Column}else{FlexDirection::Row},background_color:theme.background(if context.is_hovered(&props.id){SurfaceRole::Raised}else{SurfaceRole::Base})) {
        View(flex_shrink:0.0_f32,column_gap:1) {
        #((action.checked==Some(true)).then(||element!(Text(content:UiGlyph::Current.text(),color:theme.foreground(TextRole::Accent)))))
        #(props.glyph.map(|glyph|element!(Text(content:glyph.text(),color:theme.foreground(role)))))
        #(primary.then(||element!(Text(content:UiGlyph::Current.text(),color:theme.foreground(TextRole::Primary)))))
        Text(content:action.toolbar_label.unwrap_or(action.title),color:theme.foreground(role),weight:if label_emphasized {Weight::Bold}else{Weight::Normal},decoration:if focused {TextDecoration::Underline}else{TextDecoration::None})
        #(action.keys.first().filter(|_|props.show_key.unwrap_or(true)).map(|key|element!(Text(content:key.to_string(),color:theme.foreground(TextRole::Muted)))))
        }
        #(reason.map(|content|if stacked {element!(super::wrapped_reason::WrappedReason(content,color:theme.foreground(TextRole::Disabled))).into_any()}else{element!(Text(content,color:theme.foreground(TextRole::Disabled))).into_any()}))
    }}.into_any()
}
#[component]
pub fn MenuButton(props: &ButtonProps) -> impl Into<AnyElement<'static>> {
    element!(Button(id:props.id.clone(),action:props.action.clone(),glyph:Some(UiGlyph::Menu),show_key:props.show_key))
}

#[cfg(test)]
#[path = "button_focus_style_test.rs"]
mod focus_style_test;
#[cfg(test)]
#[path = "button_key_hint_test.rs"]
mod key_hint_test;
#[cfg(test)]
#[path = "button_reason_layout_test.rs"]
mod reason_layout_test;
