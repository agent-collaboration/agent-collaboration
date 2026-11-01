use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use futures_util::StreamExt;
use iocraft::prelude::*;

use super::quota_browse_presentation_test::quota_two_account_view_model;
use super::quota_floor_editor::WeeklyFloorEditorPhase;
use super::quota_floor_editor::WeeklyFloorEditorState;
use super::quota_floor_editor::WeeklyFloorStep;
use super::quota_floor_editor::WeeklyQuotaFloorSaveError;
use super::quota_floor_editor::WeeklyQuotaFloorSaver;
use super::quota_floor_editor::step_weekly_floor_percent;
use super::quota_floor_editor::weekly_floor_editor_key_command;
use super::quota_reset_component_input::forced_quota_exit_key;
use super::quota_status_component::QuotaStatusComponent;
use crate::quota_status_view_model::QuotaStatusViewModelLoader;

#[test]
fn weekly_floor_stepper_clamps_to_zero_and_fifteen() {
    assert_eq!(step_weekly_floor_percent(0, WeeklyFloorStep::Decrease), 0);
    assert_eq!(step_weekly_floor_percent(14, WeeklyFloorStep::Increase), 15);
    assert_eq!(step_weekly_floor_percent(15, WeeklyFloorStep::Increase), 15);
}

#[test]
fn floor_editor_preserves_component_owned_ctrl_c_and_ctrl_d_exit_keys() {
    assert!(forced_quota_exit_key(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL
    ));
    assert!(forced_quota_exit_key(
        KeyCode::Char('d'),
        KeyModifiers::CONTROL
    ));
    assert!(!forced_quota_exit_key(
        KeyCode::Char('e'),
        KeyModifiers::CONTROL
    ));
    assert!(!forced_quota_exit_key(KeyCode::Esc, KeyModifiers::NONE));
}

#[test]
fn committed_save_cannot_be_cancelled_while_refresh_is_unresolved() {
    let mut state = Some(WeeklyFloorEditorState::new(
        codex_router_core::ids::AccountId::new("committed-floor")
            .expect("test account should be valid"),
        "committed".to_owned(),
        11,
    ));
    state.as_mut().expect("editor should exist").phase = WeeklyFloorEditorPhase::SavedRefreshFailed;

    assert!(
        weekly_floor_editor_key_command(&mut state, KeyCode::Esc, KeyModifiers::NONE).is_none()
    );
    assert!(state.is_some());
    assert!(
        weekly_floor_editor_key_command(&mut state, KeyCode::Char('e'), KeyModifiers::CONTROL)
            .is_none()
    );
    assert!(state.is_some());
}

