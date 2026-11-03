//! Stateless quota fact formatting shared by printed and interactive views.
use agent_collaboration_tui::UiTextRole;
use codex_router_quota_service::quota_display_model::{
    ResetPaceState, ResetPaceViewModel, SampleConfidence, SampleMetadata,
};
use iocraft::Color;

pub(crate) const fn reset_pace_role(state: ResetPaceState) -> UiTextRole {
    match state {
        ResetPaceState::UnderBurning => UiTextRole::Warning,
        ResetPaceState::Healthy => UiTextRole::Success,
        ResetPaceState::OverBurning => UiTextRole::Error,
        ResetPaceState::Unavailable => UiTextRole::Secondary,
    }
}
pub(crate) const fn reset_pace_color(state: ResetPaceState) -> Color {
    reset_pace_role(state).color()
}

pub(crate) fn colorize_reset_pace_ansi(text: &str) -> String {
    text.lines()
        .map(colorize_reset_pace_line_ansi)
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn colorize_reset_pace_line_ansi(line: &str) -> String {
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

pub(crate) fn reset_pace_ansi_ranges(line: &str) -> Vec<(usize, usize, &'static str)> {
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

pub(crate) fn reset_pace_segment_start(line: &str, phrase_start: usize) -> usize {
    let mut cursor = phrase_start;
    cursor = scan_back_while(line, cursor, |character| {
        character.is_ascii_digit() || character == '.' || character == 'x'
    });
    cursor = scan_back_while(line, cursor, char::is_whitespace);
    scan_back_while(line, cursor, |character| {
        matches!(character, '□' | '■' | '│')
    })
}

pub(crate) fn scan_back_while(
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

pub(crate) fn reset_pace_summary(reset_pace: &ResetPaceViewModel) -> String {
    if reset_pace.state == ResetPaceState::Unavailable {
        return reset_pace.semantic_label.to_owned();
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

pub(crate) fn is_depleted_quota_label(value: &str) -> bool {
    value == "Exhausted"
}

pub(crate) fn reset_pace_meter(reset_pace: &ResetPaceViewModel) -> String {
    reset_pace_meter_slots(
        reset_pace.meter_left_segments.filled,
        reset_pace.center_marker,
        reset_pace.meter_right_segments.filled,
    )
}

pub(crate) fn reset_pace_meter_slots(
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

pub(crate) fn sample_metadata_summary(sample_metadata: &SampleMetadata) -> String {
    if sample_metadata.confidence == SampleConfidence::Unknown {
        return sample_metadata.semantic_label.to_owned();
    }
    format!(
        "{} {}",
        sample_metadata.semantic_label, sample_metadata.age_label
    )
}
