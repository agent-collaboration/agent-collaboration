//! The account grid keeps its four groups; hit regions and scroll come from shared controls.
use super::quota_control_context::{QuotaControlContext, QuotaRefreshState, action_id};
use agent_collaboration_tui::{
    UiText, UiTextRole,
    controls::{ListRow, ListSelectionControl, MenuButton, ScrollList, Toolbar},
    focus_model::TargetId,
    surface_handle::SurfaceContext,
    theme::{SurfaceRole, TextRole},
};
use iocraft::prelude::*;

#[derive(Props)]
pub(crate) struct QuotaControlBrowseProps {
    #[iocraft(required)]
    pub(crate) context: QuotaControlContext,
    #[iocraft(required)]
    pub(crate) list: ListSelectionControl,
}

#[component]
pub(crate) fn QuotaControlBrowse(
    props: &QuotaControlBrowseProps,
    hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    #[cfg(test)]
    let mut hooks = hooks;
    let surface = *hooks.use_context::<SurfaceContext>();
    let theme = surface.theme();
    let context = &props.context;
    let width = context.width.get();
    let height = context.height.get();
    #[cfg(test)]
    super::quota_viewport_trace::use_viewport_trace(
        &mut hooks,
        context.viewport_trace.clone(),
        [
            u16::try_from(width).unwrap_or(u16::MAX),
            u16::try_from(height).unwrap_or(u16::MAX),
        ],
        [width, height],
    );
    let compact = width < 48;
    let padding = usize::from(!compact);
    let content_width = width.saturating_sub(2 + padding * 2);
    let sidecar = width >= super::quota_view_configuration::SIDECAR_QUOTA_WIDTH;
    let list_width = if sidecar {
        (content_width.saturating_sub(2) * 3 / 5)
            .max(58)
            .min(content_width.saturating_sub(44))
    } else {
        content_width
    };
    let details_width = content_width.saturating_sub(list_width + 2).max(34);
    let row_width = list_width.saturating_sub(if compact { 1 } else { 5 });
    let model = context.view_model.read();
    let rows = context.filters.read().committed.visible_rows(&model.rows);
    let selected = context.focused_account_id.read().clone();
    let selected_details = selected
        .as_ref()
        .and_then(|id| model.rows.iter().find(|row| &row.account_id == id))
        .map(|row| &row.details);
    let menu = match action_id("menu") {
        Ok(menu) => menu,
        Err(error) => return element!(UiText(content:error,role:UiTextRole::Error)).into_any(),
    };
    let scroll_id = props.list.scroll_target();
    let toolbar = surface
        .stack
        .snapshot()
        .surface(surface.surface)
        .map(|surface| super::quota_toolbar_layout::toolbar_layout(context, &surface.actions))
        .unwrap_or_default();
    let counts = toolbar.caption;
    let keyboard_target = Some(props.list.id().clone());
    let rendered_rows = rows.iter().enumerate().filter_map(|(index,row)| {
        let key=super::quota_list_controls::account_row_key(&row.account_id).ok()?;
        let focused=props.list.selected().as_ref()==Some(&key) && surface.is_focused(props.list.id());
        Some(element!(View(flex_direction:FlexDirection::Column,flex_shrink:0.0_f32,margin_top:if index==0 {0}else{2}) {
            ListRow(control:props.list.clone(),row:key,marker_offset:if super::quota_account_groups::group_widths(row_width.saturating_sub(2)).is_some(){0u16}else{2u16}) {
                #(super::quota_account_groups::render_account_groups(row,row_width.saturating_sub(2),focused))
            }
        }).into_any())
    }).collect::<Vec<_>>();
    let refresh = match &*context.refresh.read() {
        QuotaRefreshState::Refreshing => Some("Refreshing…".to_owned()),
        _ => None,
    };
    let status = refresh.or_else(|| context.notice.read().clone());
    let selected_notice = selected
        .as_ref()
        .filter(|id| !rows.iter().any(|row| &row.account_id == *id))
        .map(|_| "Selected account is no longer listed");
    element!(View(width:u32::try_from(width).unwrap_or(u32::MAX),height:u32::try_from(height).unwrap_or(u32::MAX),flex_direction:FlexDirection::Column,border_style:BorderStyle::Round,border_color:theme.foreground(TextRole::Muted),background_color:theme.background(SurfaceRole::Base),padding_left:u16::try_from(padding).unwrap_or(0),padding_right:u16::try_from(padding).unwrap_or(0),overflow:Overflow::Hidden) {
        View(width:100pct,flex_shrink:0.0_f32,column_gap:1) {
            View(flex_grow:1.0_f32,flex_shrink:1.0_f32,min_width:0) {Toolbar(available_width:toolbar.action_width) {MenuButton(id:TargetId::for_action(&menu),action:menu)}}
            #((!counts.is_empty()).then(||element!(UiText(content:counts,role:UiTextRole::Secondary))))
        }
        #(super::quota_help_rendering::render_quota_route(&model,content_width))
        #(status.map(|content|element!(UiText(content,role:UiTextRole::Secondary,wrap:TextWrap::Wrap))))
        #(selected_notice.map(|content|element!(UiText(content,role:UiTextRole::Warning,wrap:TextWrap::Wrap))))
        View(width:100pct,min_height:0,flex_grow:1.0_f32,flex_shrink:1.0_f32,align_items:AlignItems::Stretch) {
        View(width:u32::try_from(list_width).unwrap_or(u32::MAX),min_height:0,max_height:100pct,flex_grow:0.0_f32,flex_shrink:1.0_f32,flex_direction:FlexDirection::Column,border_style:if compact {BorderStyle::None}else{BorderStyle::Single},border_color:theme.foreground(TextRole::Muted),padding_left:if compact{0}else{1},padding_right:if compact{0}else{1},overflow:Overflow::Hidden) {
            #(super::quota_account_groups::render_group_headers(row_width.saturating_sub(2)).map(|headers|element!(View(padding_left:2,flex_shrink:0.0_f32){#(headers)})))
            ScrollList(id:scroll_id,scroll:props.list.scroll(),keyboard_target,grow:true) {
                #(rows.is_empty().then(||element!(UiText(content:"No accounts match these filters",role:UiTextRole::Secondary,wrap:TextWrap::Wrap))))
                #(rendered_rows)
            }
        }
        #(sidecar.then(||element!(View(width:2,flex_shrink:0.0_f32))))
        #(sidecar.then(||{
            if context.account_options.read().is_some()||context.weekly_floor_editor.read().is_some()||context.reset_target.read().is_some() {
                element!(View(width:u32::try_from(details_width).unwrap_or(u32::MAX))).into_any()
            }else{super::quota_browse_rendering::render_selected_panel(selected_details,details_width,height.saturating_sub(4),true)}
        }))
        }
    }).into_any()
}