// Each next input follows the real draw produced by the prior domain transition.
enum FloorFrame {
    Any,
    Browse,
    Draft(u16),
    Message(&'static str),
}
struct FloorScenario {
    view_model: codex_router_quota_service::quota_display_model::QuotaStatusViewModel,
    saver: WeeklyQuotaFloorSaver,
    loader: Option<QuotaStatusViewModelLoader>,
    steps: Vec<(TerminalEvent, FloorFrame)>,
}
fn floor_key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}
async fn render_floor_scenario(inputs: FloorScenario) -> Vec<String> {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    // External completions stay held until the actual pending state is painted.
    // This makes identical retry outcomes observable without timing waits.
    let release = Arc::new(tokio::sync::Notify::new());
    let saved_port = inputs.saver;
    let save_gate = release.clone();
    let saver: WeeklyQuotaFloorSaver = Arc::new(move |account, percent| {
        let result = saved_port(account, percent);
        let gate = save_gate.clone();
        Box::pin(async move {
            let result = result.await;
            gate.notified().await;
            result
        })
    });
    let loader = inputs.loader.map(|loader| {
        let gate = release.clone();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        Arc::new(move || {
            let result = loader();
            let gate = gate.clone();
            let retry = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) > 0;
            Box::pin(async move {
                let result = result.await;
                if retry {
                    gate.notified().await;
                }
                result
            }) as crate::quota_status_view_model::QuotaStatusViewModelLoadFuture
        }) as QuotaStatusViewModelLoader
    });
    let outcome = Arc::new(Mutex::new(None));
    let mut component = element!(QuotaStatusComponent(view_model:inputs.view_model,width:120usize,height:48usize,weekly_floor_saver:Some(saver),reload_view_model:loader,spinner_interval:Duration::from_secs(3600),outcome_sink:Some(outcome.clone())));
    let frames = crate::quota_test_support::recording_backend::recording_terminal_render_loop(
        &mut component,
        MockTerminalConfig::with_events(events),
    );
    tokio::pin!(frames);
    let mut output = Vec::new();
    tokio::time::timeout(Duration::from_secs(2), async {
        output.push(
            crate::quota_frame_observation::next_current_frame(frames.as_mut())
                .await
                .unwrap()
                .unwrap()
                .to_string(),
        );
        for (event, expected) in inputs.steps {
            let operation = matches!(&event,TerminalEvent::Key(key) if key.code==KeyCode::Enter);
            sender.send(event).unwrap();
            if operation {
                loop {
                    let frame = crate::quota_frame_observation::next_current_frame(frames.as_mut())
                        .await
                        .unwrap()
                        .unwrap()
                        .to_string();
                    let pending = frame.contains("Saving preference");
                    output.push(frame);
                    if pending {
                        break;
                    }
                }
                release.notify_one();
            }
            loop {
                let frame = crate::quota_frame_observation::next_current_frame(frames.as_mut())
                    .await
                    .unwrap()
                    .expect("floor must remain mounted until its quit action")
                    .to_string();
                let matched = match expected {
                    FloorFrame::Any => true,
                    FloorFrame::Browse => {
                        !frame.contains("Weekly floor") && frame.contains("Filters")
                    }
                    FloorFrame::Draft(percent) => frame.lines().any(|line| {
                        line.contains("Floor") && line.contains(&format!("{percent}%"))
                    }),
                    FloorFrame::Message(message) => frame.contains(message),
                };
                output.push(frame);
                if matched {
                    break;
                }
            }
        }
        sender.send(TerminalEvent::Key(control_key('c'))).unwrap();
        while let Some(canvas) = frames.next().await {
            output.push(canvas.to_string());
        }
    })
    .await
    .unwrap_or_else(|error| {
        panic!(
            "floor scenario must reach its actual quit boundary: {error}; last frame: {:?}",
            output.last()
        )
    });
    assert_eq!(*outcome.lock().unwrap(), Some(crate::QuotaOutcome::Quit));
    output
}

#[tokio::test]
async fn ctrl_e_saves_the_focused_stable_account_and_reloads_immediately() {
    let mut view_model = quota_two_account_view_model();
    view_model.rows[0].account = "duplicate".to_owned();
    view_model.rows[1].account = "duplicate".to_owned();
    view_model.rows[1].weekly_quota_floor_percent = 10;
    let expected_account_id = view_model.rows[1].account_id.clone();
    let mut reloaded_view_model = view_model.clone();
    reloaded_view_model.rows[1].weekly_quota_floor_percent = 11;
    let recorded_saves = Arc::new(Mutex::new(Vec::new()));
    let saver: WeeklyQuotaFloorSaver = {
        let recorded_saves = Arc::clone(&recorded_saves);
        Arc::new(move |account_id, percent| {
            recorded_saves
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((account_id, percent));
            Box::pin(async { Ok(()) })
        })
    };
    let loader: QuotaStatusViewModelLoader = Arc::new(move || {
        let reloaded_view_model = reloaded_view_model.clone();
        Box::pin(async move { Some(reloaded_view_model) })
    });
    let frames = render_floor_scenario(FloorScenario {
        view_model,
        saver,
        loader: Some(loader),
        steps: vec![
            (floor_key(KeyCode::Down), FloorFrame::Any),
            (TerminalEvent::Key(control_key('e')), FloorFrame::Draft(10)),
            (floor_key(KeyCode::Right), FloorFrame::Draft(11)),
            (floor_key(KeyCode::Enter), FloorFrame::Browse),
        ],
    })
    .await;

    assert_eq!(
        *recorded_saves
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        vec![(expected_account_id, 11)]
    );
    assert!(frames.iter().any(|frame| frame.contains("Weekly floor")));
    assert!(frames.last().is_some_and(|frame| frame.contains("Filters")));
}

