//! Buttons project an action; labels, bindings and availability have one owner.
use crate::{
    actions::{ActionDef, ActionId, ActionPresentation, Availability},
    focus_model::{TargetId, TargetSpec},
    hit_regions::{HitRegionKind, use_target_geometry},
    surface_handle::SurfaceContext,
    theme::{SurfaceRole, TextRole, UiGlyph},
};
use iocraft::prelude::*;
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
    let theme = context.theme();
    let available = action.availability.is_available();
    let focused = available && context.is_focused(&props.id);
    let checked = action.checked == Some(true);
    let label_emphasized = available && (focused || checked);
    let primary = context
        .stack
        .snapshot()
        .surface(context.surface)
        .is_some_and(|surface| surface.primary_action.as_ref() == Some(&props.action));
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
    element! {View(padding_left:1,padding_right:1,flex_shrink:0.0_f32,column_gap:1,background_color:theme.background(if context.is_hovered(&props.id){SurfaceRole::Raised}else{SurfaceRole::Base})) {
        #((action.checked==Some(true)).then(||element!(Text(content:UiGlyph::Current.text(),color:theme.foreground(TextRole::Accent)))))
        #(props.glyph.map(|glyph|element!(Text(content:glyph.text(),color:theme.foreground(role)))))
        #(primary.then(||element!(Text(content:UiGlyph::Current.text(),color:theme.foreground(TextRole::Primary)))))
        Text(content:action.toolbar_label.unwrap_or(action.title),color:theme.foreground(role),weight:if label_emphasized {Weight::Bold}else{Weight::Normal},decoration:if focused {TextDecoration::Underline}else{TextDecoration::None})
        #(action.keys.first().filter(|_|props.show_key.unwrap_or(true)).map(|key|element!(Text(content:key.to_string(),color:theme.foreground(TextRole::Muted)))))
        #(reason.map(|content|element!(Text(content,color:theme.foreground(TextRole::Disabled)))))
    }}.into_any()
}
#[component]
pub fn MenuButton(props: &ButtonProps) -> impl Into<AnyElement<'static>> {
    element!(Button(id:props.id.clone(),action:props.action.clone(),glyph:Some(UiGlyph::Menu),show_key:props.show_key))
}
