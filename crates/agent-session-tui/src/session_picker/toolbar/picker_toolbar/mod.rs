//! One toolbar projection preserves the count, filter and focus eligibility.
use crate::session_picker::screen::picker_surface::ActionId;
use crate::session_picker::screen::picker_surface::PickerSurfaceContext;
use crate::session_picker::screen::picker_surface::SessionsPickerModel;
use crate::session_picker::screen::picker_surface::SurfaceResult;
use crate::session_picker::screen::picker_surface::TargetId;
use crate::session_picker::screen::picker_surface::target;
use agent_collaboration_tui::controls::fields::Button;
use agent_collaboration_tui::controls::fields::FilterField;
use agent_collaboration_tui::controls::fields::MenuButton;
use agent_collaboration_tui::controls::fields::SegmentedSwitch;
use agent_collaboration_tui::foundation::theme::TextRole;
use agent_collaboration_tui::surfaces::surface_handle::SurfaceContext;
use agent_session_inventory::picker_data_query::SessionsPickerRoot;
use iocraft::prelude::*;
use unicode_width::UnicodeWidthStr;

const FILTER_OUTER_MINIMUM: u16 = 14;
const TOOLBAR_ROW_GAP: u16 = 1;
const MENU_WIDTH: u16 = 12;
const NEW_WIDTH: u16 = 7;
const NEW_WITH_KEY_WIDTH: u16 = 14;

// Owner-decided scope projection (2026-10-31): This folder | Repo | All.
pub(crate) const SCOPE_SETTINGS: &[(&str, &str, SessionsPickerRoot)] = &[
    ("scope-folder", "This folder", SessionsPickerRoot::Cwd),
    ("scope-repo", "Repo", SessionsPickerRoot::Repo),
    ("scope-all", "All", SessionsPickerRoot::Any),
];

pub(crate) struct PickerToolbarLayout {
    pub scope_width: u16,
    pub show_new: bool,
    pub show_new_key: bool,
    pub filter_minimum: u16,
    pub count: String,
    pub machine: Option<String>,
}

/// R13 (design 5bafadfff): count shrinks before controls; filter never drops.
pub(crate) fn toolbar_layout(model: &SessionsPickerModel, width: u16) -> PickerToolbarLayout {
    let row_width = width.saturating_sub(4);
    let compact = format!(
        "{}/{}",
        model.visible_record_len(),
        model.request.records.len()
    );
    let compact_width = u16::try_from(compact.width()).unwrap_or(u16::MAX);
    let mandatory = MENU_WIDTH
        .saturating_add(FILTER_OUTER_MINIMUM)
        .saturating_add(compact_width)
        .saturating_add(TOOLBAR_ROW_GAP * 2);
    let show_new = row_width >= mandatory.saturating_add(NEW_WIDTH + TOOLBAR_ROW_GAP);
    let show_new_key =
        show_new && row_width >= mandatory.saturating_add(NEW_WITH_KEY_WIDTH + TOOLBAR_ROW_GAP);
    let buttons_width = MENU_WIDTH
        + if show_new {
            TOOLBAR_ROW_GAP
                + if show_new_key {
                    NEW_WITH_KEY_WIDTH
                } else {
                    NEW_WIDTH
                }
        } else {
            0
        };
    // The three complete, unkeyed buttons include one checked marker and separators.
    let scope_minimum = u16::try_from(
        SCOPE_SETTINGS
            .iter()
            .map(|(_, label, _)| label.width() + 2)
            .sum::<usize>()
            + 2
            + SCOPE_SETTINGS.len().saturating_sub(1),
    )
    .unwrap_or(u16::MAX);
    let scope_room = row_width
        .saturating_sub(buttons_width + FILTER_OUTER_MINIMUM + compact_width + TOOLBAR_ROW_GAP * 3);
    let show_scope = show_new && scope_room >= scope_minimum;
    let gaps = TOOLBAR_ROW_GAP * if show_scope { 3 } else { 2 };
    let filter_minimum =
        FILTER_OUTER_MINIMUM.min(row_width.saturating_sub(buttons_width + compact_width + gaps));
    let status_room = row_width.saturating_sub(
        buttons_width + filter_minimum + gaps + if show_scope { scope_minimum } else { 0 },
    );
    let machine = model
        .machine_controls
        .label(model.request.machine_mode)
        .to_owned();
    let full = format!("{compact} sessions");
    let (count, machine) = [
        (full, Some(machine.clone())),
        (compact.clone(), Some(machine)),
        (compact, None),
    ]
    .into_iter()
    .find(|(count, machine)| {
        let needed = count.width() + machine.as_ref().map_or(0, |machine| machine.width() + 3);
        needed <= usize::from(status_room)
    })
    .unwrap_or_else(|| {
        (
            format!(
                "{}/{}",
                model.visible_record_len(),
                model.request.records.len()
            ),
            None,
        )
    });
    let status_width =
        u16::try_from(count.width() + machine.as_ref().map_or(0, |machine| machine.width() + 3))
            .unwrap_or(u16::MAX);
    PickerToolbarLayout {
        scope_width: if show_scope {
            row_width.saturating_sub(buttons_width + filter_minimum + status_width + gaps)
        } else {
            0
        },
        show_new,
        show_new_key,
        filter_minimum,
        count,
        machine,
    }
}

