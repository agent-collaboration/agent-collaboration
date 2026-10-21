//! Composed UX obligations from the owner-adopted correction brief.
use super::*;

#[tokio::test]
async fn phone_quota_component_honors_actual_physical_width() {
    for width in [32usize, 40] {
        let frame = render_quota_capture_model_at(
            quota_two_account_view_model(),
            width,
            24,
            vec![provider_filter_key(KeyCode::Esc)],
        )
        .await;
        assert!(
            frame
                .lines()
                .all(|line| unicode_width::UnicodeWidthStr::width(line) <= width),
            "rendering a wider UI and clipping is not phone support at {width}: {frame}"
        );
        assert!(
            frame.contains("Quota status") && frame.contains("Help"),
            "{frame}"
        );
    }
}

#[tokio::test]
async fn quota_header_keeps_named_total_and_usable_counts_at_every_width() {
    for width in [120usize, 76, 48, 40, 32] {
        let frame = render_quota_capture_model_at(
            quota_two_account_view_model(),
            width,
            24,
            vec![provider_filter_key(KeyCode::Esc)],
        )
        .await;
        let header = frame
            .lines()
            .take(5)
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        assert!(
            header.contains("accounts 2") && header.contains("usable 2"),
            "unexplained counts at {width}: {frame}"
        );
    }
}

#[tokio::test]
async fn contextual_help_uses_current_census_including_zero_and_distinct_unknown_counts() {
    let mut model = quota_two_account_view_model();
    model.header.observed_clients = Some(4294967295);
    model.header.pool = super::super::quota_status_symbols::QuotaPoolCensus {
        usable: 2,
        reserve: 3,
        blocked: 4,
        unknown: 5,
        excluded: 0,
    };
    model.header.freshness = super::super::quota_status_symbols::QuotaFreshnessCensus {
        fresh: 6,
        stale: 7,
        unknown: 8,
    };
    let frames = render_quota_filter_phases(
        model,
        120,
        36,
        vec![
            ("Quota status", vec![provider_filter_key(KeyCode::F(1))]),
            ("Quota Help", vec![provider_filter_key(KeyCode::Esc)]),
            ("Filters", vec![provider_filter_key(KeyCode::Esc)]),
        ],
        None,
    )
    .await;
    let help = frames
        .iter()
        .find(|frame| frame.contains("Quota Help"))
        .expect("real contextual Help opened");
    let help = actual_dialog_text(help, "Quota Help")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for count in [
        "Accounts 14",
        "Usable 2",
        "Reserve 3",
        "Blocked 4",
        "Pool unknown 5",
        "Excluded 0",
        "Clients 4294967295",
        "Fresh 6",
        "Stale 7",
        "Data unknown 8",
    ] {
        assert!(
            help.contains(count),
            "current count {count} is missing: {help}"
        );
    }
}

#[test]
fn actual_dialog_crop_rejects_missing_modal_bottom_with_background_facts() {
    let frame = "╭──────────────────────────────────╮\n│ Use credits Disallow              │\n│   ╭──────────────────────╮       │\n│   │ Quota Help           │       │\n│   │ Close                │       │\n╰──────────────────────────────────╯";
    assert!(
        std::panic::catch_unwind(|| actual_dialog_text(frame, "Quota Help")).is_err(),
        "a modal without its bottom must not borrow Use credits/Disallow from the outer frame"
    );
}

#[test]
fn actual_dialog_crop_rejects_mismatched_modal_bottom_with_background_facts() {
    let frame = "╭──────────────────────────────────╮\n│ Use credits Disallow              │\n│   ╭──────────────────────╮       │\n│   │ Quota Help           │       │\n│   │ Close                │       │\n│  ╰───────────────────────╯       │\n╰──────────────────────────────────╯";
    assert!(
        std::panic::catch_unwind(|| actual_dialog_text(frame, "Quota Help")).is_err(),
        "a displaced bottom is not the modal's boundary"
    );
}

#[test]
fn actual_dialog_crop_keeps_modal_content_without_background_facts() {
    let frame = "╭──────────────────────────────────╮\n│ Use credits Disallow              │\n│   ╭──────────────────────╮       │\n│   │ Quota Help           │       │\n│   │ Close                │       │\n│   ╰──────────────────────╯       │\n╰──────────────────────────────────╯";
    let dialog = actual_dialog_text(frame, "Quota Help");
    assert!(dialog.contains("Quota Help") && dialog.contains("Close"));
    assert!(!dialog.contains("Use credits") && !dialog.contains("Disallow"));
}