#[tokio::test]
async fn esc_and_ctrl_e_cancel_without_saving() {
    for cancel_event in [
        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Esc)),
        TerminalEvent::Key(control_key('e')),
    ] {
        let recorded_saves = Arc::new(Mutex::new(Vec::new()));
        let saver: WeeklyQuotaFloorSaver = {
            let recorded_saves = Arc::clone(&recorded_saves);
            Arc::new(move |account_id, percent| {
                recorded_saves
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push((account_id, percent));
                Box::pin(async { Ok(()) })
            })
        };
        let _frames = render_floor_scenario(FloorScenario {
            view_model: quota_two_account_view_model(),
            saver,
            loader: None,
            steps: vec![
                (TerminalEvent::Key(control_key('e')), FloorFrame::Draft(0)),
                (cancel_event, FloorFrame::Browse),
            ],
        })
        .await;

        assert!(
            recorded_saves
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty()
        );
    }
}

#[tokio::test]
async fn committed_save_with_failed_reload_stays_truthful_and_does_not_save_twice() {
    let save_count = Arc::new(Mutex::new(0usize));
    let saver: WeeklyQuotaFloorSaver = {
        let save_count = Arc::clone(&save_count);
        Arc::new(move |_account_id, _percent| {
            *save_count
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) += 1;
            Box::pin(async { Ok(()) })
        })
    };
    let loader: QuotaStatusViewModelLoader = Arc::new(|| Box::pin(async { None }));
    let frames = render_floor_scenario(FloorScenario {
        view_model: quota_two_account_view_model(),
        saver,
        loader: Some(loader),
        steps: vec![
            (TerminalEvent::Key(control_key('e')), FloorFrame::Draft(0)),
            (floor_key(KeyCode::Right), FloorFrame::Draft(1)),
            (
                floor_key(KeyCode::Enter),
                FloorFrame::Message("saved; refresh failed"),
            ),
            (
                floor_key(KeyCode::Enter),
                FloorFrame::Message("saved; refresh failed"),
            ),
        ],
    })
    .await;

    assert_eq!(
        *save_count
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        1
    );
    assert!(
        frames
            .iter()
            .any(|frame| frame.contains("saved; refresh failed"))
    );
    assert!(
        frames
            .iter()
            .rev()
            .any(|frame| frame.contains("Current") && frame.contains("1%"))
    );
}

#[tokio::test]
async fn mutation_failure_remains_visible_and_enter_retries_without_losing_the_draft() {
    let save_count = Arc::new(Mutex::new(0usize));
    let saver: WeeklyQuotaFloorSaver = {
        let save_count = Arc::clone(&save_count);
        Arc::new(move |_account_id, _percent| {
            *save_count
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) += 1;
            Box::pin(async { Err(WeeklyQuotaFloorSaveError::DatabaseBusy) })
        })
    };
    let frames = render_floor_scenario(FloorScenario {
        view_model: quota_two_account_view_model(),
        saver,
        loader: None,
        steps: vec![
            (TerminalEvent::Key(control_key('e')), FloorFrame::Draft(0)),
            (floor_key(KeyCode::Right), FloorFrame::Draft(1)),
            (
                floor_key(KeyCode::Enter),
                FloorFrame::Message("database busy; retry save"),
            ),
            (
                floor_key(KeyCode::Enter),
                FloorFrame::Message("database busy; retry save"),
            ),
            (TerminalEvent::Key(control_key('e')), FloorFrame::Browse),
        ],
    })
    .await;

    assert_eq!(
        *save_count
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        2
    );
    assert!(
        frames
            .iter()
            .any(|frame| frame.contains("database busy; retry save"))
    );
    assert!(frames.iter().any(|frame| frame.contains("1%")));
}

#[tokio::test]
async fn real_iocraft_arrow_button_handles_mouse_click() {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut model = quota_two_account_view_model();
    model.rows[0].account_id = codex_router_core::ids::AccountId::new("mouse-account").unwrap();
    model.rows[0].account = "mouse".to_owned();
    model.rows[0].weekly_quota_floor_percent = 10;
    let mut harness =
        crate::test_support::quota_control_fixture(crate::test_support::QuotaFixtureOptions {
            view_model: model,
            ports: crate::QuotaStatusViewOptions {
                weekly_floor_saver: Some(Arc::new(|_, _| Box::pin(async { Ok(()) }))),
                ..Default::default()
            },
            width: 48,
            height: 24,
            control: crate::test_support::QuotaFixtureControl::Floor,
        })
        .element;
    let frames = harness.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let mut clicked = false;
    let mut observed = false;
    tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(canvas) = crate::quota_frame_observation::next_current_frame(frames.as_mut())
            .await
            .expect("current floor frame")
        {
            let frame = canvas.to_string();
            if !clicked {
                let (row, line) = frame
                    .lines()
                    .enumerate()
                    .find(|(_, line)| line.contains('▶'))
                    .expect("visible increment control");
                let column = unicode_width::UnicodeWidthStr::width(
                    line.get(..line.find('▶').unwrap())
                        .expect("matched text starts at UTF-8 boundary"),
                );
                sender
                    .send(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                        MouseEventKind::Down(MouseButton::Left),
                        u16::try_from(column).unwrap_or(u16::MAX),
                        u16::try_from(row).unwrap_or(u16::MAX),
                    )))
                    .unwrap();
                clicked = true;
            } else if frame
                .lines()
                .any(|line| line.contains("Floor") && line.contains("11%"))
            {
                observed = true;
                sender.send(TerminalEvent::Key(control_key('c'))).unwrap();
            }
        }
    })
    .await
    .expect("actual arrow click changes the draft before exit");
    assert!(observed);
}

