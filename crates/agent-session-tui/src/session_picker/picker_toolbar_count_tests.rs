//! R13 owner rule: design 5bafadfff; count and filter stay at every supported grid.
use super::*;

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
