//! A menu model is complete before open_overlay returns; paint registers no targets.
use crate::controls::{
    ActionProjections,
    fields::button_target,
    list::{ScrollHandle, ScrollList, ScrollReveal},
};
use crate::{
    actions::{ActionError, ActionRegistry, Availability, Placement},
    foundation::theme::{SurfaceRole, TextRole, UiGlyph},
    surfaces::{
        focus_model::{FocusError, FocusModel, GroupId, TargetId, TargetSpec},
        hit_regions::{HitRegion, HitRegionKind},
        overlay_host::SurfaceSizing,
        surface_handle::{SurfaceContext, SurfaceStackHandle},
        surface_stack::{OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceModel},
    },
};
use iocraft::prelude::*;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

fn menu_dimensions(registry: &ActionRegistry) -> (u16, u16) {
    let entries = ActionProjections::from_registry(registry).menu;
    let content_height = u16::try_from(entries.len())
        .unwrap_or(u16::MAX)
        .saturating_add(2);
    let preferred = entries
        .iter()
        .map(|action| {
            let key = action.keys.first().map_or(0, |key| key.to_string().width());
            let reason = match &action.availability {
                Availability::Available => 0,
                Availability::Unavailable { reason } => reason.width() + 2,
            };
            action.title.width() + key + reason + 6
        })
        .max()
        .unwrap_or(0);
    let preferred = u16::try_from(preferred).unwrap_or(u16::MAX);
    (preferred, content_height)
}

fn ellipsize(content: &str, width: usize) -> String {
    if content.width() <= width {
        return content.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut result = String::new();
    let mut used = 0;
    for character in content.chars() {
        let cells = character.width().unwrap_or(0);
        if used + cells >= width {
            break;
        }
        result.push(character);
        used += cells;
    }
    result.push_str(UiGlyph::Ellipsis.text());
    result
}
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
    anchor: TargetId,
) -> Result<OverlaySpec, MenuModelError> {
    let (preferred_width, maximum_height) = menu_dimensions(registry);
    let layout = OverlayLayout::Anchored {
        target: anchor,
        preferred_width,
        maximum_width: u16::MAX,
        maximum_height,
    };
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
    let sizing = *hooks.use_context::<SurfaceSizing>();
    let body_width = usize::from(sizing.max_width.saturating_sub(3));
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
            let key=action.keys.first().map(ToString::to_string).unwrap_or_default();
            let reason_gap=u16::from(reason.is_some());
            let remaining=body_width.saturating_sub(action.title.width()+3+usize::from(reason_gap));
            let key_width=key.width().min(if reason.is_some(){remaining.saturating_sub(2)}else{remaining});
            let gap=u16::from(reason.is_some()&&key_width>0);
            let reason=ellipsize(reason.as_deref().unwrap_or_default(),remaining.saturating_sub(key_width+usize::from(gap)));
            let key=ellipsize(&key,key_width);
            element!(ScrollReveal(focused) {HitRegion(target:id.clone(),kind:HitRegionKind::Target) {View(width:100pct,min_height:1,flex_shrink:0.0_f32,column_gap:1,background_color:theme.background(if focused {SurfaceRole::Selected}else if context.is_hovered(&id){SurfaceRole::Alternate}else{SurfaceRole::Raised})) {
                View(width:1,flex_shrink:0.0_f32) {Text(content:if action.checked==Some(true){UiGlyph::Checked.text()}else if focused {UiGlyph::Focus.text()}else{""},color:theme.foreground(TextRole::Focused))}
                View(flex_shrink:0.0_f32) {Text(content:action.title,wrap:TextWrap::NoWrap,color:theme.foreground(if !available {TextRole::Disabled}else if focused {TextRole::Focused}else{TextRole::Text}))}
                View(flex_grow:1.0_f32,flex_shrink:1.0_f32,min_width:0,column_gap:gap,padding_left:reason_gap) {
                    View(flex_grow:1.0_f32,min_width:0,justify_content:JustifyContent::End) {Text(content:reason,wrap:TextWrap::NoWrap,align:TextAlign::Right,color:theme.foreground(TextRole::Muted))}
                    View(flex_shrink:0.0_f32) {Text(content:key,wrap:TextWrap::NoWrap,color:theme.foreground(TextRole::Muted))}
                }
            }}}).into_any()
        }))
        }
    }}
}

#[cfg(test)]
#[path = "menu_layout_test.rs"]
mod tests;
