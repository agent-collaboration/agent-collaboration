use super::quota_fact_presentation::{
    reset_pace_color, reset_pace_role, reset_pace_summary, sample_metadata_summary,
};
use agent_collaboration_tui::{UiText, UiTextRole};
use iocraft::prelude::*;

use super::responsive_quota_layout::visible_account_window_start;
use codex_router_quota_service::quota_display_model::{
    QuotaSelectedAccountViewModel, QuotaStatusAccountViewModel, ResetPaceState, ResetPaceViewModel,
};

const DETAIL_LABEL_WIDTH: usize = 10;

#[derive(Clone, Copy)]
pub(crate) struct QuotaAccountListProps<'a> {
    pub(crate) rows: &'a [QuotaStatusAccountViewModel],
    pub(crate) compact_phone: bool,
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) focused_row_index: Option<usize>,
    pub(crate) visible_rows: usize,
    pub(crate) empty_notice: Option<&'a str>,
}

pub(crate) fn render_account_list(props: QuotaAccountListProps<'_>) -> AnyElement<'static> {
    let QuotaAccountListProps {
        rows,
        compact_phone,
        width,
        height,
        focused_row_index,
        visible_rows,
        empty_notice,
    } = props;
    let row_width = width.saturating_sub(4).max(32);
    let mut children = Vec::new();
    if rows.is_empty()
        && let Some(notice) = empty_notice
    {
        return element!(View(width: u32::try_from(width).unwrap_or(u32::MAX), height: u32::try_from(height).unwrap_or(u32::MAX)) { Text(content: notice.to_owned(), color: Color::Grey) }).into_any();
    }
    let window_start = visible_account_window_start(focused_row_index, rows.len(), visible_rows);
    let compact_overflow_markers = super::responsive_quota_layout::quota_account_list_height(
        rows.len(),
        focused_row_index,
        visible_rows,
    ) > height;
    if window_start > 0 {
        children.push(quota_more_marker(format!("+{window_start} more above")));
        if !compact_overflow_markers {
            children.push(quota_gap());
        }
    }
    let window_end = (window_start + visible_rows).min(rows.len());
    for (offset, index) in (window_start..window_end).enumerate() {
        if offset > 0 {
            children.push(quota_gap());
        }
        if let Some(row) = rows.get(index) {
            children.push(render_account_row(
                row,
                row_width,
                focused_row_index == Some(index),
                super::quota_view_configuration::QuotaInteractionMode::Static,
                compact_phone,
            ));
        }
    }
    let remaining = rows.len().saturating_sub(window_start + visible_rows);
    if remaining > 0 {
        if !compact_overflow_markers {
            children.push(quota_gap());
        }
        children.push(quota_more_marker(format!("+{remaining} more below")));
    }

    element! {
        View(
            width: u32::try_from(width).unwrap_or(u32::MAX),
            flex_direction: FlexDirection::Column,
            border_style: BorderStyle::Single,
            border_color: Color::DarkGrey,
            overflow: Overflow::Hidden,
            padding_left: 1,
            padding_right: 1,
            padding_top: 0,
            padding_bottom: 0,
        ) {
            #(children)
        }
    }
    .into_any()
}

pub(crate) fn quota_more_marker(content: String) -> AnyElement<'static> {
    element! {
        Text(content, color: Color::DarkGrey, weight: Weight::Light)
    }
    .into_any()
}

pub(crate) fn quota_list_columns(width: usize) -> (usize, usize, usize) {
    let columns = width.saturating_sub(2);
    let account = (columns * 40 / 100).max(1);
    let status = (columns * 28 / 100).max(1);
    (account, status, columns.saturating_sub(account + status))
}

