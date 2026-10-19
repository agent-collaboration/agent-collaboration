use codex_router_tui::UseDialogInteraction;
use iocraft::prelude::*;

use super::quota_status_view_model::*;
use super::responsive_quota_layout::visible_account_window_start;

const DETAIL_LABEL_WIDTH: usize = 10;

pub(super) struct QuotaAccountListProps<'a> {
    pub(super) rows: &'a [QuotaStatusAccountViewModel],
    pub(super) interactive: bool,
    pub(super) width: usize,
    pub(super) height: usize,
    pub(super) focused_row_index: Option<usize>,
    pub(super) visible_rows: usize,
    pub(super) focused_account_id: State<Option<codex_router_core::ids::AccountId>>,
    pub(super) interaction_enabled: bool,
    pub(super) empty_notice: Option<&'a str>,
}

pub(super) fn render_account_list(props: QuotaAccountListProps<'_>) -> AnyElement<'static> {
    let QuotaAccountListProps {
        rows,
        interactive,
        width,
        height,
        focused_row_index,
        visible_rows,
        mut focused_account_id,
        interaction_enabled,
        empty_notice,
    } = props;
    if interactive {
        return element!(QuotaAccountViewport(rows:rows.to_vec(),width,height,focused_row_index,focused_account_id,interaction_enabled,empty_notice:empty_notice.map(str::to_owned))).into_any();
    }
    let row_width = width.saturating_sub(4).max(32);
    let mut children = Vec::new();
    if rows.is_empty()
        && let Some(notice) = empty_notice
    {
        return element!(View(width: width as u32, height: height as u32) { Text(content: notice.to_owned(), color: Color::Grey) }).into_any();
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
            let account_id = row.account_id.clone();
            children.push(
                element! {
                    InteractiveQuotaAccountRow(
                        row: Some(row.clone()),
                        mode:super::quota_provider_filters::QuotaInteractionMode::Static,
                        width: row_width,
                        focused: focused_row_index == Some(index),
                        interaction_enabled,
                        click_handler: move |_| {
                            if focused_account_id.read().as_ref() != Some(&account_id) {
                                focused_account_id.set(Some(account_id.clone()));
                            }
                        },
                    )
                }
                .into_any(),
            );
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
            width: width as u32,
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

#[derive(Props)]
struct QuotaAccountViewportProps {
    rows: Vec<QuotaStatusAccountViewModel>,
    width: usize,
    height: usize,
    focused_row_index: Option<usize>,
    focused_account_id: Option<State<Option<codex_router_core::ids::AccountId>>>,
    interaction_enabled: bool,
    empty_notice: Option<String>,
}

#[component]
fn QuotaAccountViewport(props: &QuotaAccountViewportProps) -> impl Into<AnyElement<'static>> {
    let row_width = props.width.saturating_sub(5);
    let (account_width, status_width, quota_width) =
        quota_list_columns(row_width.saturating_sub(4));
    let rows=props.rows.iter().enumerate().map(|(index,row)| {
        let account_id=row.account_id.clone();let state=props.focused_account_id;
        element! {InteractiveQuotaAccountRow(row:Some(row.clone()),width:row_width,focused:props.focused_row_index==Some(index),interaction_enabled:props.interaction_enabled,
            click_handler:move |_|if let Some(mut state)=state {state.set(Some(account_id.clone()));})}.into_any()
    });
    element! {
        View(width:props.width as u32,max_height:props.height as u32,min_height:0,flex_shrink:1.0_f32,
            border_style:BorderStyle::Single,border_color:Color::DarkGrey,padding_left:1,padding_right:1,flex_direction:FlexDirection::Column,overflow:Overflow::Hidden) {
            View(padding_left:1,padding_right:2,flex_shrink:0.0_f32) {
                codex_router_tui::SelectableRow {
                    View(column_gap:1) {
                        View(width:account_width as u32) {Text(content:"Account",color:Color::White)}
                        View(width:status_width as u32) {Text(content:"Status",color:Color::White)}
                        View(width:quota_width as u32) {Text(content:"Quota",color:Color::White)}
                    }
                }
            }
            codex_router_tui::DialogBodyViewport {
                #(props.rows.is_empty().then(||element!(Text(content:props.empty_notice.clone().unwrap_or_else(||"No selectable account".to_owned()),color:Color::Grey))))
                #((rows.enumerate()).map(|(index,row)|element! {
                    View(flex_direction:FlexDirection::Column,flex_shrink:0.0_f32,margin_top:if index==0 {0}else {1}) {#(row)}
                }))
            }
        }
    }
}

