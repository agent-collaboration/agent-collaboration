//! Owner 71efc57bb/873f62923, refined by a12f9c2db: interactive window facts only.
use super::*;
use iocraft::Canvas;

fn account() -> QuotaStatusAccountViewModel {
    crate::quota_browse_presentation_test::quota_two_account_view_model()
        .rows
        .remove(0)
}

fn known(percent: u32, stale: bool) -> QuotaDisplayWindow {
    QuotaDisplayWindow::Known {
        remaining_percent: percent,
        reset_in_seconds: Some(572400),
        stale,
    }
}

fn render(row: &QuotaStatusAccountViewModel, width: usize) -> Canvas {
    render_account_groups(row, width, false).render(None)
}

#[test]
fn account_third_line_shows_login_independently_from_routing_and_quota() {
    let mut row = account();
    row.status = codex_router_quota_service::quota_display_facts::QuotaRowStatus::Account(
        codex_router_quota_service::quota_display_facts::QuotaAccountStatus::Blocked,
    );
    row.display_weekly = known(42, true);
    let canvas = render(&row, 120);
    let frame = canvas.to_string();
    let lines = frame.lines().collect::<Vec<_>>();
    assert!(lines[2].contains("Signed in"), "{frame}");
    assert!(!lines[2].contains("blocked"), "{frame}");
    assert!(
        lines[3].contains("blocked"),
        "routing stays visible: {frame}"
    );
    assert!(frame.contains("Weekly · stale"), "{frame}");
    assert!(!row.details.status.is_empty(), "routing remains in Details");
}

#[test]
fn narrow_account_column_wraps_login_and_preserves_separate_routing() {
    let mut row = account();
    row.login_state = codex_router_quota_service::quota_display_model::QuotaLoginState::Required;
    row.status = codex_router_quota_service::quota_display_facts::QuotaRowStatus::Account(
        codex_router_quota_service::quota_display_facts::QuotaAccountStatus::Blocked,
    );
    for width in [17_u32, 12] {
        let canvas = element!(View(width, flex_direction:FlexDirection::Column) {
            #(account_group(&row, false))
        })
        .render(None);
        let frame = canvas.to_string();
        let words = frame.split_whitespace().collect::<Vec<_>>().join(" ");
        let login = words
            .find("Sign-in required")
            .expect("wrapped login words remain visible");
        let routing = words.find("blocked").expect("routing remains visible");
        assert!(login < routing, "{width}: {frame}");
        assert!(canvas.width() <= width as usize, "{width}: {frame}");
    }
}

fn quota_lines(canvas: &Canvas) -> Vec<String> {
    let frame = canvas.to_string();
    let lines = frame.lines().collect::<Vec<_>>();
    let start = lines
        .iter()
        .position(|line| line.trim() == "Quota left · reset in")
        .unwrap();
    assert!(
        lines[start + 1]
            .chars()
            .all(|character| character == '─' || character == ' '),
        "actual heading rule: {frame}"
    );
    let start = start + 2;
    let end = lines
        .iter()
        .position(|line| line.trim() == "Weekly pace")
        .unwrap();
    lines[start..end]
        .iter()
        .map(|line| line.trim_end().to_owned())
        .collect()
}

fn assert_line_color(canvas: &Canvas, label: &str, expected: Color) {
    let frame = canvas.to_string();
    let (row, line) = frame
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(label))
        .unwrap();
    let column = unicode_width::UnicodeWidthStr::width(line.split_once(label).unwrap().0);
    for offset in 0..unicode_width::UnicodeWidthStr::width(label) {
        assert_eq!(
            canvas
                .cell(column + offset, row)
                .unwrap()
                .text_style()
                .unwrap()
                .color,
            Some(expected),
            "{label}: {frame}"
        );
    }
}

#[test]
fn known_weekly_uses_two_lines_and_excludes_the_reported_short_window() {
    let mut row = account();
    row.display_weekly = known(42, false);
    row.display_short = known(83, false);
    row.weekly_window = "DO NOT PARSE THE DISPLAY STRING".to_owned();
    let canvas = render(&row, 29);
    assert_eq!(
        quota_lines(&canvas),
        [
            format!("{}{} 42%", "█".repeat(10), "░".repeat(15)),
            "Weekly · 6d 15h".to_owned()
        ]
    );
    assert!(
        !canvas.to_string().contains("83%")
            && !canvas
                .to_string()
                .lines()
                .any(|line| line.trim_start().starts_with("5h ·") || line.contains("· 5h")),
        "{canvas}"
    );
}

#[test]
fn missing_weekly_falls_back_to_the_real_short_window_without_weekly_label() {
    let mut row = account();
    row.display_weekly = QuotaDisplayWindow::Unknown;
    row.display_short = QuotaDisplayWindow::Known {
        remaining_percent: 83,
        reset_in_seconds: Some(11520),
        stale: false,
    };
    let canvas = render(&row, 29);
    assert_eq!(
        quota_lines(&canvas),
        [
            format!("{}{} 83%", "█".repeat(20), "░".repeat(5)),
            "5h · 3h 12m".to_owned()
        ]
    );
    assert!(
        !quota_lines(&canvas)
            .iter()
            .any(|line| line.contains("Weekly")),
        "{canvas}"
    );
}

#[test]
fn unknown_windows_keep_an_explicit_muted_weekly_state_without_fabricated_amount() {
    let mut row = account();
    row.display_weekly = QuotaDisplayWindow::Unknown;
    row.display_short = QuotaDisplayWindow::Unknown;
    let canvas = render(&row, 29);
    assert_eq!(
        quota_lines(&canvas),
        [
            format!("{} ?%", "░".repeat(26)),
            "Weekly · unknown".to_owned()
        ]
    );
    assert_line_color(&canvas, "Weekly · unknown", Color::Grey);
}

#[test]
fn stale_weekly_replaces_the_countdown_with_the_warning_state() {
    let mut row = account();
    row.display_weekly = known(42, true);
    row.display_short = known(83, false);
    let canvas = render(&row, 29);
    assert_eq!(
        quota_lines(&canvas),
        [
            format!("{}{} 42%", "█".repeat(10), "░".repeat(15)),
            "Weekly · stale".to_owned()
        ]
    );
    assert_line_color(
        &canvas,
        "Weekly · stale",
        Color::Rgb {
            r: 255,
            g: 165,
            b: 0,
        },
    );
}

#[test]
fn the_bar_shrinks_before_the_percentage_or_weekly_caption_at_narrow_widths() {
    let mut row = account();
    row.display_weekly = known(100, false);
    for (width, bar_cells) in [(29usize, 24usize), (22, 17)] {
        let canvas = render(&row, width);
        assert_eq!(
            quota_lines(&canvas),
            [
                format!("{} 100%", "█".repeat(bar_cells)),
                "Weekly · 6d 15h".to_owned()
            ]
        );
        assert_eq!(
            unicode_width::UnicodeWidthStr::width(quota_lines(&canvas)[0].as_str()),
            width
        );
    }
}
