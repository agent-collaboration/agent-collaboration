//! Picker layout budget.
use crate::session_picker::screen::picker_component::SessionsPickerModel;

pub(crate) fn session_visible_row_budget(
    model: &SessionsPickerModel,
    available_height: usize,
) -> usize {
    let visible_len = model.visible_len();
    if visible_len == 0 {
        return 0;
    }
    for candidate in (1..=visible_len).rev() {
        if session_list_height(model, model.focused_window_start(candidate), candidate)
            <= available_height
        {
            return candidate;
        }
    }
    1
}

pub(crate) fn session_list_height(
    model: &SessionsPickerModel,
    window_start: usize,
    visible_rows: usize,
) -> usize {
    let visible_len = model.visible_len();
    let record_height =
        if crate::session_picker::screen::picker_list_view::session_rows_are_stacked(model.width) {
            3
        } else {
            2
        };
    let visible_rows = visible_rows.max(1);
    let window_end = (window_start + visible_rows).min(visible_len);
    let visible_count = window_end.saturating_sub(window_start.max(1));
    let remaining = visible_len.saturating_sub(window_start + visible_rows);

    let header_height = 2;
    let record_rows_height = (window_start..window_end)
        .map(|index| session_choice_row_height(index, record_height))
        .sum::<usize>();
    let record_gap_height = visible_count.saturating_sub(1);
    let more_above_height = if window_start > 1 { 2 } else { 0 };
    let more_below_height = if remaining > 0 { 2 } else { 0 };
    let list_border_and_padding_height = 2;

    list_border_and_padding_height
        + header_height
        + more_above_height
        + record_rows_height
        + record_gap_height
        + more_below_height
}

const fn session_choice_row_height(visible_index: usize, record_height: usize) -> usize {
    if visible_index == 0 { 0 } else { record_height }
}
