//! Help entries are a fresh projection of the source scope, including unavailable actions.
use super::{Button, DialogFrame, HelpControl};
use crate::{
    actions::Availability,
    focus_model::TargetId,
    hit_regions::{HitRegion, HitRegionKind},
    surface_handle::SurfaceContext,
    theme::TextRole,
};
use iocraft::prelude::*;
use unicode_width::UnicodeWidthStr;

#[derive(Props)]
pub struct HelpDialogProps {
    #[iocraft(required)]
    pub control: HelpControl,
}
#[component]
pub fn HelpDialog(props: &HelpDialogProps, hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let context = *hooks.use_context::<SurfaceContext>();
    let theme = context.theme();
    let entries = props
        .control
        .source(context.surface)
        .map(|source| source.presentations())
        .unwrap_or_default();
    let entries = entries
        .into_iter()
        .map(|action| {
            let keys = action
                .keys
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            (action, keys)
        })
        .collect::<Vec<_>>();
    let key_width = u32::try_from(
        entries
            .iter()
            .map(|(_, keys)| keys.width())
            .max()
            .unwrap_or(0)
            .max(1),
    )
    .unwrap_or(u32::MAX);
    element!(HitRegion(target:props.control.body_target(),kind:HitRegionKind::Target) {View(width:100pct,flex_direction:FlexDirection::Column,flex_shrink:0.0_f32) {
        #(entries.into_iter().map(|(action,keys)| {let available=action.availability.is_available();let reason=match action.availability {Availability::Available=>None,Availability::Unavailable {reason}=>Some(reason)};element!(View(width:100pct,column_gap:2,flex_shrink:0.0_f32) {
            View(width:key_width,flex_shrink:0.0_f32) {Text(content:if keys.is_empty(){"—".to_owned()}else{keys},color:theme.foreground(TextRole::Muted))}
            Text(content:action.title,color:theme.foreground(if available {TextRole::Text}else{TextRole::Disabled}))
            #(reason.map(|content|element!(Text(content,color:theme.foreground(TextRole::Disabled)))))
        }).into_any()}))
    }})
}

#[derive(Props)]
pub(crate) struct HelpFrameProps {
    #[iocraft(required)]
    pub(crate) control: HelpControl,
}
#[component]
pub(crate) fn HelpFrame(props: &HelpFrameProps, hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let context = *hooks.use_context::<SurfaceContext>();
    let Some(scroll) = props.control.scroll(context.surface) else {
        return element!(View).into_any();
    };
    element!(DialogFrame(title:"Help".to_owned(),scroll,scroll_target:props.control.scroll_target(),scroll_focus:Some(props.control.body_target()),help_action:Some(element!(Button(id:TargetId::for_action(props.control.action()),action:props.control.action().clone())).into_any())) {
        HelpDialog(control:props.control.clone())
    }).into_any()
}