#[derive(Default, Props)]
struct InteractiveQuotaAccountRowProps {
    row: Option<QuotaStatusAccountViewModel>,
    mode: super::quota_provider_filters::QuotaInteractionMode,
    width: usize,
    focused: bool,
    interaction_enabled: bool,
    click_handler: HandlerMut<'static, ()>,
}

#[component]
fn InteractiveQuotaAccountRow(
    props: &mut InteractiveQuotaAccountRowProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    hooks.use_dialog_local_events({
        let interaction_enabled = props.interaction_enabled;
        let mut click_handler = props.click_handler.take();
        move |event| {
            if !interaction_enabled {
                return;
            }
            if let TerminalEvent::FullscreenMouse(FullscreenMouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                ..
            }) = event
            {
                click_handler(());
            }
        }
    });

    props.row.as_ref().map_or_else(
        || element!(View).into_any(),
        |row| render_account_row(row, props.width, props.focused, props.mode),
    )
}

pub(super) fn quota_more_marker(content: String) -> AnyElement<'static> {
    element! {
        Text(content, color: Color::DarkGrey, weight: Weight::Light)
    }
    .into_any()
}

pub(super) fn quota_list_columns(width: usize) -> (usize, usize, usize) {
    let columns = width.saturating_sub(2);
    let account = (columns * 40 / 100).max(1);
    let status = (columns * 28 / 100).max(1);
    (account, status, columns.saturating_sub(account + status))
}

pub(super) fn render_account_row(
    row: &QuotaStatusAccountViewModel,
    width: usize,
    focused: bool,
    mode: super::quota_provider_filters::QuotaInteractionMode,
) -> AnyElement<'static> {
    let body_width = width.saturating_sub(4);
    let (account_width, status_width, quota_width) = quota_list_columns(body_width);
    let provider = match row.provider {
        codex_router_core::provider::Provider::Openai => "◎ OpenAI",
        codex_router_core::provider::Provider::Claude => "◆ Claude",
    };
    element! {
        View(width:width as u32,padding_left:1,padding_right:1,flex_shrink:0.0_f32) {
            codex_router_tui::SelectableRow(selected:focused) {
                View(column_gap:1,flex_shrink:0.0_f32) {
                    View(width:account_width as u32,min_width:0,flex_shrink:0.0_f32,flex_direction:FlexDirection::Column) {
                        Text(content:row.account.clone(),color:if focused {Color::Yellow}else {Color::White},weight:Weight::Bold)
                        Text(content:provider,color:Color::Grey)
                    }
                    View(width:status_width as u32,min_width:0,flex_shrink:0.0_f32,flex_direction:FlexDirection::Column) {
                        Text(content:if mode==super::quota_provider_filters::QuotaInteractionMode::Static {row.status.label().to_owned()}else {row.status.compact(status_width)},color:Color::White)
                        Text(content:row.active_clients.clone(),color:Color::Grey)
                        Text(content:format!("↻ {}",row.reset_credits),color:Color::Grey)
                        Text(content:format!("Credits {}",row.credit_usage_summary),color:Color::Grey)
                    }
                    View(width:quota_width as u32,min_width:0,flex_shrink:0.0_f32,flex_direction:FlexDirection::Column) {
                        Text(content:row.reason.clone(),color:Color::Grey)
                        codex_router_tui::FormField(label:"Weekly".to_owned(),label_width:11u16,stacked:quota_width<28) {Text(content:row.weekly_window.clone(),color:Color::White,wrap:TextWrap::Wrap)}
                        codex_router_tui::FormField(label:"5h".to_owned(),label_width:11u16,stacked:quota_width<28) {Text(content:row.short_window.clone(),color:Color::White,wrap:TextWrap::Wrap)}
                        codex_router_tui::FormField(label:"Weekly pace".to_owned(),label_width:11u16,stacked:quota_width<28) {Text(content:reset_pace_summary(&row.reset_pace),color:reset_pace_color(row.reset_pace.state),wrap:TextWrap::Wrap)}
                    }
                }
            }
        }
    }.into_any()
}

