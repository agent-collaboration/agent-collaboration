//! A menu model is complete before open_overlay returns; paint registers no targets.
use super::{ActionProjections, ScrollHandle, ScrollList, ScrollReveal, button_target};
use crate::{
    actions::{ActionError, ActionRegistry, Availability, Placement},
    focus_model::{FocusError, FocusModel, GroupId, TargetId, TargetSpec},
    hit_regions::{HitRegion, HitRegionKind},
    surface_handle::{SurfaceContext, SurfaceStackHandle},
    surface_stack::{OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceModel},
    theme::{SurfaceRole, TextRole, UiGlyph},
};
use iocraft::prelude::*;
#[derive(Debug)]
pub enum MenuModelError {
    Actions(ActionError),
    Focus(FocusError),
    Identity(crate::actions::IdentifierError),
}
impl std::fmt::Display for MenuModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid menu model: {self:?}")
    }
}
impl std::error::Error for MenuModelError {}
pub fn menu_overlay(
    stack: SurfaceStackHandle,
    registry: &ActionRegistry,
    layout: OverlayLayout,
) -> Result<OverlaySpec, MenuModelError> {
    let actions = registry
        .definitions()
        .iter()
        .filter(|action| action.placement != Placement::Hidden)
        .cloned()
        .map(|mut action| {
            let run = action.run.clone();
            action.run = Handler::from(move |()| {
                stack.close_top();
                run(());
            });
            action
        })
        .collect::<Vec<_>>();
    let group = GroupId::new("menu-items").map_err(MenuModelError::Identity)?;
    let scroll = ScrollHandle::new(stack);
    let scroll_id = TargetId::new("menu-scroll").map_err(MenuModelError::Identity)?;
    let mut targets = actions
        .iter()
        .map(|action| {
            button_target(TargetId::for_action(&action.id), action).in_group(group.clone())
        })
        .collect::<Vec<_>>();
    targets.push(
        TargetSpec::new(scroll_id.clone())
            .pointer_only()
            .with_input(scroll.input_handler()),
    );
    Ok(OverlaySpec {
        kind: OverlayKind::Menu,
        model: SurfaceModel {
            focus: FocusModel::new(targets).map_err(MenuModelError::Focus)?,
            actions: ActionRegistry::new(registry.scope().clone(), actions)
                .map_err(MenuModelError::Actions)?,
            primary_action: None,
        },
        view: OverlayView::new(move || {
            element!(MenuDropdown(id:scroll_id.clone(),scroll:scroll.clone())).into_any()
        }),
        layout,
    })
}
#[derive(Props)]
pub struct MenuDropdownProps {
    #[iocraft(required)]
    pub id: TargetId,
    #[iocraft(required)]
    pub scroll: ScrollHandle,
}
#[component]
pub fn MenuDropdown(props: &MenuDropdownProps, hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let context = *hooks.use_context::<SurfaceContext>();
    let theme = context.theme();
    let entries = context
        .stack
        .snapshot()
        .surface(context.surface)
        .map(|surface| ActionProjections::from_registry(&surface.actions).menu)
        .unwrap_or_default();
    let keyboard_target = context
        .stack
        .snapshot()
        .surface(context.surface)
        .and_then(|surface| surface.focus.current().cloned());
    element! {View(width:100pct,height:100pct,border_style:BorderStyle::Round,border_color:theme.foreground(TextRole::Accent),background_color:theme.background(SurfaceRole::Raised),flex_direction:FlexDirection::Column,overflow:Overflow::Hidden) {
        ScrollList(id:props.id.clone(),keyboard_target,scroll:props.scroll.clone(),grow:true) {
        #(entries.into_iter().map(|action|{
            let id=TargetId::for_action(&action.id);let focused=context.is_focused(&id);let available=action.availability.is_available();
            let reason=match action.availability {Availability::Available=>None,Availability::Unavailable {reason}=>Some(reason)};
            element!(ScrollReveal(focused) {HitRegion(target:id.clone(),kind:HitRegionKind::Target) {View(width:100pct,min_height:1,flex_shrink:0.0_f32,column_gap:1,background_color:theme.background(if focused {SurfaceRole::Selected}else if context.is_hovered(&id){SurfaceRole::Alternate}else{SurfaceRole::Raised})) {
                View(width:1,flex_shrink:0.0_f32) {Text(content:if action.checked==Some(true){UiGlyph::Checked.text()}else if focused {UiGlyph::Focus.text()}else{""},color:theme.foreground(TextRole::Focused))}
                Text(content:action.title,color:theme.foreground(if !available {TextRole::Disabled}else if focused {TextRole::Focused}else{TextRole::Text}))
                #(reason.map(|content|element!(Text(content,color:theme.foreground(TextRole::Disabled)))))
                View(flex_grow:1.0_f32)
                Text(content:action.keys.iter().map(ToString::to_string).collect::<Vec<_>>().join(" / "),color:theme.foreground(TextRole::Muted))
            }}}).into_any()
        }))
        }
    }}
}
