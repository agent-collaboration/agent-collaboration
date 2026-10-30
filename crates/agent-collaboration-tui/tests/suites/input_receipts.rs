use crate::support::{
    event_driver::{EventDriver, InputMode},
    receipt_scene::{ReceiptOverlay, ReceiptScene},
    recording_backend::recording_terminal_render_loop,
};
use agent_collaboration_tui::{
    input_dispatch_receipt::InputDispatchReceiptSink, surface_stack::SurfaceId,
};
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn real_input_owner_flushes_one_receipt_after_the_action_and_consumer_callback() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("input-dispatch.jsonl");
    let sink = InputDispatchReceiptSink::create(&path).unwrap();
    let observations = Arc::new(Mutex::new(Vec::new()));
    let on_observed = Handler::from({
        let path = path.clone();
        let observations = observations.clone();
        move |()| {
            observations
                .lock()
                .unwrap()
                .push(std::fs::read_to_string(&path).unwrap().lines().count())
        }
    });
    let (mut driver, config, journal) = EventDriver::new();
    let mut ui = element!(ReceiptScene(journal,sink:sink.clone(),on_observed));
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
    let scenario = async {
        driver.initial(&mut frames).await;
        let acknowledgements = driver
            .drive(
                &mut frames,
                InputMode::Burst,
                vec![(
                    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Enter)),
                    true,
                )],
                true,
            )
            .await;
        assert_eq!(acknowledgements[0].snapshot.0[0].1, b"|1|0");
        sink.flush_and_check().unwrap();
        let bytes = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            bytes.lines().count(),
            1,
            "one complete receipt after the real handler and on_routed return"
        );
        let record: serde_json::Value = serde_json::from_str(bytes.trim_end()).unwrap();
        assert_eq!(record["sequence"], 1);
        assert_eq!(record["surface"], serde_json::json!({"kind":"screen"}));
        assert_eq!(record["target"], "text");
        assert_eq!(record["action"], "inspect");
        assert_eq!(
            *observations.lock().unwrap(),
            vec![0],
            "publication follows the real on_routed callback"
        );
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
        .await
        .unwrap();
}

#[tokio::test]
async fn receipts_count_ignored_input_and_redact_text_and_paste_from_real_events() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("input-dispatch.jsonl");
        let sink = InputDispatchReceiptSink::create(&path).unwrap();
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(ReceiptScene(journal,sink:sink.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            let key = |kind, code, modifiers| {
                let mut key = KeyEvent::new(kind, code);
                key.modifiers = modifiers;
                TerminalEvent::Key(key)
            };
            let acknowledgements = driver
                .drive(
                    &mut frames,
                    mode,
                    vec![
                        (
                            key(
                                KeyEventKind::Press,
                                KeyCode::Char('z'),
                                KeyModifiers::empty(),
                            ),
                            true,
                        ),
                        (
                            key(
                                KeyEventKind::Press,
                                KeyCode::Char('界'),
                                KeyModifiers::SHIFT,
                            ),
                            true,
                        ),
                        (
                            TerminalEvent::BracketedPaste("private draft contents".into()),
                            true,
                        ),
                        (
                            key(KeyEventKind::Repeat, KeyCode::Enter, KeyModifiers::empty()),
                            false,
                        ),
                        (
                            key(KeyEventKind::Release, KeyCode::Enter, KeyModifiers::empty()),
                            false,
                        ),
                        (TerminalEvent::Resize(60, 14), true),
                        (
                            key(
                                KeyEventKind::Press,
                                KeyCode::Char('x'),
                                KeyModifiers::CONTROL,
                            ),
                            false,
                        ),
                        (
                            TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                                MouseEventKind::Up(MouseButton::Left),
                                40,
                                10,
                            )),
                            false,
                        ),
                        (
                            key(KeyEventKind::Press, KeyCode::Enter, KeyModifiers::empty()),
                            true,
                        ),
                    ],
                    true,
                )
                .await;
            assert_eq!(
                acknowledgements.last().unwrap().snapshot.0[0].1,
                "z界private draft contents|1|0".as_bytes()
            );
            sink.flush_and_check().unwrap();
            let bytes = std::fs::read_to_string(&path).unwrap();
            assert!(bytes.ends_with('\n'), "last line is complete and flushed");
            assert!(!bytes.contains("private draft contents") && !bytes.contains('界'));
            let records = bytes
                .lines()
                .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
                .collect::<Vec<_>>();
            assert_eq!(records.len(), 9);
            for (index, record) in records.iter().enumerate() {
                assert_eq!(record.as_object().unwrap().len(), 5);
                assert_eq!(record["sequence"], index + 1);
                assert_eq!(record["surface"], serde_json::json!({"kind":"screen"}));
            }
            assert_eq!(
                records[0]["event"]["code"],
                serde_json::json!({"kind":"text"})
            );
            assert_eq!(records[1]["event"]["code"], records[0]["event"]["code"]);
            assert_eq!(
                records[1]["event"]["modifiers"],
                serde_json::json!(["shift"])
            );
            assert_eq!(
                records[2]["event"],
                serde_json::json!({"kind":"other","event":"bracketedPaste"})
            );
            assert_eq!(records[3]["event"]["phase"], "repeat");
            assert_eq!(records[4]["event"]["phase"], "release");
            assert_eq!(
                records[5]["event"],
                serde_json::json!({"kind":"resize","columns":60,"rows":14})
            );
            assert_eq!(
                records[6]["event"]["code"],
                serde_json::json!({"kind":"shortcut","key":"x"})
            );
            assert_eq!(
                records[7]["event"],
                serde_json::json!({"kind":"mouse","mouse":{"kind":"up","button":"left"},"column":40,"row":10,"modifiers":[]})
            );
            for record in &records[..3] {
                assert_eq!(record["target"], "text");
                assert!(record["action"].is_null());
            }
            for record in &records[3..8] {
                assert!(
                    record["target"].is_null() && record["action"].is_null(),
                    "unused focused target is not the consumer: {record}"
                );
            }
            assert_eq!(records[8]["target"], "text");
            assert_eq!(records[8]["action"], "inspect");
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }
}

