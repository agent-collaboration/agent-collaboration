//! Consumer presentation obligations from the Lead's 2026-10-30 taste review.
//! Authority: Router push 01a18f3c-1da7-7482-87a6-cba9e41bf700, items 1–8.
use super::*;

fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}

#[tokio::test]
async fn settings_menu_does_not_repeat_cycle_entries_and_help_precedes_quit() {
    use agent_collaboration_tui::{
        controls::ActionProjections,
        surfaces::{surface_handle::SurfaceStackHandle, surface_stack::OverlayKind},
    };
    let observed = Arc::new(Mutex::new(None));
    let observer = observed.clone();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    sender.send(key(KeyCode::F(10))).unwrap();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut picker = element!(SessionsPickerComponent(request:picker_request(),width:120usize,height:36usize,
        surface_observer:move |(stack,_): (SurfaceStackHandle,State<SessionsPickerModel>)|{*observer.lock().unwrap()=Some(stack)}));
    let frames = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let frame = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(canvas) = frames.next().await {
            let frame = canvas.to_string();
            let stack = observed.lock().unwrap().unwrap();
            if stack
                .snapshot()
                .overlays()
                .last()
                .is_some_and(|overlay| matches!(overlay.kind, OverlayKind::Menu))
                && frame.contains("Group by F6")
            {
                return frame;
            }
        }
        panic!("picker ended before the settings menu");
    })
    .await
    .expect("settings sections publish after F10");
    let normalized = frame.split_whitespace().collect::<Vec<_>>().join(" ");
    for duplicate in ["Scope: all", "Sort: updated", "Runtime: All Ctrl+T"] {
        assert!(!normalized.contains(duplicate), "{frame}");
    }
    let item_count = {
        let stack = observed.lock().unwrap().unwrap();
        let snapshot = stack.snapshot();
        ActionProjections::from_registry(&snapshot.top().actions)
            .menu
            .len()
    };
    for _ in 1..item_count {
        sender.send(key(KeyCode::Down)).unwrap();
    }
    let frame = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(canvas) = frames.next().await {
            let frame = canvas.to_string();
            if frame.contains("Help") && frame.contains("Quit") {
                return frame;
            }
        }
        panic!("picker ended before Help and Quit");
    })
    .await
    .expect("keyboard navigation reveals Help and Quit");
    assert!(
        frame.find("Help").unwrap() < frame.find("Quit").unwrap(),
        "{frame}"
    );
    sender.send(ctrl_key('d')).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while frames.next().await.is_some() {}
    })
    .await
    .expect("picker exits after Quit");
}

#[tokio::test]
async fn new_buttons_are_plain_and_one_action_hint_line_follows_them() {
    for (width, height) in [(120, 36), (76, 30), (48, 12), (32, 24)] {
        let frame = render_picker_capture_at(
            picker_request(),
            width,
            height,
            vec![ctrl_key('n'), ctrl_key('d')],
        )
        .await;
        assert_eq!(frame.matches("Ctrl+Enter").count(), 1, "{frame}");
        assert_eq!(frame.matches("Esc").count(), 1, "{frame}");
        assert!(
            frame.contains("Start") && frame.contains("Cancel"),
            "{frame}"
        );
        let rows = frame.lines().collect::<Vec<_>>();
        let hint = rows
            .iter()
            .position(|row| row.contains("Ctrl+Enter"))
            .unwrap();
        let close_hint = rows.iter().position(|row| row.contains("Esc")).unwrap();
        assert!(
            rows[hint].contains("Start"),
            "the start key/title group stays whole: {frame}"
        );
        assert!(
            rows[close_hint].contains("Cancel"),
            "the close key/title group stays whole: {frame}"
        );
        // Lead01a19630: a narrow hint line wraps only between whole groups.
        assert_eq!(
            close_hint > hint,
            width == 32,
            "hint group wrapping: {frame}"
        );
        if close_hint > hint {
            assert!(
                !rows[hint..=close_hint].iter().any(|row| row.contains('·')),
                "wrapped groups omit the separator: {frame}"
            );
        }
        assert!(
            rows[..hint].iter().any(|row| row.contains("Start")
                && row.contains("Cancel")
                && !row.contains("Ctrl+Enter")
                && !row.contains("Esc")),
            "plain buttons precede the one hint line: {frame}"
        );
    }
}

