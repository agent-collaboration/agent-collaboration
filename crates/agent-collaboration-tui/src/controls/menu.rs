//! A menu model is complete before open_overlay returns; paint registers no targets.
use crate::controls::{
    ActionProjections,
    action_projections::menu_sections,
    fields::button_target,
    list::{ScrollHandle, ScrollList, scroll_list::use_scroll_reveal},
};
use crate::{
    actions::{ActionError, ActionPresentation, ActionRegistry, Availability, Placement},
    foundation::theme::{SurfaceRole, TextRole, UiGlyph, UiTextRole},
    surfaces::{
        focus_model::{FocusError, FocusModel, GroupId, TargetId, TargetSpec},
        hit_regions::{HitRegionKind, use_target_geometry},
        overlay_host::SurfaceSizing,
        surface_handle::{SurfaceContext, SurfaceStackHandle},
        surface_stack::{OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceModel},
    },
};
use iocraft::prelude::*;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

fn menu_dimensions(registry: &ActionRegistry) -> (u16, u16) {
    let sections = menu_sections(registry);
    let content_height = sections
        .iter()
        .map(|section| section.items.len() + usize::from(section.heading.is_some()))
        .sum::<usize>()
        .saturating_add(sections.len().saturating_sub(1))
        .saturating_add(2);
    let content_height = u16::try_from(content_height).unwrap_or(u16::MAX);
    let preferred = sections
        .iter()
        .flat_map(|section| section.heading.iter().chain(section.items.iter()))
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
    let heading_ids = registry
        .menu_sections()
        .iter()
        .filter_map(|section| section.heading_action.as_ref())
        .collect::<std::collections::HashSet<_>>();
    let actions = registry
        .definitions()
        .iter()
        .filter(|action| action.placement != Placement::Hidden || heading_ids.contains(&action.id))
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
    let overlay_registry = ActionRegistry::new(registry.scope().clone(), actions)
        .map_err(MenuModelError::Actions)?
        .with_menu_sections(registry.menu_sections().to_vec())
        .map_err(MenuModelError::Actions)?;
    let projection = ActionProjections::from_registry(&overlay_registry);
    let mut targets = projection
        .menu
        .iter()
        .filter_map(|item| {
            overlay_registry
                .definitions()
                .iter()
                .find(|action| action.id == item.id)
        })
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
            actions: overlay_registry,
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
    let sections = context
        .stack
        .snapshot()
        .surface(context.surface)
        .map(|surface| menu_sections(&surface.actions))
        .unwrap_or_default();
    let keyboard_target = context
        .stack
        .snapshot()
        .surface(context.surface)
        .and_then(|surface| surface.focus.current().cloned());
    let mut rows = Vec::new();
    for (index, section) in sections.into_iter().enumerate() {
        if index > 0 {
            rows.push(element!(View(width:100pct,height:1,flex_shrink:0.0_f32,border_edges:Edges::Top,border_style:BorderStyle::Single,border_color:theme.foreground_color(UiTextRole::Rule.color()))).into_any());
        }
        if let Some(heading) = section.heading {
            let key = heading.keys.first().map(ToString::to_string);
            rows.push(element!(crate::controls::typography::GroupHeading(name:heading.title,count:None,summary:key)).into_any());
        }
        for action in section.items {
            let key = action.id.as_str().to_owned();
            rows.push(element!(MenuItemRow(key, action, body_width)).into_any());
        }
    }
    element!(View(width:100pct,height:100pct,border_style:BorderStyle::Round,border_color:theme.foreground(TextRole::Accent),background_color:theme.background(SurfaceRole::Raised),flex_direction:FlexDirection::Column,overflow:Overflow::Hidden) {
        ScrollList(id:props.id.clone(),keyboard_target,scroll:props.scroll.clone(),grow:true) {#(rows)}
    })
}

#[derive(Props)]
struct MenuItemRowProps {
    #[iocraft(required)]
    action: ActionPresentation,
    body_width: usize,
}
#[component]
fn MenuItemRow(props: &MenuItemRowProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let context = *hooks.use_context::<SurfaceContext>();
    let theme = context.theme();
    let action = props.action.clone();
    let body_width = props.body_width;

    let id = TargetId::for_action(&action.id);
    let focused = context.is_focused(&id);
    use_target_geometry(&mut hooks, context, id.clone(), HitRegionKind::Target);
    use_scroll_reveal(&mut hooks, focused, Some(id.clone()));
    let available = action.availability.is_available();
    let reason = match action.availability {
        Availability::Available => None,
        Availability::Unavailable { reason } => Some(reason),
    };
    let key = action
        .keys
        .first()
        .map(ToString::to_string)
        .unwrap_or_default();
    let reason_gap = u16::from(reason.is_some());
    let remaining = body_width.saturating_sub(action.title.width() + 3 + usize::from(reason_gap));
    let key_width = key.width().min(if reason.is_some() {
        remaining.saturating_sub(2)
    } else {
        remaining
    });
    let gap = u16::from(reason.is_some() && key_width > 0);
    let reason = ellipsize(
        reason.as_deref().unwrap_or_default(),
        remaining.saturating_sub(key_width + usize::from(gap)),
    );
    let key = ellipsize(&key, key_width);
    element!(View(width:100pct,min_height:1,flex_shrink:0.0_f32,column_gap:1,background_color:theme.background(if focused{SurfaceRole::Selected}else if context.is_hovered(&id){SurfaceRole::Alternate}else{SurfaceRole::Raised})) {
        View(width:1,flex_shrink:0.0_f32) {Text(content:if action.checked==Some(true){UiGlyph::Checked.text()}else if focused{UiGlyph::Focus.text()}else{""},color:theme.foreground(TextRole::Focused))}
        View(flex_shrink:0.0_f32) {Text(content:action.title,wrap:TextWrap::NoWrap,color:theme.foreground(if !available{TextRole::Disabled}else if focused{TextRole::Focused}else{TextRole::Text}))}
        View(flex_grow:1.0_f32,flex_shrink:1.0_f32,min_width:0,column_gap:gap,padding_left:reason_gap) {
            View(flex_grow:1.0_f32,min_width:0,justify_content:JustifyContent::End) {Text(content:reason,wrap:TextWrap::NoWrap,align:TextAlign::Right,color:theme.foreground(TextRole::Muted))}
            View(flex_shrink:0.0_f32) {Text(content:key,wrap:TextWrap::NoWrap,color:theme.foreground(TextRole::Muted))}
        }
    })
}

#[cfg(test)]
#[path = "menu_sections_test.rs"]
mod sections_test;
#[cfg(test)]
#[path = "menu_layout_test.rs"]
mod tests;