#[test]
fn actual_dialog_crop_preserves_cjk_overwritten_top_left_corner() {
    // The second wide glyph covers the modal's left corner at column four.
    let frame = "╭──────────────────────────────────╮\n│ Use credits Disallow              │\n│東京──────────────────────╮       │\n│   │ Quota Help           │       │\n│   │ Close                │       │\n│   ╰──────────────────────╯       │\n╰──────────────────────────────────╯";
    let dialog = actual_dialog_text(frame, "Quota Help");
    assert!(dialog.contains("Quota Help") && dialog.contains("Close"));
    assert!(!dialog.contains("Use credits") && !dialog.contains("Disallow"));
}

pub(in crate::presentation::quota) fn actual_dialog_text(frame: &str, title: &str) -> String {
    let lines = frame.lines().collect::<Vec<_>>();
    let title_row = lines
        .iter()
        .position(|line| line.contains(title))
        .expect("dialog title is visible");
    let column_of = |line: &str, marker: char| {
        line.find(marker).map(|offset| {
            unicode_width::UnicodeWidthStr::width(
                line.get(..offset)
                    .expect("found marker is a UTF-8 boundary"),
            )
        })
    };
    let title_start = unicode_width::UnicodeWidthStr::width(
        lines[title_row]
            .get(..lines[title_row].find(title).unwrap())
            .expect("found title is a UTF-8 boundary"),
    );
    let title_end = title_start + unicode_width::UnicodeWidthStr::width(title);
    // Bind the nearest top border before looking below the title. A wide
    // underlay glyph can cover its left corner; the contiguous horizontal
    // border ending at the top-right corner still fixes that cell's column.
    let (top, left, right) = (0..title_row)
        .rev()
        .find_map(|row| {
            let corner_offset = lines[row].find('╮')?;
            let right = column_of(lines[row], '╮')?;
            let horizontal_cells = lines[row]
                .get(..corner_offset)?
                .chars()
                .rev()
                .take_while(|character| *character == '─')
                .count();
            let left = right.checked_sub(horizontal_cells + 1)?;
            (horizontal_cells > 0 && left < title_start && right >= title_end)
                .then_some((row, left, right))
        })
        .expect("modal top border independently encloses its title");
    let bottom = (title_row + 1..lines.len())
        .find(|row| {
            column_of(lines[*row], '╰') == Some(left) && column_of(lines[*row], '╯') == Some(right)
        })
        .expect("modal bottom border matches its independently bound top");
    lines[top + 1..bottom]
        .iter()
        .map(|line| {
            line.chars()
                .scan(0usize, |column, character| {
                    let start = *column;
                    *column += unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
                    Some((start, character))
                })
                .filter(|(column, _)| *column > left && *column < right)
                .map(|(_, character)| if character == '┃' { ' ' } else { character })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn clarity_control_key(character: char) -> KeyEvent {
    let mut key = KeyEvent::new(KeyEventKind::Press, KeyCode::Char(character));
    key.modifiers = KeyModifiers::CONTROL;
    key
}

#[path = "quota_ux_reset_test.rs"]
mod quota_ux_reset_test;

#[path = "quota_ux_form_test.rs"]
mod quota_ux_form_test;

#[path = "quota_ux_choice_test.rs"]
mod quota_ux_choice_test;

#[path = "quota_ux_frame_test.rs"]
mod quota_ux_frame_test;

#[path = "quota_ux_pace_test.rs"]
mod quota_ux_pace_test;

#[path = "quota_ux_browse_scroll_test.rs"]
mod quota_ux_browse_scroll_test;

#[path = "quota_ux_options_entry_test.rs"]
mod quota_ux_options_entry_test;

#[path = "quota_ux_help_test.rs"]
pub(in crate::presentation::quota) mod quota_ux_help_test;

#[path = "quota_ux_inventory_caption_test.rs"]
mod quota_ux_inventory_caption_test;

#[path = "quota_ux_identity_test.rs"]
mod quota_ux_identity_test;
