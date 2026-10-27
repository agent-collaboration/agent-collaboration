//! Picker list view.
use super::{
    InteractiveSessionChoiceRow, SessionPickerRecord, SessionsPickerModel, SessionsPickerOutcome,
    compact_age, truncate_end,
};
use crate::presentation::session_picker::picker_rendering::iocraft_cell_count;
use iocraft::prelude::*;

const STATUS_COLUMN_WIDTH: usize = 6;
const AGE_COLUMN_WIDTH: usize = 6;

pub(super) const fn session_rows_are_stacked(viewport_width: usize) -> bool {
    // At phone widths the status and two ages would consume fitting identities.
    viewport_width < 40
}

fn session_column_widths(width: usize) -> (usize, usize, usize) {
    let inner_width = width.saturating_sub(2);
    // Compact rows show a status icon, so preserve identity space instead of
    // reserving the full desktop heading width for that one-cell value.
    let status_width = if width < 43 { 2 } else { STATUS_COLUMN_WIDTH };
    let age_width = if width < 43 { 3 } else { AGE_COLUMN_WIDTH };
    let fixed_width = 2 + status_width + (age_width * 2) + 3;
    let title_width = inner_width.saturating_sub(fixed_width).max(2);
    (title_width, status_width, age_width)
}

pub(super) fn render_session_list(
    model: &SessionsPickerModel,
    mut model_state: State<SessionsPickerModel>,
    _selected_outcome: State<Option<SessionsPickerOutcome>>,
    width: usize,
    visible_rows: usize,
    panel_height: usize,
) -> AnyElement<'static> {
    let row_width = width.saturating_sub(6).max(24);
    let stacked = session_rows_are_stacked(model.width);
    let mut rows = vec![render_session_header(row_width, stacked)];
    let visible_len = model.visible_len();
    let focused_index = model.focused_visible_index();
    let visible_rows = visible_rows.max(1);
    let window_start = model.focused_window_start(visible_rows);
    if window_start > 1 {
        rows.push(
            element! {
                Text(
                    content: format!("+{} more above",window_start-1),
                    color: Color::DarkGrey,
                    weight: Weight::Light,
                )
            }
            .into_any(),
        );
        rows.push(list_gap());
    }
    if model.visible_record_len() == 0 {
        rows.push(
            element!(Text(content:no_matching_sessions_label(model),color:Color::Grey)).into_any(),
        );
    }
    let window_end = (window_start + visible_rows).min(visible_len);
    for visible_index in window_start..window_end {
        if visible_index > window_start.max(1) {
            rows.push(list_gap());
        }
        if visible_index == 0 {
            continue;
        }
        if let Some(record) = model.visible_choice_record_at(visible_index) {
            let identity = record.identity.clone();
            let row = render_record_row(record, visible_index == focused_index, row_width, stacked);
            rows.push(
                element! {
                    InteractiveSessionChoiceRow(
                        focus_handler: move |_| {
                            model_state.write().machine_controls.control_focused = false;
                            let should_update_focus = {
                                let model_value = model_state.read();
                                model_value.focused_identity() != Some(&identity)
                                    || model_value.focused_window_start(visible_rows) != window_start
                            };
                            if should_update_focus {
                                let _ = model_state.write().focus_visible_identity_in_window(
                                    &identity,
                                    Some(window_start),
                                );
                            }
                        },
                        activates_on_click: false,
                    ) {
                        #(row)
                    }
                }
                .into_any(),
            );
        }
    }
    let remaining = visible_len.saturating_sub(window_start + visible_rows);
    if remaining > 0 {
        rows.push(list_gap());
        rows.push(
            element! {
                Text(
                    content: format!("+{remaining} more below"),
                    color: Color::DarkGrey,
                    weight: Weight::Light,
                )
            }
            .into_any(),
        );
    }

    element! {
        View(
            width: iocraft_cell_count(width),
            height: iocraft_cell_count(panel_height),
            flex_direction: FlexDirection::Column,
            border_style: BorderStyle::Single,
            border_color: Color::DarkGrey,
            overflow: Overflow::Hidden,
            padding_left: 1,
            padding_right: 1,
            padding_top: 0,
            padding_bottom: 0,
        ) {
            #(rows)
        }
    }
    .into_any()
}

pub(super) fn no_matching_sessions_label(model: &SessionsPickerModel) -> &'static str {
    if model.search.is_empty() {
        "No existing sessions match these filters"
    } else {
        "No matching sessions"
    }
}

pub(super) fn start_new_args_label(model: &SessionsPickerModel) -> String {
    if model.request.new_session_args_display.is_empty() {
        String::new()
    } else {
        format!("args: {}", model.request.new_session_args_display)
    }
}