fn read_receipts(path: &std::path::Path) -> std::io::Result<Vec<serde_json::Value>> {
    let bytes = std::fs::read_to_string(path)?;
    if !bytes.ends_with('\n') {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "receipt publication must flush a complete line",
        ));
    }
    bytes
        .lines()
        .map(|line| serde_json::from_str(line).map_err(std::io::Error::from))
        .collect()
}
fn control(code: KeyCode) -> TerminalEvent {
    let mut key = KeyEvent::new(KeyEventKind::Press, code);
    key.modifiers = KeyModifiers::CONTROL;
    TerminalEvent::Key(key)
}

#[tokio::test]
async fn outside_close_receipt_keeps_the_old_overlay_and_release_does_not_activate_beneath() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("input-dispatch.jsonl");
        let sink = InputDispatchReceiptSink::create(&path).unwrap();
        let stack_handle = Arc::new(Mutex::new(None));
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(ReceiptScene(journal,sink:sink.clone(),overlay:ReceiptOverlay::Menu,stack_handle:stack_handle.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            let stack = stack_handle.lock().unwrap().unwrap();
            let SurfaceId::Overlay(menu) = stack.snapshot().top_id() else {
                panic!("expected menu")
            };
            let acknowledgements = driver
                .drive(
                    &mut frames,
                    mode,
                    vec![
                        (
                            TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                                MouseEventKind::Down(MouseButton::Left),
                                0,
                                0,
                            )),
                            true,
                        ),
                        (
                            TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                                MouseEventKind::Up(MouseButton::Left),
                                0,
                                0,
                            )),
                            false,
                        ),
                    ],
                    true,
                )
                .await;
            assert!(
                acknowledgements
                    .iter()
                    .all(|ack| ack.snapshot.0[0].1 == b"|0|0")
            );
            assert_eq!(stack.snapshot().top_id(), SurfaceId::Screen);
            sink.flush_and_check().unwrap();
            let records = read_receipts(&path).unwrap();
            assert_eq!(records.len(), 2);
            assert_eq!(
                records[0]["surface"],
                serde_json::json!({"kind":"overlay","overlayId":menu.get().to_string()})
            );
            assert_eq!(records[1]["surface"], serde_json::json!({"kind":"screen"}));
            for record in records {
                assert!(record["target"].is_null() && record["action"].is_null());
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn dialog_dropdown_primary_receipt_names_the_invoked_parent_after_both_overlays_close() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("input-dispatch.jsonl");
        let sink = InputDispatchReceiptSink::create(&path).unwrap();
        let stack_handle = Arc::new(Mutex::new(None));
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(ReceiptScene(journal,sink:sink.clone(),overlay:ReceiptOverlay::DialogDropdown,stack_handle:stack_handle.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            let stack = stack_handle.lock().unwrap().unwrap();
            let before = stack.snapshot();
            let dialog = before.overlays()[0].id;
            assert_eq!(before.overlays().len(), 2);
            let acknowledgements = driver
                .drive(
                    &mut frames,
                    mode,
                    vec![(control(KeyCode::Enter), true)],
                    true,
                )
                .await;
            assert_eq!(acknowledgements[0].snapshot.0[0].1, b"|1|0");
            assert_eq!(stack.snapshot().top_id(), SurfaceId::Screen);
            sink.flush_and_check().unwrap();
            let records = read_receipts(&path).unwrap();
            assert_eq!(records.len(), 1);
            assert_eq!(
                records[0]["surface"],
                serde_json::json!({"kind":"overlay","overlayId":dialog.get().to_string()})
            );
            assert!(records[0]["target"].is_null());
            assert_eq!(records[0]["action"], "start");
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn ctrl_c_receipt_names_the_screen_action_and_does_not_invent_a_denied_invocation() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("input-dispatch.jsonl");
        let sink = InputDispatchReceiptSink::create(&path).unwrap();
        let stack_handle = Arc::new(Mutex::new(None));
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(ReceiptScene(journal,sink:sink.clone(),overlay:ReceiptOverlay::Menu,stack_handle:stack_handle.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            let stack = stack_handle.lock().unwrap().unwrap();
            let menu = stack.snapshot().top_id();
            let acknowledgements = driver
                .drive(
                    &mut frames,
                    mode,
                    vec![
                        (control(KeyCode::Char('c')), true),
                        (control(KeyCode::Char('c')), false),
                    ],
                    true,
                )
                .await;
            assert!(
                acknowledgements
                    .iter()
                    .all(|ack| ack.snapshot.0[0].1 == b"|0|1")
            );
            assert_eq!(
                stack.snapshot().top_id(),
                menu,
                "screen quit receipt does not imply overlay closure"
            );
            sink.flush_and_check().unwrap();
            let records = read_receipts(&path).unwrap();
            assert_eq!(records.len(), 2);
            for record in &records {
                assert_eq!(record["surface"], serde_json::json!({"kind":"screen"}));
                assert!(record["target"].is_null());
            }
            assert_eq!(records[0]["action"], "quit");
            assert!(records[1]["action"].is_null());
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }
}
