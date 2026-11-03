//! R13 owner rule: design 5bafadfff; count and filter stay at every supported grid.
use crate::session_picker::test_support::*;
use unicode_width::UnicodeWidthStr;

#[tokio::test]
async fn toolbar_status_keeps_one_blank_cell_before_the_right_border_at_all_four_grids() {
    let mut request = picker_request();
    request.root = SessionsRoot::Any;
    request.records = (0..6)
        .map(|index| {
            picker_record(
                &format!("toolbar-padding-{index}"),
                "Padding session",
                "/repo/project-a",
                "codex-router",
                "cli",
            )
        })
        .collect();

    for (width, height) in [(120usize, 36usize), (76, 30), (48, 12), (32, 24)] {
        let frame =
            render_picker_capture_at(request.clone(), width, height, vec![ctrl_key('d')]).await;
        let toolbar = frame.lines().find(|row| row.contains("Menu F10")).unwrap();
        assert!(
            toolbar.contains("Filter") && toolbar.contains("6/6"),
            "{frame}"
        );
        assert_eq!(
            toolbar.contains("6/6 sessions · This machine"),
            width >= 76,
            "{width}x{height}: {frame}"
        );
        // Display width anchors the border cell; the literal space is the
        // independent contract for the cell immediately to its left.
        assert_eq!(toolbar.width(), width, "{width}x{height}: {frame}");
        assert!(
            toolbar.ends_with(" │"),
            "toolbar right padding cell at column {} is not blank at {width}x{height}: {frame}",
            width - 2,
        );
    }
}

#[tokio::test]
async fn compact_toolbar_count_stays_on_the_actual_row_at_all_four_grids() {
    for (width, height) in [(120usize, 36usize), (76, 30), (48, 12), (32, 24)] {
        let frame =
            render_picker_capture_at(picker_request(), width, height, vec![ctrl_key('d')]).await;
        let toolbar = frame.lines().find(|row| row.contains("Menu F10")).unwrap();
        assert!(
            toolbar.contains("1/3"),
            "R13 count missing at {width}x{height}: {frame}"
        );
        assert!(
            toolbar.contains("Filter"),
            "R13 Filter dropped at {width}x{height}: {frame}"
        );
        assert_eq!(
            toolbar.contains("sessions"),
            width >= 76,
            "{width}x{height}: {frame}"
        );
        assert_eq!(
            toolbar.contains("This machine"),
            width >= 76,
            "{width}x{height}: {frame}"
        );
        assert_eq!(
            toolbar.contains("+ New"),
            width >= 43,
            "{width}x{height}: {frame}"
        );
        assert_eq!(
            toolbar.contains("Ctrl+N"),
            width >= 50,
            "{width}x{height}: {frame}"
        );
    }
}