pub(crate) fn render_account_row(
    row: &QuotaStatusAccountViewModel,
    width: usize,
    focused: bool,
    mode: super::quota_view_configuration::QuotaInteractionMode,
    compact_phone: bool,
) -> AnyElement<'static> {
    if mode == super::quota_view_configuration::QuotaInteractionMode::Interactive {
        return super::quota_account_groups::render_interactive_account(row, width, focused);
    }
    let row_padding = usize::from(!compact_phone);
    let body_width = width.saturating_sub(2 + 2 * row_padding);
    let (account_width, status_width, quota_width) = quota_list_columns(body_width);
    // "unavailable" needs eleven cells. Keep narrow no-estimate wording whole;
    // Details retains the full label, including the collecting-data reason.
    let pace_summary =
        if compact_phone && quota_width < 11 && row.reset_pace.state == ResetPaceState::Unavailable
        {
            "Unknown".to_owned()
        } else {
            reset_pace_summary(&row.reset_pace)
        };
    let provider = match row.provider {
        codex_router_core::provider::Provider::Openai => "◎ OpenAI",
        codex_router_core::provider::Provider::Claude => "◆ Claude",
    };
    element! {
        View(width:u32::try_from(width).unwrap_or(u32::MAX),padding_left:u32::try_from(row_padding).unwrap_or(u32::MAX),padding_right:u32::try_from(row_padding).unwrap_or(u32::MAX),flex_shrink:0.0_f32) {
            agent_collaboration_tui::SelectableRow(selected:focused) {
                View(column_gap:1,flex_shrink:0.0_f32) {
                    View(width:u32::try_from(account_width).unwrap_or(u32::MAX),min_width:0,flex_shrink:0.0_f32,flex_direction:FlexDirection::Column) {
                        Text(content:row.account.clone(),color:if focused {Color::Yellow}else {Color::White},weight:Weight::Bold)
                        Text(content:provider,color:Color::Grey)
                    }
                    View(width:u32::try_from(status_width).unwrap_or(u32::MAX),min_width:0,flex_shrink:0.0_f32,flex_direction:FlexDirection::Column) {
                        Text(content:if mode==super::quota_view_configuration::QuotaInteractionMode::Static {row.status.label().to_owned()}else {row.status.compact(status_width)},color:Color::White)
                        Text(content:row.active_clients.clone(),color:Color::Grey)
                        Text(content:row.reset_credits.clone(),color:Color::Grey)
                        Text(content:format!("Credits {}",row.credit_usage_summary),color:Color::Grey)
                    }
                    View(width:u32::try_from(quota_width).unwrap_or(u32::MAX),min_width:0,flex_shrink:0.0_f32,flex_direction:FlexDirection::Column) {
                        Text(content:row.reason.clone(),color:Color::Grey)
                        agent_collaboration_tui::FormFields(stacked:quota_width<28) {
                        agent_collaboration_tui::FormField(label:"Weekly".to_owned()) {Text(content:row.weekly_window.clone(),color:Color::White,wrap:TextWrap::Wrap)}
                        agent_collaboration_tui::FormField(label:"5h".to_owned()) {Text(content:row.short_window.clone(),color:Color::White,wrap:TextWrap::Wrap)}
                        agent_collaboration_tui::FormField(label:"Weekly pace".to_owned()) {Text(content:pace_summary,color:reset_pace_color(row.reset_pace.state),wrap:TextWrap::Wrap)}
                        }
                    }
                }
            }
        }
    }.into_any()
}

pub(crate) fn render_selected_panel(
    selected: Option<&QuotaSelectedAccountViewModel>,
    width: usize,
    height: usize,
    interactive: bool,
) -> AnyElement<'static> {
    let details = selected
        .map(|selected| render_selected_details(selected, width.saturating_sub(4), interactive))
        .unwrap_or_else(|| {
            element! {
                Text(content: "No selectable account", color: Color::Grey)
            }
            .into_any()
        });

    element! {
        View(
            width: u32::try_from(width).unwrap_or(u32::MAX),
            height: u32::try_from(height).unwrap_or(u32::MAX),
            flex_direction: FlexDirection::Column,
            border_style: BorderStyle::Single,
            border_color: Color::DarkGrey,
            overflow: Overflow::Hidden,
            padding_left: 1,
            padding_right: 1,
            padding_top: 0,
            padding_bottom: 0,
        ) {
            #((interactive).then(||selected.map(|selected|element! {
                View(flex_direction:FlexDirection::Column,flex_shrink:0.0_f32) {
                    agent_collaboration_tui::UiHeading(content:"Selected account".to_owned(),level:agent_collaboration_tui::UiHeadingLevel::Section)
                    UiText(content:selected.account.clone(),role:UiTextRole::Body,wrap:TextWrap::Wrap)
                }
            }.into_any())).flatten())
            #(details)
        }
    }
    .into_any()
}