#[tokio::test]
async fn narrow_toolbar_keeps_menu_and_filter_on_one_row() {
    for width in [32, 40, 48, 76, 120] {
        let frame =
            render_picker_capture_at(picker_request(), width, 36, vec![ctrl_key('d')]).await;
        let toolbar = frame.lines().find(|line| line.contains("Menu")).unwrap();
        assert!(toolbar.contains("Filter"), "{width}: {frame}");
        assert_eq!(toolbar.contains("New"), width >= 43, "{width}: {frame}");
        assert_eq!(toolbar.contains("Ctrl+N"), width >= 50, "{width}: {frame}");
        // R13, design 5bafadfff: compact count stays even when its noun/machine go.
        assert!(toolbar.contains("1/3"), "{width}: {frame}");
        if width >= 76 {
            assert!(frame.contains("1/3 sessions · This machine"), "{frame}");
        }
        assert!(frame.lines().count() <= 36, "{frame}");
    }
    let queried = render_picker_capture_at(
        picker_request(),
        120,
        36,
        vec![
            TerminalEvent::BracketedPaste("00000000-0000-4000-8000-000000000042".into()),
            ctrl_key('d'),
        ],
    )
    .await;
    assert!(
        queried.contains("0/3 sessions · This machine"),
        "count stays whole beside a long query: {queried}"
    );
    for width in [32, 40] {
        let opened = render_picker_capture_at(
            picker_request(),
            width,
            24,
            vec![ctrl_key('n'), ctrl_key('d')],
        )
        .await;
        assert!(
            opened.contains("New session"),
            "hidden key hint keeps its binding: {opened}"
        );
    }
    let activated = render_picker_capture_at(
        picker_request(),
        48,
        24,
        vec![
            key(KeyCode::Tab),
            key(KeyCode::Tab),
            key(KeyCode::Enter),
            ctrl_key('d'),
        ],
    )
    .await;
    assert!(
        activated.contains("New session"),
        "the compact button remains in toolbar focus order: {activated}"
    );
}

#[tokio::test]
async fn toolbar_scope_visibility_matches_targets_and_filter_uses_remaining_cells() {
    use agent_collaboration_tui::surfaces::{
        surface_handle::SurfaceStackHandle, surface_stack::SurfaceId,
    };
    for width in [32usize, 40, 48, 76, 104, 120] {
        let observed = Arc::new(Mutex::new(None::<SurfaceStackHandle>));
        let sink = observed.clone();
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let mut component = element!(SessionsPickerComponent(request:picker_request(),width,height:36usize,
            surface_observer:move |(stack,_)| *sink.lock().unwrap()=Some(stack)));
        let mut checked = false;
        let journey = component
            .mock_terminal_render_loop(MockTerminalConfig::with_events(events))
            .map(|canvas| {
                if checked {
                    return;
                }
                let stack = observed.lock().unwrap().as_ref().copied().unwrap();
                let Some(map) = stack.hit_regions().published() else {
                    return;
                };
                let Some(filter) = map.targets.iter().find(|region| {
                    region.surface == SurfaceId::Screen
                        && region.target.as_str() == "session-filter"
                        && !region.cells.is_empty()
                }) else {
                    return;
                };
                let frame = canvas.to_string();
                let toolbar = frame.lines().find(|line| line.contains("Menu")).unwrap();
                let visible = width >= 104;
                for label in ["This folder", "Repo", "All"] {
                    assert_eq!(toolbar.contains(label), visible, "{width}: {frame}");
                }
                let screen = stack.snapshot();
                for action in ["scope-folder", "scope-repo", "scope-all"] {
                    let target = screen
                        .surface(SurfaceId::Screen)
                        .unwrap()
                        .focus
                        .targets()
                        .iter()
                        .find(|target| target.id.as_str() == format!("action:{action}"))
                        .unwrap();
                    assert_eq!(target.can_focus(), visible, "{width}: {action}");
                }
                let minimum = filter
                    .cells
                    .iter()
                    .map(|(column, _)| *column)
                    .min()
                    .unwrap();
                let maximum = filter
                    .cells
                    .iter()
                    .map(|(column, _)| *column)
                    .max()
                    .unwrap();
                assert!(
                    maximum - minimum + 1 >= if width == 32 { 9 } else { 12 },
                    "filter editor at {width}: {frame}"
                );
                checked = true;
                sender.send(ctrl_key('d')).unwrap();
            })
            .collect::<Vec<_>>();
        tokio::time::timeout(Duration::from_secs(2), journey)
            .await
            .expect("toolbar geometry and target publication");
        assert!(checked, "toolbar at {width} was never observed");
    }
}