pub(super) fn render_selected_panel(
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
            width: width as u32,
            height: height as u32,
            flex_direction: FlexDirection::Column,
            border_style: BorderStyle::Single,
            border_color: Color::DarkGrey,
            overflow: Overflow::Hidden,
            padding_left: 1,
            padding_right: 1,
            padding_top: 0,
            padding_bottom: 0,
        ) {
            #(details)
        }
    }
    .into_any()
}

pub(super) fn render_selected_details(
    selected: &QuotaSelectedAccountViewModel,
    detail_width: usize,
    interactive: bool,
) -> AnyElement<'static> {
    let line = |label: &str, value: &str, color: Color| {
        if interactive {
            element! {
                View(width: 100pct) {
                    View(width: DETAIL_LABEL_WIDTH as u32, flex_shrink: 0.0_f32) {
                        Text(content: label.to_owned(), color: Color::Grey, wrap: TextWrap::NoWrap)
                    }
                    View(flex_grow: 1.0_f32, flex_shrink: 1.0_f32, min_width: 0) {
                        Text(content: value.to_owned(), color, wrap: TextWrap::Wrap)
                    }
                }
            }
            .into_any()
        } else {
            detail_line(label, value, detail_width, color)
        }
    };
    let pace_line = |label: &str, pace: &ResetPaceViewModel| {
        if interactive {
            line(
                label,
                &reset_pace_summary(pace),
                reset_pace_color(pace.state),
            )
        } else {
            reset_pace_detail_line(label, pace, detail_width)
        }
    };
    element! {
        View(
            width: detail_width as u32,
            max_width: 100pct,
            flex_direction: FlexDirection::Column,
            row_gap: 0,
            padding_top: 0,
        ) {
            #(if interactive { element!(codex_router_tui::UiHeading(content: "Selected account".to_owned(), level: codex_router_tui::UiHeadingLevel::Section)).into_any() } else { element!(Text(content: "Selected account", color: Color::Cyan, weight: Weight::Bold)).into_any() })
            #(if interactive {
                element! {
                    View(width: 100pct, column_gap: 4, flex_wrap: FlexWrap::Wrap, flex_shrink: 0.0_f32) {
                        Text(content: selected.account.clone(), color: Color::White, weight: Weight::Bold, wrap: TextWrap::NoWrap)
                        Text(content: selected.status.clone(), color: Color::White, weight: Weight::Bold, wrap: TextWrap::NoWrap)
                        View(flex_grow: 1.0_f32, flex_shrink: 1.0_f32, min_width: 0, max_width: 100pct) {
                            Text(content: selected.reason.clone(), color: Color::White, weight: Weight::Bold, wrap: TextWrap::Wrap)
                        }
                    }
                }.into_any()
            } else {
                element!(Text(content: fit_line(&format!("{}    {}    {}", selected.account, selected.status, selected.reason), detail_width), color: Color::White, weight: Weight::Bold, wrap: TextWrap::NoWrap)).into_any()
            })
            #(if interactive { element!(codex_router_tui::UiHeading(content: "Quota windows".to_owned(), level: codex_router_tui::UiHeadingLevel::Section)).into_any() } else { element!(Text(content: "Quota windows", color: Color::Cyan, weight: Weight::Bold)).into_any() })
            #(line("weekly", &selected.weekly_window, Color::White))
            #(line("5h", &selected.short_window, Color::White))
            #(quota_gap())
            #(if interactive { element!(codex_router_tui::UiHeading(content: "Reset pace".to_owned(), level: codex_router_tui::UiHeadingLevel::Section)).into_any() } else { element!(Text(content: "Reset pace", color: Color::Cyan, weight: Weight::Bold)).into_any() })
            #(pace_line("weekly", &selected.reset_pace))
            #(line("sample", &sample_metadata_summary(&selected.sample_metadata), Color::White))
            #(line("rate", &selected.total_rate, Color::White))
            #(line("conn", &selected.connection_rate, Color::White))
            #(pace_line("5h", &selected.short_reset_pace))
            #(quota_gap())
            #(if interactive { element!(codex_router_tui::UiHeading(content: "Activity".to_owned(), level: codex_router_tui::UiHeadingLevel::Section)).into_any() } else { element!(Text(content: "Activity", color: Color::Cyan, weight: Weight::Bold)).into_any() })
            #(line("clients", &selected.active_clients, Color::White))
            #(line("guards", &selected.guards, Color::White))
            #(line("reset", &selected.reset, Color::White))
            #(line("note", &selected.note, Color::Grey))
        }
    }
    .into_any()
}