pub(crate) fn render_toolbar(
    context: &PickerSurfaceContext,
    surface: SurfaceContext,
) -> SurfaceResult<AnyElement<'static>> {
    let model = context.model.read();
    let theme = surface.theme();
    let layout = toolbar_layout(&model, surface.stack.viewport().0);
    let action_button = |name: &str, menu: bool| -> SurfaceResult<AnyElement<'static>> {
        let action = ActionId::new(name)?;
        let id = TargetId::for_action(&action);
        Ok(if menu {
            element!(MenuButton(id, action)).into_any()
        } else {
            element!(Button(id, action,show_key:Some(layout.show_new_key))).into_any()
        })
    };
    let buttons = element!(View(min_width:0,column_gap:1,flex_shrink:0.0_f32) {
        #(Some(action_button("menu",true)?))
        #(layout.show_new.then(||action_button("new",false)).transpose()?)
    })
    .into_any();
    // One growing sibling keeps right-aligned status text on integer cells.
    // Prefer the existing filter cap and shrink it to the shared projection.
    let filter = element!(View(width:28,min_width:layout.filter_minimum,flex_shrink:1.0_f32,max_width:28) {
        FilterField(id:target("session-filter")?,value:context.filter,placeholder:"Filter…".to_owned())
    }).into_any();
    let scope_actions = SCOPE_SETTINGS
        .iter()
        .map(|(name, _, _)| ActionId::new(*name))
        .collect::<Result<Vec<_>, _>>()?;
    let scope = (layout.scope_width > 0).then(|| {
        element!(View(flex_shrink:0.0_f32) {SegmentedSwitch(actions:scope_actions,available_width:layout.scope_width)}).into_any()
    });
    let status=element!(View(min_width:0,flex_grow:1.0_f32,flex_shrink:0.0_f32,justify_content:JustifyContent::End) {
        View(column_gap:1,flex_shrink:0.0_f32) {
            Text(content:layout.count,color:theme.foreground(TextRole::Accent),weight:Weight::Bold,wrap:TextWrap::NoWrap)
            #(layout.machine.map(|machine|element!(View(column_gap:1,flex_shrink:0.0_f32) {
                Text(content:"·",color:theme.foreground(TextRole::Muted),wrap:TextWrap::NoWrap)
                Text(content:machine,color:theme.foreground(TextRole::Muted),wrap:TextWrap::NoWrap)
            })))
        }
    }).into_any();
    Ok(
        element!(View(width:100pct,column_gap:TOOLBAR_ROW_GAP,flex_shrink:0.0_f32) {
            #(Some(buttons)) #(Some(filter)) #(scope) #(Some(status))
        })
        .into_any(),
    )
}

#[cfg(test)]
pub(crate) mod picker_toolbar_projection_test;