#[tokio::test]
async fn hidden_new_returns_focus_after_resize_unless_the_user_moves() {
    use agent_collaboration_tui::surfaces::surface_handle::SurfaceStackHandle;
    let observed = Arc::new(Mutex::new(None::<SurfaceStackHandle>));
    let sink = observed.clone();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    sender.send(TerminalEvent::Resize(24, 24)).unwrap();
    sender.send(TerminalEvent::Resize(48, 24)).unwrap();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut component = element!(SessionsPickerComponent(request:picker_request(),width:0usize,height:24usize,
        surface_observer:move |(stack,_)| *sink.lock().unwrap()=Some(stack)));
    let mut phase = 0;
    let journey = component
        .mock_terminal_render_loop(MockTerminalConfig::with_events(events))
        .map(|canvas| {
            let stack = observed.lock().unwrap().as_ref().copied().unwrap();
            let snapshot = stack.snapshot();
            let focused = snapshot.top().focus.current().map(|target| target.as_str());
            let frame = canvas.to_string();
            match phase {
                0 if canvas.width() == 48 && frame.contains("+ New") => {
                    sender.send(key(KeyCode::Tab)).unwrap();
                    sender.send(key(KeyCode::Tab)).unwrap();
                    phase = 1;
                }
                1 if focused == Some("action:new") => {
                    sender.send(TerminalEvent::Resize(32, 24)).unwrap();
                    phase = 2;
                }
                2 if canvas.width() == 32 && !frame.contains("+ New") => {
                    assert_eq!(focused, Some("sessions-list"), "{frame}");
                    sender.send(TerminalEvent::Resize(48, 24)).unwrap();
                    phase = 3;
                }
                3 if canvas.width() == 48 && frame.contains("+ New") => {
                    assert_eq!(focused, Some("action:new"), "{frame}");
                    sender.send(TerminalEvent::Resize(32, 24)).unwrap();
                    phase = 4;
                }
                4 if canvas.width() == 32 && !frame.contains("+ New") => {
                    assert_eq!(focused, Some("sessions-list"), "{frame}");
                    sender.send(key(KeyCode::Tab)).unwrap();
                    phase = 5;
                }
                5 if focused == Some("action:menu") => {
                    sender.send(TerminalEvent::Resize(48, 24)).unwrap();
                    phase = 6;
                }
                6 if canvas.width() == 48 && frame.contains("+ New") => {
                    assert_eq!(focused, Some("action:menu"), "{frame}");
                    sender.send(ctrl_key('d')).unwrap();
                    phase = 7;
                }
                _ => {}
            }
        })
        .collect::<Vec<_>>();
    tokio::time::timeout(Duration::from_secs(2), journey)
        .await
        .expect("actual resize and focus transitions");
    assert_eq!(phase, 7);
}

#[tokio::test]
async fn stacked_rows_do_not_show_merged_header_columns() {
    let frame = render_picker_capture_at(picker_request(), 32, 24, vec![ctrl_key('d')]).await;
    let header = frame.lines().find(|line| line.contains("Session")).unwrap();
    assert!(header.contains("Session"), "{frame}");
    for omitted in ["Status", "Upd", "New", "Created"] {
        assert!(!header.contains(omitted), "{frame}");
    }
}

#[tokio::test]
async fn row_metadata_uses_readable_separators() {
    let frame = render_picker_capture_at(picker_request(), 120, 36, vec![ctrl_key('d')]).await;
    assert!(
        frame.contains("Codex · This machine · ⎇ main · 📂 /repo/project-a"),
        "{frame}"
    );
}

#[tokio::test]
async fn details_sections_have_one_blank_row_before_each_heading() {
    let frame = render_picker_capture_at(
        picker_request(),
        120,
        36,
        vec![key(KeyCode::Enter), ctrl_key('d')],
    )
    .await;
    let body = dialog_text(&frame, dialog_bounds(&frame, "Session details"));
    let lines = body.lines().collect::<Vec<_>>();
    for heading in ["Original title", "Conversation"] {
        let row = lines
            .iter()
            .position(|line| line.contains(heading))
            .unwrap();
        assert!(lines[row - 1].trim().is_empty(), "{heading}: {frame}");
        assert!(
            !lines[row - 2].trim().is_empty(),
            "one separator only: {frame}"
        );
    }
}

#[tokio::test]
async fn help_context_uses_screen_instead_of_internal_surface_jargon() {
    let frame = render_picker_capture_at(
        picker_request(),
        120,
        36,
        vec![key(KeyCode::F(1)), ctrl_key('d')],
    )
    .await;
    assert!(
        frame.contains("Screen") && frame.contains("Sessions"),
        "{frame}"
    );
    assert!(!frame.contains("Surface"), "{frame}");
}
