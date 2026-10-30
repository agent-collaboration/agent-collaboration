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
}
#[component]
pub fn Button(props: &ButtonProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let context = *hooks.use_context::<SurfaceContext>();
    use_target_geometry(&mut hooks, context, props.id.clone(), HitRegionKind::Target);
    let Some(action) = presentation(context, &props.action) else {
        return element!(View).into_any();
    };
    let theme = context.theme();
    let focused = context.is_focused(&props.id);
    let primary = context
        .stack
        .snapshot()
        .surface(context.surface)
        .is_some_and(|surface| surface.primary_action.as_ref() == Some(&props.action));
    let role = if !action.availability.is_available() {
        TextRole::Disabled
    } else if focused {
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
    element! {View(padding_left:1,padding_right:1,flex_shrink:0.0_f32,column_gap:1,background_color:theme.background(if focused||action.checked==Some(true) {SurfaceRole::Selected}else if context.is_hovered(&props.id){SurfaceRole::Raised}else{SurfaceRole::Base})) {
        #((action.checked==Some(true)).then(||element!(Text(content:UiGlyph::Current.text(),color:theme.foreground(TextRole::Accent)))))
        #(props.glyph.map(|glyph|element!(Text(content:glyph.text(),color:theme.foreground(role)))))
        #(primary.then(||element!(Text(content:UiGlyph::Current.text(),color:theme.foreground(TextRole::Primary)))))
        Text(content:action.title,color:theme.foreground(role))
        #(action.keys.first().map(|key|element!(Text(content:key.to_string(),color:theme.foreground(TextRole::Muted)))))
        #(reason.map(|content|element!(Text(content,color:theme.foreground(TextRole::Disabled)))))
    }}.into_any()
}
#[component]
pub fn MenuButton(props: &ButtonProps) -> impl Into<AnyElement<'static>> {
    element!(Button(id:props.id.clone(),action:props.action.clone(),glyph:Some(UiGlyph::Menu)))
}