fn control_key(character: char) -> KeyEvent {
    let mut event = KeyEvent::new(KeyEventKind::Press, KeyCode::Char(character));
    event.modifiers = KeyModifiers::CONTROL;
    event
}

#[tokio::test]
async fn weekly_floor_dialog_pointer_changes_draft_without_saving_on_cancel() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use unicode_width::UnicodeWidthStr;
    let mut model = quota_two_account_view_model();
    model.rows[0].weekly_quota_floor_percent = 0;
    let save_calls = Arc::new(AtomicUsize::new(0));
    let observed_saves = save_calls.clone();
    let saver: WeeklyQuotaFloorSaver = Arc::new(move |_, _| {
        observed_saves.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    });
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut component = element!(QuotaStatusComponent(view_model: model, width:120usize, height:36usize, weekly_floor_saver:Some(saver)));
    let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let mut phase = 0;
    let mut last = String::new();
    let result = tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(canvas) = crate::quota_frame_observation::next_current_frame(frames.as_mut())
            .await
            .expect("current floor frame")
        {
            let frame = canvas.to_string();
            match phase {
                0 if frame.contains("Filters") => {
                    sender.send(TerminalEvent::Key(control_key('e'))).unwrap();
                    phase = 1;
                }
                1 if frame.contains("Weekly floor")
                    && frame
                        .lines()
                        .any(|line| line.contains("Floor") && line.contains('▶')) =>
                {
                    let (row, line) = frame
                        .lines()
                        .enumerate()
                        .find(|(_, line)| line.contains("Floor") && line.contains('▶'))
                        .unwrap_or_else(|| {
                            panic!("real composed dialog must expose increment control: {frame}")
                        });
                    let offset = line.find('▶').unwrap();
                    let column = UnicodeWidthStr::width(
                        line.get(..offset)
                            .expect("glyph begins at a UTF-8 boundary"),
                    );
                    assert!(
                        column < 120 && row < 36,
                        "increment control stays in the physical viewport: {frame}"
                    );
                    println!("composed_floor_increment column={column} row={row}\n{frame}");
                    sender
                        .send(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                            MouseEventKind::Down(MouseButton::Left),
                            u16::try_from(column).expect("visible column fits terminal bounds"),
                            u16::try_from(row).expect("visible row fits terminal bounds"),
                        )))
                        .unwrap();
                    phase = 2;
                }
                2 if frame
                    .lines()
                    .any(|line| line.contains("Floor") && line.contains("1%")) =>
                {
                    assert_eq!(
                        save_calls.load(Ordering::SeqCst),
                        0,
                        "pointer must change only draft"
                    );
                    sender
                        .send(TerminalEvent::Key(KeyEvent::new(
                            KeyEventKind::Press,
                            KeyCode::Esc,
                        )))
                        .unwrap();
                    phase = 3;
                }
                3 if !frame.contains("Weekly floor") && frame.contains("Filters") => {
                    sender
                        .send(TerminalEvent::Key(KeyEvent::new(
                            KeyEventKind::Press,
                            KeyCode::Char('q'),
                        )))
                        .unwrap();
                    phase = 4;
                }
                _ => {}
            }
            last = frame;
        }
    })
    .await;
    assert!(
        result.is_ok(),
        "composed dialog pointer/cancel phase={phase}: {last}"
    );
    assert_eq!(phase, 4, "{last}");
    assert_eq!(
        save_calls.load(Ordering::SeqCst),
        0,
        "cancel cannot save draft"
    );
}
