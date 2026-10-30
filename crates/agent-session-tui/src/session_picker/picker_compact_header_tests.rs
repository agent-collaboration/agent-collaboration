//! Compact chrome must leave the viewport to records, while active state stays visible.
use super::*;
use crate::session_picker::provider_filter_tests::mixed_request;

fn compact_request() -> SessionsPickerRequest {
    let mut request = mixed_request();
    // Literal ordering makes the two rows under test the first real records.
    request.records[0].recency_at_ms = Some(9_000);
    request.records[1].recency_at_ms = Some(8_000);
    request
}

#[tokio::test]
async fn compact_header_machine_and_provider_share_one_row_without_idle_settings() {
    for (width, height) in [(120, 36), (76, 30), (48, 24)] {
        let frame =
            render_picker_capture_at(compact_request(), width, height, vec![ctrl_key('d')]).await;
        assert!(
            frame
                .lines()
                .any(|line| line.contains("This machine") && line.contains("All providers")),
            "machine/provider must share one row at {width}: {frame}"
        );
        assert!(
            frame.contains("Sessions") && frame.contains("Sessions · 5/5"),
            "short title and count: {frame}"
        );
        for idle in ["Search: []", "View: [All]", "Sort: [updated]"] {
            assert!(
                !frame.contains(idle),
                "idle setting consumes a row: {frame}"
            );
        }
        for name in ["Native Alpha", "Native Beta"] {
            assert!(
                frame.contains(name),
                "compact chrome must leave multiple real records visible: {frame}"
            );
        }
        assert!(frame.lines().count() <= height, "{frame}");
    }
}

#[tokio::test]
async fn compact_header_active_search_stays_visible_and_escape_restores_rows() {
    let key = |character| {
        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Char(character)))
    };
    let frame = render_picker_capture_at(
        compact_request(),
        48,
        24,
        vec![key('B'), key('e'), key('t'), key('a'), ctrl_key('d')],
    )
    .await;
    assert!(
        frame.contains("Search: [Beta]") && frame.contains("Native Beta"),
        "{frame}"
    );
    assert!(!frame.contains("Native Alpha"), "{frame}");
    let restored = render_picker_capture_at(
        compact_request(),
        48,
        24,
        vec![
            key('B'),
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Esc)),
            ctrl_key('d'),
        ],
    )
    .await;
    assert!(
        !restored.contains("Search: []") && restored.contains("Native Alpha"),
        "{restored}"
    );
}

#[test]
fn compact_header_text_respects_terminal_cell_width_for_wide_machine_names() {
    use unicode_width::UnicodeWidthStr;
    // CJK letters occupy two terminal columns; a character count is not a cell budget.
    for width in [16, 7, 5, 1, 0] {
        let rendered = fit_line("日本語の長いマシン名", width);
        assert!(
            UnicodeWidthStr::width(rendered.as_str()) <= width,
            "wide alias consumes the shortcut's cells at width {width}: {rendered}"
        );
    }
    assert_eq!(fit_line("This machine", 16), "This machine");
}

#[cfg(feature = "quota-reset-test-harness")]
#[tokio::test]
async fn phone_chrome_preserves_counts_actions_controls_and_selected_name() {
    for (width, height) in [(32, 24), (40, 24), (48, 12), (48, 24), (76, 30), (120, 36)] {
        let request = crate::session_picker::provider_filter_storyboard_fixture::provider_filter_storyboard_request()
            .expect("valid synthetic six-session inventory");
        let frame = render_picker_capture_at(request, width, height, vec![ctrl_key('d')]).await;
        if width <= 40 {
            println!("phone chrome {width}x{height}\n{frame}");
        }
        let chrome = frame.split('┌').next().expect("picker chrome before list");
        assert!(
            chrome
                .lines()
                .any(|line| line.contains("Sessions · 6/6") && line.contains("Help")),
            "whole title, count and Help at {width}: {frame}"
        );
        assert!(
            chrome
                .lines()
                .any(|line| line.contains("New") && line.contains("Details")),
            "whole action words at {width}: {frame}"
        );
        for label in ["This machine", "All providers"] {
            assert!(
                chrome.contains(label),
                "fitting control {label} at {width}: {frame}"
            );
        }
        let selected = frame
            .lines()
            .find(|line| line.contains("Codex Alpha"))
            .expect("fitting primary session name must remain whole");
        assert!(
            selected.contains('❯'),
            "selection stays with identity at {width}: {frame}"
        );
        assert!(
            frame.lines().count() <= height,
            "physical row budget at {width}: {frame}"
        );
    }
}

#[cfg(feature = "quota-reset-test-harness")]
#[tokio::test]
async fn phone_chrome_keeps_the_last_focused_session_in_the_physical_window() {
    let mut request = crate::session_picker::provider_filter_storyboard_fixture::provider_filter_storyboard_request()
        .expect("valid synthetic six-session inventory");
    let last = request.records.last_mut().expect("last session");
    last.title = "Last session".to_owned();
    last.full_title = "Last session".to_owned();
    let mut events = (0..5)
        .map(|_| TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Down)))
        .collect::<Vec<_>>();
    events.push(ctrl_key('d'));
    let frame = render_picker_capture_at(request, 32, 24, events).await;
    println!("phone last focused session 32x24\n{frame}");
    assert!(
        frame
            .lines()
            .any(|line| line.contains("Last session") && line.contains('❯')),
        "last focused identity must remain whole and visible: {frame}"
    );
    assert!(frame.lines().count() <= 24, "physical row budget: {frame}");
}

