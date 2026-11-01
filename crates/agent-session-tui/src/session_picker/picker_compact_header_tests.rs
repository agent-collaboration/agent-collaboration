// R13: toolbar/Menu replace the old filter summary and header actions.
//! Compact chrome must leave the viewport to records, while active state stays visible.
use super::*;
use crate::session_picker::provider_filter_tests::mixed_request;

#[cfg(feature = "quota-reset-test-harness")]
#[tokio::test]
async fn wide_glyph_storyboard_variant_seeds_the_measured_left_border_rows() {
    use crate::session_picker::provider_filter_storyboard_fixture::{
        apply_storyboard_variant, provider_filter_storyboard_request,
    };
    use unicode_width::UnicodeWidthStr;
    let request = apply_storyboard_variant(
        provider_filter_storyboard_request().unwrap(),
        Some("wide-glyph-left-border"),
    )
    .unwrap();
    let frame = render_picker_capture_at(request.clone(), 120, 36, vec![ctrl_key('d')]).await;
    let mut overlay_bounds = Vec::new();
    for (title, opening) in [
        ("New session", ctrl_key('n')),
        (
            "Session details",
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::F(4))),
        ),
    ] {
        let overlay =
            render_picker_capture_at(request.clone(), 120, 36, vec![opening, ctrl_key('d')]).await;
        overlay_bounds.push((title, dialog_bounds(&overlay, title)));
    }
    for glyph in ["📂", "✉️", "界"] {
        let rows = frame
            .lines()
            .enumerate()
            .filter_map(|(row, line)| {
                line.split_once(glyph)
                    .map(|(prefix, _)| (row, UnicodeWidthStr::width(prefix)))
            })
            .collect::<Vec<_>>();
        assert!(
            rows.iter().any(|(row, column)| *row >= 9 && *column == 15),
            "{glyph} must start at overlay-left minus one on a covered title row: {rows:?}\n{frame}"
        );
        for (title, bounds) in &overlay_bounds {
            assert!(
                rows.iter().any(|(row, column)| *row > bounds.top
                    && *row < bounds.bottom
                    && *column + 1 == bounds.left),
                "{glyph} must seed a covered row of {title}: {rows:?}\n{frame}"
            );
        }
    }
    // Row 8 is a separator. This fixture earns left-border coverage, not a corner claim.
    assert!(!frame.lines().nth(8).unwrap().contains("boundary-"));
}

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
        let capture =
            render_counted_picker_capture_at(compact_request(), width, height, vec![ctrl_key('d')])
                .await;
        assert_eq!((capture.visible_records, capture.total_records), (5, 5));
        assert_eq!(
            capture.committed_provider_filter,
            Some(SessionProviderFilter::All)
        );
        let frame = capture.text;
        // R13: count captions shrink before controls; the compact count stays.
        if width >= 76 {
            assert!(
                frame
                    .lines()
                    .any(|line| line.contains("5/5 sessions · This machine")),
                "machine and count share one row at {width}: {frame}"
            );
        } else {
            assert!(
                frame
                    .lines()
                    .any(|line| line.contains("Menu") && line.contains("Filter")),
                "{frame}"
            );
        }
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
    let capture = render_counted_picker_capture_at(
        compact_request(),
        48,
        24,
        vec![key('B'), key('e'), key('t'), key('a'), ctrl_key('d')],
    )
    .await;
    assert_eq!((capture.visible_records, capture.total_records), (1, 5));
    let frame = capture.text;
    assert!(
        frame.contains("Beta") && frame.contains("Native Beta"),
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
        let capture =
            render_counted_picker_capture_at(request, width, height, vec![ctrl_key('d')]).await;
        assert_eq!((capture.visible_records, capture.total_records), (6, 6));
        assert_eq!(capture.machine_label, "This machine");
        let frame = capture.text;
        if width <= 40 {
            println!("phone chrome {width}x{height}\n{frame}");
        }
        let chrome = frame.split('┌').next().expect("picker chrome before list");
        if width >= 76 {
            assert!(chrome.contains("6/6 sessions · This machine"), "{frame}");
        }
        assert!(
            chrome.contains("6/6"),
            "R13 keeps the compact count at {width}: {frame}"
        );
        assert!(
            chrome
                .lines()
                .any(|line| line.contains("Menu") && (width < 43 || line.contains("New"))),
            "whole action words at {width}: {frame}"
        );
        for label in ["Filter", "Menu"] {
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
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    for _ in 0..5 {
        sender
            .send(TerminalEvent::Key(KeyEvent::new(
                KeyEventKind::Press,
                KeyCode::Down,
            )))
            .unwrap();
    }
    let mut component = element!(SessionsPickerComponent(request,width:32usize,height:24usize));
    let mut frame = String::new();
    let mut exited = false;
    let journey = component
        .mock_terminal_render_loop(MockTerminalConfig::with_events(events))
        .map(|canvas| {
            frame = canvas.to_string();
            // R3: wait for actual scroll reveal before ending the fixture.
            if !exited
                && frame
                    .lines()
                    .any(|line| line.contains("Last session") && line.contains('❯'))
            {
                sender.send(ctrl_key('d')).unwrap();
                exited = true;
            }
        })
        .collect::<Vec<_>>();
    tokio::time::timeout(Duration::from_secs(2), journey)
        .await
        .expect("selected last row must be revealed");
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
        "Help reachable",
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
            let mut help_step = "Details";
            let mut help_frame = String::new();
            let (frame, help_reachable) = tokio::time::timeout(Duration::from_secs(2), async {
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
                // R16, admitted adaptive-header delta: the hint may disappear,
                // but F1 from this exact pose and the main Menu still expose Help.
                sender
                    .send(TerminalEvent::Key(KeyEvent::new(
                        KeyEventKind::Press,
                        KeyCode::F(1),
                    )))
                    .unwrap();
                help_step = "F1 Help from Details";
                loop {
                    let help = next_current_canvas(canvases.as_mut())
                        .await
                        .unwrap()
                        .expect("Help from captured Details pose")
                        .to_string();
                    help_frame = help.clone();
                    if help.contains("Close Help") {
                        break;
                    }
                }
                for code in [KeyCode::Esc, KeyCode::Esc, KeyCode::F(10)] {
                    sender
                        .send(TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code)))
                        .unwrap();
                }
                help_step = "Menu Help after return";
                let mut menu_visible = false;
                let help_reachable = loop {
                    let menu = next_current_canvas(canvases.as_mut())
                        .await
                        .unwrap()
                        .expect("Menu Help after returning from Details")
                        .to_string();
                    help_frame = menu.clone();
                    if !menu.contains("Close Help")
                        && !menu.contains("Session details")
                        && menu.contains("Help")
                    {
                        break true;
                    }
                    menu_visible |= !menu.contains("Close Help")
                        && !menu.contains("Session details")
                        && menu.contains("New session");
                    if menu_visible {
                        // The narrow menu has a scrollbar; inspect its last rows.
                        sender
                            .send(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                                MouseEventKind::ScrollDown,
                                3,
                                4,
                            )))
                            .unwrap();
                    }
                };
                (frame, help_reachable)
            })
            .await
            .unwrap_or_else(|error| panic!("bounded Details component navigation at {width}x{height}, Down={down_count}, step={help_step}: {error}; {help_frame}"));
            // Only the captured modal may prove facts, never browse text behind it.
            let dialog_text = dialog_text(&frame, dialog_bounds(&frame, "Session details"));
            if height > 12 {
                let pinned_context = dialog_text
                    .lines()
                    .skip_while(|line| !line.contains("Session details"))
                    .take_while(|line| !line.trim().is_empty())
                    .collect::<String>()
                    .split_whitespace()
                    .collect::<String>();
                assert!(
                    pinned_context.contains("ClaudeGamma"),
                    "adaptive header must pin the full name at {width}x{height}, Down={down_count}: {frame}"
                );
                assert!(
                    pinned_context.contains("[000.0011]"),
                    "adaptive header must retain the whole short ID tag: {frame}"
                );
            }
            let compact = dialog_text
                .chars()
                .filter(|character| !character.is_whitespace() && !matches!(character, '│' | '┃'))
                .collect::<String>();
            let visible: [bool; 10] = std::array::from_fn(|index| {
                if index == 1 {
                    help_reachable
                } else {
                    compact.contains(&required[index].split_whitespace().collect::<String>())
                }
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
