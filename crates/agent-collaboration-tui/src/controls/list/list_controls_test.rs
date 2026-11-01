use crate::test_support::{
    event_driver::{EventDriver, InputMode},
    list_scene::{ListHandle, ListScene, key, rows, target},
    recording_backend::recording_terminal_render_loop,
};
use agent_collaboration_tui::{
    controls::list::{ListRowKey, ListSelectionState},
    surfaces::surface_stack::SurfaceId,
};
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};
#[tokio::test]
async fn list_navigation_updates_the_selected_key_before_enter_in_one_burst() {
    let activated = Arc::new(Mutex::new(Vec::new()));
    let (mut driver, config, journal) = EventDriver::new();
    let mut ui = element!(ListScene(journal,activated:activated.clone()));
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
    let scenario = async {
        driver.initial(&mut frames).await;
        let key = |code| TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code));
        driver
            .drive(
                &mut frames,
                InputMode::Burst,
                vec![(key(KeyCode::Down), false), (key(KeyCode::Enter), true)],
                true,
            )
            .await;
        assert_eq!(*activated.lock().unwrap(), vec!["bravo"]);
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
        .await
        .unwrap();
}
#[tokio::test]
async fn list_navigation_pages_and_bounds_reach_the_consumer_before_the_next_event() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        let handle = ListHandle::default();
        let selections = Arc::new(Mutex::new(Vec::new()));
        let activated = Arc::new(Mutex::new(Vec::new()));
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(ListScene(journal,handle:handle.clone(),selections:selections.clone(),activated:activated.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let last_ack = std::cell::Cell::new(0);
        let scenario = async {
            driver.initial(&mut frames).await;
            let expected = [
                "bravo", "charlie", "echo", "alpha", "echo", "delta", "bravo", "bravo",
            ];
            driver
                .drive_checked(
                    &mut frames,
                    mode,
                    [
                        KeyCode::Down,
                        KeyCode::Down,
                        KeyCode::PageDown,
                        KeyCode::Home,
                        KeyCode::End,
                        KeyCode::Up,
                        KeyCode::PageUp,
                        KeyCode::Enter,
                    ]
                    .into_iter()
                    .map(|code| (key(code), true))
                    .collect(),
                    true,
                    |ack| {
                        last_ack.set(ack.sequence);
                        assert_eq!(
                            ack.snapshot.0[0].1,
                            format!("{}|list", expected[ack.sequence - 1]).as_bytes()
                        )
                    },
                )
                .await;
            assert_eq!(*activated.lock().unwrap(), vec!["bravo"]);
            assert_eq!(
                *selections.lock().unwrap(),
                expected[..7]
                    .iter()
                    .map(|key| Some((*key).to_owned()))
                    .collect::<Vec<_>>()
            );
            assert_eq!(handle.get().control.selected().unwrap().as_str(), "bravo");
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "{mode:?} last ack {} position {:?}",
                    last_ack.get(),
                    handle.get().control.scroll().position()
                )
            });
    }
}
#[tokio::test]
async fn tab_leaves_the_whole_list_for_the_toolbar_and_returns_without_walking_rows() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        let activated = Arc::new(Mutex::new(Vec::new()));
        let handle = ListHandle::default();
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(ListScene(journal,handle:handle.clone(),activated:activated.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            let fixture = handle.get();
            assert_eq!(
                fixture
                    .stack
                    .snapshot()
                    .top()
                    .focus
                    .targets()
                    .iter()
                    .filter(|target| target.can_focus())
                    .count(),
                2
            );
            let expected = ["toolbar", "toolbar", "list", "list"];
            driver
                .drive_checked(
                    &mut frames,
                    mode,
                    [
                        KeyCode::Tab,
                        KeyCode::Enter,
                        KeyCode::BackTab,
                        KeyCode::Enter,
                    ]
                    .into_iter()
                    .map(|code| (key(code), true))
                    .collect(),
                    true,
                    |ack| {
                        assert_eq!(
                            ack.snapshot.0[0].1,
                            format!("alpha|{}", expected[ack.sequence - 1]).as_bytes()
                        )
                    },
                )
                .await;
            assert_eq!(*activated.lock().unwrap(), vec!["toolbar", "alpha"]);
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }
}
#[tokio::test]
async fn list_reload_preserves_keys_then_uses_the_removed_rows_position_and_last_row() {
    let handle = ListHandle::default();
    let selections = Arc::new(Mutex::new(Vec::new()));
    let activated = Arc::new(Mutex::new(Vec::new()));
    let (mut driver, config, journal) = EventDriver::new();
    let mut ui = element!(ListScene(journal,handle:handle.clone(),selections:selections.clone(),activated:activated.clone()));
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
    let scenario = async {
        driver.initial(&mut frames).await;
        driver
            .drive(
                &mut frames,
                InputMode::Burst,
                vec![
                    (key(KeyCode::Down), true),
                    (key(KeyCode::Down), true),
                    (key(KeyCode::Tab), true),
                ],
                true,
            )
            .await;
        let fixture = handle.get();
        let control = fixture.control;
        assert!(
            control
                .replace_rows(rows(&["echo", "charlie", "alpha", "bravo"]))
                .unwrap()
        );
        assert_eq!(
            control.selected().unwrap().as_str(),
            "charlie",
            "reorder keeps identity"
        );
        assert_eq!(
            fixture.stack.snapshot().top().focus.current(),
            Some(&target("toolbar"))
        );
        assert!(fixture.stack.hit_regions().published().is_none());
        assert!(
            control
                .replace_rows(rows(&["alpha", "bravo", "echo"]))
                .unwrap()
        );
        assert_eq!(
            control.selected().unwrap().as_str(),
            "bravo",
            "remove uses former position"
        );
        driver.initial(&mut frames).await;
        driver
            .drive(
                &mut frames,
                InputMode::Burst,
                vec![(key(KeyCode::BackTab), true), (key(KeyCode::Down), true)],
                true,
            )
            .await;
        assert_eq!(control.selected().unwrap().as_str(), "echo");
        assert!(control.replace_rows(rows(&["alpha", "bravo"])).unwrap());
        assert_eq!(
            control.selected().unwrap().as_str(),
            "bravo",
            "removed last row chooses new last"
        );
        assert!(control.replace_rows(Vec::new()).unwrap());
        assert_eq!(control.selected(), None);
        driver.initial(&mut frames).await;
        driver
            .drive(
                &mut frames,
                InputMode::Burst,
                vec![(key(KeyCode::Enter), false)],
                false,
            )
            .await;
        assert!(activated.lock().unwrap().is_empty());
        assert_eq!(
            *selections.lock().unwrap(),
            vec![
                Some("bravo".into()),
                Some("charlie".into()),
                Some("bravo".into()),
                Some("echo".into()),
                Some("bravo".into()),
                None
            ]
        );
        let snapshot = fixture.stack.snapshot();
        assert_eq!(
            snapshot.top().focus.targets().len(),
            3,
            "list, scroll and untouched toolbar only"
        );
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
        .await
        .unwrap();
}
#[tokio::test]
async fn list_click_and_wheel_select_keys_and_reveal_the_selected_row() {
    let handle = ListHandle::default();
    let selections = Arc::new(Mutex::new(Vec::new()));
    let (mut driver, config, journal) = EventDriver::new();
    let mut ui = element!(ListScene(journal,handle:handle.clone(),selections:selections.clone()));
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
    let scenario = async {
        driver.initial(&mut frames).await;
        let fixture = handle.get();
        let map = fixture.stack.hit_regions().published().unwrap();
        let row = fixture
            .control
            .row_target(&ListRowKey::new("bravo").unwrap());
        let point = *map
            .targets
            .iter()
            .find(|region| region.target == row)
            .unwrap()
            .cells
            .first()
            .unwrap();
        driver
            .drive(
                &mut frames,
                InputMode::Burst,
                vec![(
                    TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                        MouseEventKind::Down(MouseButton::Left),
                        point.0,
                        point.1,
                    )),
                    true,
                )],
                true,
            )
            .await;
        assert_eq!(fixture.control.selected().unwrap().as_str(), "bravo");
        driver
            .drive(
                &mut frames,
                InputMode::Burst,
                vec![(
                    TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                        MouseEventKind::ScrollDown,
                        point.0,
                        point.1,
                    )),
                    true,
                )],
                true,
            )
            .await;
        while fixture.stack.hit_regions().published().is_none() {
            driver.initial(&mut frames).await;
        }
        assert_eq!(fixture.control.selected().unwrap().as_str(), "charlie");
        assert!(
            fixture.control.scroll().position().offset > 0,
            "position {:?}",
            fixture.control.scroll().position()
        );
        assert!((0..driver.frames.last().unwrap().height).any(|row| {
            driver
                .frames
                .last()
                .unwrap()
                .row_text(row)
                .contains("charlie")
        }));
        assert_eq!(
            *selections.lock().unwrap(),
            vec![Some("bravo".into()), Some("charlie".into())]
        );
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
        .await
        .unwrap();
}
#[tokio::test]
async fn list_selection_stays_inert_beneath_an_overlay_including_wheel_input() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        let handle = ListHandle::default();
        let selections = Arc::new(Mutex::new(Vec::new()));
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui =
            element!(ListScene(journal,handle:handle.clone(),selections:selections.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            driver
                .drive_checked(
                    &mut frames,
                    mode,
                    vec![
                        (key(KeyCode::F(10)), true),
                        (key(KeyCode::Down), false),
                        (key(KeyCode::PageDown), false),
                        (
                            TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                                MouseEventKind::ScrollDown,
                                3,
                                1,
                            )),
                            false,
                        ),
                        (key(KeyCode::Esc), true),
                    ],
                    false,
                    |ack| {
                        assert!(
                            String::from_utf8_lossy(&ack.snapshot.0[0].1).starts_with("alpha|"),
                            "{ack:?}"
                        )
                    },
                )
                .await;
            assert_eq!(handle.get().control.selected().unwrap().as_str(), "alpha");
            assert!(selections.lock().unwrap().is_empty());
            assert_eq!(handle.get().stack.snapshot().top_id(), SurfaceId::Screen);
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }
}
#[test]
fn list_rejects_duplicate_keys_and_unknown_initial_selection_without_corrupting_rows() {
    assert!(ListSelectionState::new(rows(&["alpha", "alpha"]), None).is_err());
    assert!(
        ListSelectionState::new(rows(&["alpha"]), Some(ListRowKey::new("foreign").unwrap()))
            .is_err()
    );
    let mut selection = ListSelectionState::new(rows(&["alpha", "bravo"]), None).unwrap();
    let before = selection.clone();
    assert!(selection.replace_rows(rows(&["bravo", "bravo"])).is_err());
    assert_eq!(selection, before);
    assert!(!selection.select(&ListRowKey::new("foreign").unwrap()));
    assert_eq!(selection, before);
}
#[tokio::test]
async fn closed_list_parent_ignores_a_retained_controls_late_row_update() {
    use agent_collaboration_tui::{
        actions::{ActionId, ActionRegistry, ScopeId},
        controls::list::{ListSelectionBinding, ListSelectionControl},
        surfaces::{
            focus_model::FocusModel,
            surface_handle::SurfaceContext,
            surface_stack::{OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceModel},
        },
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    let handle = ListHandle::default();
    let (mut driver, config, journal) = EventDriver::new();
    let mut ui = element!(ListScene(journal,handle:handle.clone()));
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
    let scenario = async {
        driver.initial(&mut frames).await;
        let fixture = handle.get();
        let overlay = fixture
            .stack
            .open_overlay(OverlaySpec {
                kind: OverlayKind::Dialog,
                model: SurfaceModel {
                    focus: FocusModel::new(Vec::new()).unwrap(),
                    actions: ActionRegistry::new(
                        ScopeId::new("closed-list-dialog").unwrap(),
                        Vec::new(),
                    )
                    .unwrap(),
                    primary_action: None,
                },
                view: OverlayView::default(),
                layout: OverlayLayout::default(),
            })
            .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let control = ListSelectionControl::new(ListSelectionBinding {
            parent: SurfaceContext {
                stack: fixture.stack,
                surface: SurfaceId::Overlay(overlay),
            },
            id: target("dialog-list"),
            value: fixture.value,
            activation: ActionId::new("details").unwrap(),
            page_rows: Arc::new(|| 2),
            on_select: Handler::from({
                let calls = calls.clone();
                move |_| {
                    calls.fetch_add(1, Ordering::SeqCst);
                }
            }),
        });
        assert!(
            fixture
                .stack
                .update_overlay_if_open(overlay, |model| model.focus =
                    FocusModel::new(control.targets()).unwrap())
        );
        assert!(
            control.replace_rows(control.rows()).unwrap(),
            "live parent accepts the control"
        );
        assert_eq!(fixture.stack.close_top(), Some(overlay));
        let before = fixture.value.read().clone();
        assert!(!control.replace_rows(rows(&["foreign"])).unwrap());
        assert_eq!(*fixture.value.read(), before);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.stack.snapshot().top_id(), SurfaceId::Screen);
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
        .await
        .unwrap();
}

#[tokio::test]
async fn list_focus_uses_the_left_marker_without_a_right_edge_glyph_and_keeps_real_scrollbars() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        for viewport_rows in [2u16, 6u16] {
            let handle = ListHandle::default();
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(ListScene(journal,viewport_rows:Some(viewport_rows),handle:handle.clone()));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                driver.initial(&mut frames).await;
                let fixture = handle.get();
                let frame = driver.frames.last().unwrap();
                assert_eq!(
                    frame.cell(0, 1).unwrap().text,
                    "❯",
                    "left focus marker remains"
                );
                assert_eq!(
                    frame.cell(0, 1).unwrap().style.foreground,
                    Some(Color::AnsiValue(11))
                );
                assert_ne!(
                    frame.cell(75, 1).unwrap().text,
                    "❯",
                    "right edge is reserved for real overflow, not duplicate focus"
                );
                if fixture.control.scroll().position().maximum() > 0 {
                    assert!(
                        matches!(frame.cell(75, 1).unwrap().text.as_str(), "█" | "│"),
                        "real thumb or track remains"
                    );
                } else {
                    assert_eq!(frame.cell(75, 1).unwrap().text, " ");
                }
                driver
                    .drive(&mut frames, mode, vec![(key(KeyCode::Down), true)], true)
                    .await;
                assert_eq!(fixture.control.selected().unwrap().as_str(), "bravo");
                let frame = driver.frames.last().unwrap();
                assert_eq!(frame.cell(0, 2).unwrap().text, "❯");
                assert_eq!(
                    frame.cell(0, 2).unwrap().style.foreground,
                    Some(Color::AnsiValue(11))
                );
                assert_ne!(frame.cell(75, 1).unwrap().text, "❯");
            })
            .await
            .unwrap();
        }
    }
}