pub(crate) fn render_selected_details(
    selected: &QuotaSelectedAccountViewModel,
    detail_width: usize,
    interactive: bool,
) -> AnyElement<'static> {
    if interactive {
        return super::quota_account_details_view::render_account_details(
            selected,
            detail_width,
            None,
        );
    }
    let line =
        |label: &str, value: &str, role: UiTextRole| -> agent_collaboration_tui::FormFieldsChild {
            if interactive {
                element! {
                    agent_collaboration_tui::FormField(label:label.to_owned()) {
                        UiText(content:value.to_owned(),role,wrap:TextWrap::Wrap)
                    }
                }
                .into()
            } else {
                agent_collaboration_tui::FormFieldsChild::from(detail_line(
                    label,
                    value,
                    detail_width,
                    role.color(),
                ))
            }
        };
    let pace_line = |label: &str, pace: &ResetPaceViewModel| {
        if interactive {
            line(
                label,
                &reset_pace_summary(pace),
                reset_pace_role(pace.state),
            )
        } else {
            reset_pace_detail_line(label, pace, detail_width).into()
        }
    };
    element! {
        View(
            width: u32::try_from(detail_width).unwrap_or(u32::MAX),
            max_width: 100pct,
            flex_direction: FlexDirection::Column,
            row_gap: 0,
            padding_top: 0,
        ) {
            agent_collaboration_tui::FormFields(stacked:detail_width<32) {
            #((!interactive).then(||element!(Text(content:"Selected account",color:Color::Cyan,weight:Weight::Bold)).into_any()))
            #(if interactive {
                vec![line("Status",&selected.status,UiTextRole::Body),line("Reason",&selected.reason,UiTextRole::Body)]
            }else {
                vec![element!(Text(content:fit_line(&format!("{}    {}    {}",selected.account,selected.status,selected.reason),detail_width),color:Color::White,weight:Weight::Bold,wrap:TextWrap::NoWrap)).into_any().into()]
            })
            #([if interactive { element!(agent_collaboration_tui::UiHeading(content: "Quota windows".to_owned(), level: agent_collaboration_tui::UiHeadingLevel::Section)).into_any() } else { element!(Text(content: "Quota windows", color: Color::Cyan, weight: Weight::Bold)).into_any() }])
            #([line("weekly", &selected.weekly_window, UiTextRole::Body)])
            #([line("5h", &selected.short_window, UiTextRole::Body)])
            #([quota_gap()])
            #([if interactive { element!(agent_collaboration_tui::UiHeading(content: "Reset pace".to_owned(), level: agent_collaboration_tui::UiHeadingLevel::Section)).into_any() } else { element!(Text(content: "Reset pace", color: Color::Cyan, weight: Weight::Bold)).into_any() }])
            #([pace_line("weekly", &selected.reset_pace)])
            #([line("sample", &sample_metadata_summary(&selected.sample_metadata), UiTextRole::Body)])
            #([line("rate", &selected.total_rate, UiTextRole::Body)])
            #([line("conn", &selected.connection_rate, UiTextRole::Body)])
            #([pace_line("5h", &selected.short_reset_pace)])
            #([quota_gap()])
            #([if interactive { element!(agent_collaboration_tui::UiHeading(content: "Activity".to_owned(), level: agent_collaboration_tui::UiHeadingLevel::Section)).into_any() } else { element!(Text(content: "Activity", color: Color::Cyan, weight: Weight::Bold)).into_any() }])
            #([line("clients", &selected.active_clients, UiTextRole::Body)])
            #([line("guards", &selected.guards, UiTextRole::Body)])
            #([line("reset", &selected.reset, UiTextRole::Body)])
            #([line("note", &selected.note, UiTextRole::Secondary)])
            }
        }
    }
    .into_any()
}

pub(crate) fn reset_pace_detail_line(
    label: &str,
    reset_pace: &ResetPaceViewModel,
    width: usize,
) -> AnyElement<'static> {
    let value_width = width.saturating_sub(DETAIL_LABEL_WIDTH).max(12);
    let reset_pace_text = fit_line(&reset_pace_summary(reset_pace), value_width);
    element! {
        View(width: u32::try_from(width).unwrap_or(u32::MAX)) {
            Text(content: fit_line(label, DETAIL_LABEL_WIDTH), color: Color::Grey, wrap: TextWrap::NoWrap)
            MixedText(
                contents: vec![
                    MixedTextContent::new(reset_pace_text).color(reset_pace_color(reset_pace.state)),
                ],
                wrap: TextWrap::NoWrap,
            )
        }
    }
    .into_any()
}

pub(crate) fn detail_line(
    label: &str,
    value: &str,
    width: usize,
    color: Color,
) -> AnyElement<'static> {
    let value_width = width.saturating_sub(DETAIL_LABEL_WIDTH).max(12);
    element! {
        View(width: u32::try_from(width).unwrap_or(u32::MAX)) {
            Text(content: fit_line(label, DETAIL_LABEL_WIDTH), color: Color::Grey, wrap: TextWrap::NoWrap)
            Text(content: fit_line(value, value_width), color, wrap: TextWrap::NoWrap)
        }
    }
    .into_any()
}

pub(crate) fn quota_gap() -> AnyElement<'static> {
    element! {
        View(height: 1) {
            Text(content: "")
        }
    }
    .into_any()
}

pub(crate) fn fit_line(value: &str, width: usize) -> String {
    let line = value.replace('\n', " ");
    let char_count = line.chars().count();
    if char_count <= width {
        return format!("{line:<width$}");
    }
    if width <= 1 {
        return "…".to_owned();
    }
    let keep = width - 1;
    format!("{}…", line.chars().take(keep).collect::<String>())
}
