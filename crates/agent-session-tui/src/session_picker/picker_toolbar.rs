//! Toolbar groups wrap without clipping names, counts or focusable controls.
use super::{ActionId, PickerSurfaceContext, SessionsPickerModel, SurfaceResult, TargetId, target};
use agent_collaboration_tui::{
    controls::fields::{Button, FilterField, MenuButton, SegmentedSwitch},
    foundation::theme::TextRole,
    surfaces::surface_handle::SurfaceContext,
};
use agent_session_inventory::picker_data_query::SessionsPickerRoot;
use iocraft::prelude::*;
use unicode_width::UnicodeWidthStr;

const FILTER_OUTER_MINIMUM: u16 = 14;
const TOOLBAR_ROW_GAP: u16 = 1;

// Owner-decided scope projection (2026-10-31): This folder | Repo | All.
pub(super) const SCOPE_SETTINGS: &[(&str, &str, SessionsPickerRoot)] = &[
    ("scope-folder", "This folder", SessionsPickerRoot::Cwd),
    ("scope-repo", "Repo", SessionsPickerRoot::Repo),
    ("scope-all", "All", SessionsPickerRoot::Any),
];

pub(super) struct PickerToolbarLayout {
    pub scope_width: u16,
    pub show_status: bool,
}

/// Reserve the fixed buttons and twelve editor cells before projecting scope.
pub(super) fn toolbar_layout(model: &SessionsPickerModel, width: u16) -> PickerToolbarLayout {
    let row_width = width.saturating_sub(4); // Outer border and horizontal padding.
    // Button padding, the Menu glyph, and the key hints match the controls below.
    let buttons_width = match width {
        0..40 => 12,
        40..48 => 12 + TOOLBAR_ROW_GAP + 7,
        _ => 12 + TOOLBAR_ROW_GAP + 14,
    };
    let minimum_row = buttons_width + FILTER_OUTER_MINIMUM + TOOLBAR_ROW_GAP * 2;
    let count = format!(
        "{}/{} sessions",
        model.visible_record_len(),
        model.request.records.len()
    );
    let machine = model.machine_controls.label(model.request.machine_mode);
    let status_width = u16::try_from(
        count
            .width()
            .saturating_add(machine.width())
            .saturating_add(3),
    )
    .unwrap_or(u16::MAX);
    let show_status = width >= 76
        && row_width
            >= minimum_row
                .saturating_add(status_width)
                .saturating_add(TOOLBAR_ROW_GAP);
    let occupied = if show_status {
        minimum_row
            .saturating_add(status_width)
            .saturating_add(TOOLBAR_ROW_GAP)
    } else {
        minimum_row
    };
    PickerToolbarLayout {
        scope_width: row_width.saturating_sub(occupied.saturating_add(TOOLBAR_ROW_GAP)),
        show_status,
    }
}

pub(super) fn render_toolbar(
    context: &PickerSurfaceContext,
    surface: SurfaceContext,
) -> SurfaceResult<AnyElement<'static>> {
    let model = context.model.read();
    let theme = surface.theme();
    let width = surface.stack.viewport().0;
    let layout = toolbar_layout(&model, width);
    let action_button = |name: &str, menu: bool| -> SurfaceResult<AnyElement<'static>> {
        let action = ActionId::new(name)?;
        let id = TargetId::for_action(&action);
        Ok(if menu {
            element!(MenuButton(id, action)).into_any()
        } else {
            element!(Button(id, action,show_key:Some(width>=48))).into_any()
        })
    };
    let buttons = element!(View(min_width:0,column_gap:1,flex_shrink:0.0_f32) {
        #(Some(action_button("menu",true)?))
        #((width>=40).then(||action_button("new",false)).transpose()?)
    })
    .into_any();
    let filter = element!(View(min_width:FILTER_OUTER_MINIMUM,flex_grow:1.0_f32,flex_shrink:1.0_f32,max_width:28) {
        FilterField(id:target("session-filter")?,value:context.filter,placeholder:"Filter…".to_owned())
    }).into_any();
    let scope_actions = SCOPE_SETTINGS
        .iter()
        .map(|(name, _, _)| ActionId::new(*name))
        .collect::<Result<Vec<_>, _>>()?;
    let scope = (layout.scope_width > 0).then(|| {
        element!(View(flex_shrink:0.0_f32) {SegmentedSwitch(actions:scope_actions,available_width:layout.scope_width)}).into_any()
    });
    let status = layout.show_status.then(||element!(View(min_width:0,column_gap:1,flex_shrink:0.0_f32,overflow:Overflow::Hidden) {
        View(flex_shrink:0.0_f32) {Text(content:format!("{}/{} sessions",model.visible_record_len(),model.request.records.len()),color:theme.foreground(TextRole::Accent),weight:Weight::Bold,wrap:TextWrap::NoWrap)}
        Text(content:"·",color:theme.foreground(TextRole::Muted),wrap:TextWrap::NoWrap)
        Text(content:model.machine_controls.label(model.request.machine_mode),color:theme.foreground(TextRole::Muted),wrap:TextWrap::NoWrap)
    }).into_any());
    Ok(
        element!(View(width:100pct,column_gap:TOOLBAR_ROW_GAP,flex_shrink:0.0_f32) {
            #(Some(buttons)) #(Some(filter)) #(scope)
            View(flex_grow:1.0_f32,min_width:0)
            #(status)
        })
        .into_any(),
    )
}