pub(super) fn reset_pace_detail_line(
    label: &str,
    reset_pace: &ResetPaceViewModel,
    width: usize,
) -> AnyElement<'static> {
    let value_width = width.saturating_sub(DETAIL_LABEL_WIDTH).max(12);
    let reset_pace_text = fit_line(&reset_pace_summary(reset_pace), value_width);
    element! {
        View(width: width as u32) {
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

pub(super) fn detail_line(
    label: &str,
    value: &str,
    width: usize,
    color: Color,
) -> AnyElement<'static> {
    let value_width = width.saturating_sub(DETAIL_LABEL_WIDTH).max(12);
    element! {
        View(width: width as u32) {
            Text(content: fit_line(label, DETAIL_LABEL_WIDTH), color: Color::Grey, wrap: TextWrap::NoWrap)
            Text(content: fit_line(value, value_width), color, wrap: TextWrap::NoWrap)
        }
    }
    .into_any()
}

pub(super) const fn reset_pace_color(state: ResetPaceState) -> Color {
    match state {
        ResetPaceState::UnderBurning => Color::Yellow,
        ResetPaceState::Healthy => Color::Green,
        ResetPaceState::OverBurning => Color::Red,
        ResetPaceState::Unavailable => Color::Grey,
    }
}

pub(super) fn colorize_reset_pace_ansi(text: &str) -> String {
    text.lines()
        .map(colorize_reset_pace_line_ansi)
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn colorize_reset_pace_line_ansi(line: &str) -> String {
    let mut ranges = reset_pace_ansi_ranges(line);
    if ranges.is_empty() {
        return line.to_owned();
    }
    ranges.sort_by_key(|(start, _, _)| *start);
    let mut output = String::new();
    let mut cursor = 0;
    for (start, end, color) in ranges {
        if start < cursor {
            continue;
        }
        output.push_str(line.get(cursor..start).unwrap_or_default());
        output.push_str(color);
        output.push_str(line.get(start..end).unwrap_or_default());
        output.push_str("\u{1b}[0m");
        cursor = end;
    }
    output.push_str(line.get(cursor..).unwrap_or_default());
    output
}

pub(super) fn reset_pace_ansi_ranges(line: &str) -> Vec<(usize, usize, &'static str)> {
    let mut ranges = Vec::new();
    for (needle, color) in [
        (" reset pace healthy", "\u{1b}[38;5;10m"),
        (" reset pace under", "\u{1b}[38;5;11m"),
        (" reset pace over", "\u{1b}[38;5;9m"),
        (" pace healthy", "\u{1b}[38;5;10m"),
        (" pace under", "\u{1b}[38;5;11m"),
        (" pace over", "\u{1b}[38;5;9m"),
        (" weekly runs out", "\u{1b}[38;5;9m"),
        ("Exhausted", "\u{1b}[38;5;9m"),
    ] {
        let mut cursor = 0;
        while let Some(relative_index) = line.get(cursor..).and_then(|suffix| suffix.find(needle)) {
            let phrase_start = cursor + relative_index;
            let end = phrase_start + needle.len();
            let start = if needle == "Exhausted" {
                phrase_start
            } else {
                reset_pace_segment_start(line, phrase_start)
            };
            ranges.push((start, end, color));
            cursor = end;
        }
    }
    ranges
}

pub(super) fn reset_pace_segment_start(line: &str, phrase_start: usize) -> usize {
    let mut cursor = phrase_start;
    cursor = scan_back_while(line, cursor, |character| {
        character.is_ascii_digit() || character == '.' || character == 'x'
    });
    cursor = scan_back_while(line, cursor, char::is_whitespace);
    scan_back_while(line, cursor, |character| {
        matches!(character, '□' | '■' | '│')
    })
}

pub(super) fn scan_back_while(
    line: &str,
    cursor: usize,
    mut predicate: impl FnMut(char) -> bool,
) -> usize {
    let mut start = cursor;
    let Some(prefix) = line.get(..cursor) else {
        return start;
    };
    for (index, character) in prefix.char_indices().rev() {
        if !predicate(character) {
            break;
        }
        start = index;
    }
    start
}

pub(super) fn reset_pace_summary(reset_pace: &ResetPaceViewModel) -> String {
    if reset_pace.state == ResetPaceState::Unavailable {
        return format!(
            "{}  {}",
            reset_pace_meter(reset_pace),
            reset_pace.semantic_label
        );
    }
    if let Some(impact_label) = &reset_pace.impact_label {
        if is_depleted_quota_label(impact_label) {
            return impact_label.clone();
        }
        return format!("{}  {}", reset_pace_meter(reset_pace), impact_label);
    }
    format!(
        "{}  {} {}",
        reset_pace_meter(reset_pace),
        reset_pace.multiple_label,
        reset_pace.semantic_label
    )
}

pub(super) fn is_depleted_quota_label(value: &str) -> bool {
    value == "Exhausted"
}

pub(super) fn reset_pace_meter(reset_pace: &ResetPaceViewModel) -> String {
    reset_pace_meter_slots(
        reset_pace.meter_left_segments.filled,
        reset_pace.center_marker,
        reset_pace.meter_right_segments.filled,
    )
}

pub(super) fn reset_pace_meter_slots(
    left_filled: usize,
    center_marker: char,
    right_filled: usize,
) -> String {
    const RESET_PACE_METER_SIDE_WIDTH: usize = 7;
    const RESET_PACE_METER_EMPTY: char = '□';
    const RESET_PACE_METER_FILLED: char = '■';
    let mut left_slots = [RESET_PACE_METER_EMPTY; RESET_PACE_METER_SIDE_WIDTH];
    let mut right_slots = [RESET_PACE_METER_EMPTY; RESET_PACE_METER_SIDE_WIDTH];
    for slot in left_slots
        .iter_mut()
        .rev()
        .take(left_filled.min(RESET_PACE_METER_SIDE_WIDTH))
    {
        *slot = RESET_PACE_METER_FILLED;
    }
    for slot in right_slots
        .iter_mut()
        .take(right_filled.min(RESET_PACE_METER_SIDE_WIDTH))
    {
        *slot = RESET_PACE_METER_FILLED;
    }

    left_slots
        .into_iter()
        .chain(std::iter::once(center_marker))
        .chain(right_slots)
        .collect()
}

pub(super) fn sample_metadata_summary(sample_metadata: &SampleMetadata) -> String {
    if sample_metadata.confidence == SampleConfidence::Unknown {
        return sample_metadata.semantic_label.to_owned();
    }
    format!(
        "{} {}",
        sample_metadata.semantic_label, sample_metadata.age_label
    )
}

pub(super) fn quota_gap() -> AnyElement<'static> {
    element! {
        View(height: 1) {
            Text(content: "")
        }
    }
    .into_any()
}

pub(super) fn fit_line(value: &str, width: usize) -> String {
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