pub(super) fn render_session_header(width: usize, stacked: bool) -> AnyElement<'static> {
    if stacked {
        return element! {
            View(width:iocraft_cell_count(width),border_style:BorderStyle::Single,border_edges:Edges::Bottom,
                border_color:Color::DarkGrey,padding_left:1,padding_right:1) {
                View(width:2)
                Text(content:"Session",color:Color::White,weight:Weight::Bold,wrap:TextWrap::NoWrap)
            }
        }
        .into_any();
    }
    let (title_width, status_width, age_width) = session_column_widths(width);
    element! {
        View(
            width: iocraft_cell_count(width),
            border_style: BorderStyle::Single,
            border_edges: Edges::Bottom,
            border_color: Color::DarkGrey,
            padding_left: 1,
            padding_right: 1,
        ) {
            View(width: 2) { Text(content: "  ", color: Color::White, weight: Weight::Bold) }
            View(width: iocraft_cell_count(title_width)) { Text(content: "Session", color: Color::White, weight: Weight::Bold) }
            Text(content: " ")
            View(width: iocraft_cell_count(status_width), overflow: Overflow::Hidden) { Text(content: if width < 43 { "St" } else { "Status" }, color: Color::White, weight: Weight::Bold) }
            Text(content: " ")
            View(width: iocraft_cell_count(age_width), overflow: Overflow::Hidden, justify_content: JustifyContent::FlexEnd) { Text(content: "Upd", color: Color::White, weight: Weight::Bold) }
            Text(content: " ")
            View(width: iocraft_cell_count(age_width), overflow: Overflow::Hidden, justify_content: JustifyContent::FlexEnd) { Text(content: "New", color: Color::White, weight: Weight::Bold) }
        }
    }
    .into_any()
}

pub(super) fn list_gap() -> AnyElement<'static> {
    element! {
        View(height: 1) {
            Text(content: "")
        }
    }
    .into_any()
}

pub(super) fn render_record_row(
    record: &SessionPickerRecord,
    selected: bool,
    width: usize,
    stacked: bool,
) -> AnyElement<'static> {
    let foreground = if selected { Color::White } else { Color::Grey };
    let metadata = Color::Grey;
    let (title_width, status_width, age_width) = session_column_widths(width);
    let title_width = if stacked {
        width.saturating_sub(4)
    } else {
        title_width
    };
    let title = truncate_end(&record.title, title_width);
    let status = record.runtime_status.icon();
    let cwd = record.cwd.as_deref().unwrap_or("-");

    element! {
        View(
            width: iocraft_cell_count(width),
            flex_direction: FlexDirection::Column,
            padding_left: 1,
            padding_right: 1,
            padding_top: 0,
            padding_bottom: 0,
        ) {
            codex_router_tui::SelectableRow(selected) {
                #(if stacked { element! {
                    View(width:100pct,min_width:0,overflow:Overflow::Hidden) {
                        Text(content:title,color:if selected {Color::Yellow}else{foreground},weight:Weight::Bold,wrap:TextWrap::NoWrap)
                    }
                }.into_any() } else { element! { View(width: 100pct,column_gap:1) {
                    View(width: iocraft_cell_count(title_width), overflow: Overflow::Hidden) { Text(content: title, color: if selected { Color::Yellow } else { foreground }, weight: Weight::Bold, wrap: TextWrap::NoWrap) }
                    View(width: iocraft_cell_count(status_width), overflow: Overflow::Hidden) { Text(content: status, color:foreground,wrap:TextWrap::NoWrap) }
                    View(width: iocraft_cell_count(age_width), overflow: Overflow::Hidden, justify_content:JustifyContent::FlexEnd) {Text(content:compact_age(&record.recency),color:foreground,wrap:TextWrap::NoWrap)}
                    View(width: iocraft_cell_count(age_width), overflow: Overflow::Hidden, justify_content:JustifyContent::FlexEnd) {Text(content:compact_age(&record.created),color:foreground,wrap:TextWrap::NoWrap)}
                }}.into_any() })
                #(stacked.then(||element! {
                    View(width:100pct,column_gap:1) {
                        Text(content:status,color:foreground,wrap:TextWrap::NoWrap)
                        Text(content:format!("Upd {}",compact_age(&record.recency)),color:metadata,wrap:TextWrap::NoWrap)
                        Text(content:format!("New {}",compact_age(&record.created)),color:metadata,wrap:TextWrap::NoWrap)
                    }
                }))
                View(width:100pct,column_gap:1,overflow:Overflow::Hidden) {
                    Text(content:record.client_provider_family().label(),color:metadata,wrap:TextWrap::NoWrap)
                    Text(content:record.machine_label(),color:metadata,wrap:TextWrap::NoWrap)
                    View(max_width:12,overflow:Overflow::Hidden) {Text(content:format!("⎇ {}",record.branch),color:metadata,wrap:TextWrap::NoWrap)}
                    View(flex_grow:1.0_f32,min_width:0,overflow:Overflow::Hidden) {Text(content:format!("📂 {cwd}"),color:metadata,wrap:TextWrap::NoWrap)}
                }
            }
        }
    }
    .into_any()
}