#[cfg(feature = "quota-reset-test-harness")]
#[tokio::test]
async fn compact_details_facts_remain_reachable_with_captured_context_at_all_grids() {
    let required_markers = [
        "Session details",
        "Help",
        "Claude Gamma",
        "Session ID",
        "Endpoint",
        "State",
        "Close",
    ];
    for (width, height) in [
        (32usize, 24usize),
        (40, 24),
        (48, 12),
        (48, 24),
        (76, 30),
        (120, 36),
    ] {
        let mut reachable = [false; 10];
        let mut earliest_common = None;
        for down_count in 0..=16 {
            let request = crate::session_picker::provider_filter_storyboard_fixture::provider_filter_storyboard_request()
                .expect("valid synthetic six-session inventory");
            let captured = request
                .records
                .iter()
                .find(|record| record.title.starts_with("Claude Gamma"))
                .expect("same captured Gamma fixture");
            let full_title = if captured.full_title.is_empty() {
                captured.title.clone()
            } else {
                captured.full_title.clone()
            };
            let required = required_markers
                .iter()
                .map(|marker| (*marker).to_owned())
                .chain([
                    full_title,
                    captured.cwd.clone().expect("fixture path"),
                    captured.session_id.clone(),
                ])
                .collect::<Vec<_>>();
            let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
            let events = futures_util::stream::unfold(receiver, |mut receiver| async move {
                receiver.recv().await.map(|event| (event, receiver))
            });
            let mut component = element!(SessionsPickerComponent(request, width, height));
            let canvases =
                component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
            tokio::pin!(canvases);
            let frame = tokio::time::timeout(Duration::from_secs(2), async {
                sender.send(ctrl_key('f')).unwrap();
                for code in [
                    KeyCode::Enter,
                    KeyCode::Down,
                    KeyCode::Down,
                    KeyCode::Enter,
                    KeyCode::Tab,
                    KeyCode::Tab,
                    KeyCode::Tab,
                    KeyCode::Tab,
                    KeyCode::Tab,
                    KeyCode::Tab,
                    KeyCode::Enter,
                ] {
                    sender
                        .send(TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code)))
                        .unwrap();
                }
                sender
                    .send(TerminalEvent::Key(KeyEvent::new(
                        KeyEventKind::Press,
                        KeyCode::F(4),
                    )))
                    .unwrap();
                let mut frame = loop {
                    let frame = next_current_canvas(canvases.as_mut())
                        .await
                        .unwrap()
                        .expect("Details render")
                        .to_string();
                    if frame.contains("Session details") {
                        break frame;
                    }
                };
                if down_count > 0 {
                    for _ in 0..down_count {
                        sender
                            .send(TerminalEvent::Key(KeyEvent::new(
                                KeyEventKind::Press,
                                KeyCode::Down,
                            )))
                            .unwrap();
                    }
                    frame = next_current_canvas(canvases.as_mut())
                        .await
                        .unwrap()
                        .expect("scrolled Details")
                        .to_string();
                }
                frame
            })
            .await
            .expect("bounded Details component navigation");
            // Only the captured modal may prove facts, never browse text behind it.
            let dialog_text = dialog_text(&frame, dialog_bounds(&frame, "Session details"));
            let compact = dialog_text
                .chars()
                .filter(|character| !character.is_whitespace() && !matches!(character, '│' | '┃'))
                .collect::<String>();
            let visible: [bool; 10] = std::array::from_fn(|index| {
                compact.contains(&required[index].split_whitespace().collect::<String>())
            });
            for (seen, present) in reachable.iter_mut().zip(visible) {
                *seen |= present;
            }
            if visible[..7].iter().all(|present| *present) && earliest_common.is_none() {
                earliest_common = Some(down_count);
            }
            println!(
                "DETAILS_POSE {width}x{height} Down={down_count} visible={visible:?}\n{frame}"
            );
            assert!(
                visible[0] && visible[1] && visible[2] && visible[6],
                "captured context/chrome/actions at {width}x{height} Down={down_count}: {frame}"
            );
            assert!(
                frame.lines().count() <= height,
                "physical viewport at {width}x{height}: {frame}"
            );
            if height > 12 && earliest_common.is_some() && reachable.iter().all(|seen| *seen) {
                break;
            }
        }
        println!(
            "DETAILS_REACHABILITY {width}x{height} earliest_common={earliest_common:?} reachable={reachable:?}"
        );
        assert_details_pose_coverage(width, height, reachable, earliest_common);
    }
}

#[cfg(feature = "quota-reset-test-harness")]
fn assert_details_pose_coverage(
    width: usize,
    height: usize,
    reachable: [bool; 10],
    earliest_common: Option<usize>,
) {
    assert!(
        (width == 48 && height == 12) || earliest_common.is_some(),
        "seven common Details facts must share one pose at {width}x{height}"
    );
    assert!(
        reachable.iter().all(|seen| *seen),
        "all old facts and full title/path/ID must be reachable at {width}x{height}: {reachable:?}"
    );
}

#[cfg(feature = "quota-reset-test-harness")]
#[test]
#[should_panic(expected = "seven common Details facts must share one pose")]
fn details_coverage_rejects_individually_reachable_facts_without_common_pose() {
    assert_details_pose_coverage(32, 24, [true; 10], None);
}

#[cfg(feature = "quota-reset-test-harness")]
#[test]
fn details_coverage_preserves_the_exact_short_grid_exception() {
    assert_details_pose_coverage(48, 12, [true; 10], None);
}
