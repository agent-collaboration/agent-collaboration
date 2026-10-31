use super::*;

#[tokio::test]
async fn all_grid_credit_choices_keep_selection_empty_query_and_cancelled_policy_real() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    for (width, height) in [
        (32usize, 24usize),
        (48, 12),
        (40, 24),
        (48, 24),
        (76, 30),
        (120, 36),
    ] {
        let model = super::super::quota_storyboard_fixture_test::fixture_model();
        let saves = Arc::new(AtomicUsize::new(0));
        let counted = saves.clone();
        let saver: super::super::super::quota_account_options::CreditUsagePolicySaver =
            Arc::new(move |_, _, policy| {
                counted.fetch_add(1, Ordering::SeqCst);
                Box::pin(async move { Ok(policy) })
            });
        let (ports, driver) = super::quota_ux_form_test::options_preflight_ports();
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let outcome = Arc::new(std::sync::Mutex::new(None));
        let mut component = element!(QuotaStatusComponent(view_model:model,width,height,credit_policy_saver:Some(saver),reset_intent_sender:Some(ports.intent_sender),reset_snapshot_receiver:Some(ports.snapshot_receiver),outcome_sink:Some(outcome.clone())));
        let frames = crate::quota_test_support::recording_backend::recording_terminal_render_loop(
            &mut component,
            MockTerminalConfig::with_events(events),
        );
        tokio::pin!(frames);
        let primary_key_event = || {
            TerminalEvent::Key({
                let mut key = KeyEvent::new(KeyEventKind::Press, KeyCode::Enter);
                key.modifiers = KeyModifiers::CONTROL;
                key
            })
        };
        let mut phase = 0;
        let mut evidence = Vec::new();
        let mut captured_identity = String::new();
        let mut last = String::new();
        let journey = async {
            while let Some(canvas) =
                crate::quota_frame_observation::next_current_frame(frames.as_mut())
                    .await
                    .unwrap()
            {
                let frame = canvas.to_string();
                match phase {
                    0 => {
                        sender
                            .send(TerminalEvent::Key(clarity_control_key('r')))
                            .unwrap();
                        phase = 1;
                    }
                    1 if frame.contains("Options · Resets") => {
                        sender
                            .send(crate::quota_test_support::shift_tab_event())
                            .unwrap();
                        sender.send(provider_filter_key(KeyCode::Enter)).unwrap();
                        phase = 2;
                    }
                    2 if frame.contains("Options · Credits")
                        && !frame.contains("Tab change pending") =>
                    {
                        for key in [KeyCode::End, KeyCode::Tab, KeyCode::Tab, KeyCode::Enter] {
                            sender.send(provider_filter_key(key)).unwrap();
                        }
                        phase = 3;
                    }
                    3 if frame.contains("Credit usage") && frame.contains("Disallow ▾") => {
                        let dialog = actual_dialog_text(&frame, "Credit usage");
                        captured_identity = dialog
                            .lines()
                            .find(|line| line.contains("[alpha-tag]"))
                            .expect("captured identity before opening dropdown")
                            .to_owned();
                        assert!(
                            dialog.contains("Save") && dialog.contains("Cancel"),
                            "{dialog}"
                        );
                        for key in [KeyCode::Enter, KeyCode::Down] {
                            sender.send(provider_filter_key(key)).unwrap();
                        }
                        phase = 4;
                    }
                    4 if frame.contains("Filter:") && frame.contains("Allow") => {
                        let dropdown = actual_dialog_text(&frame, "Filter:");
                        assert!(
                            dropdown.contains("● Disallow") && !dropdown.contains("Select"),
                            "the saved policy stays checked while Allow is highlighted: {dropdown}"
                        );
                        let (row, line) = frame
                            .lines()
                            .enumerate()
                            .find(|(_, line)| line.contains("Allow") && !line.contains("Disallow"))
                            .expect("actual Allow option");
                        let column = unicode_width::UnicodeWidthStr::width(
                            line.get(..line.find("Allow").unwrap()).unwrap(),
                        );
                        assert_eq!(
                            canvas
                                .cell(column, row)
                                .unwrap()
                                .text_style()
                                .unwrap()
                                .color,
                            Some(Color::Yellow),
                            "{frame}"
                        );
                        evidence.push(("expanded", frame.clone()));
                        sender
                            .send(TerminalEvent::BracketedPaste("zznomatchzz".to_owned()))
                            .unwrap();
                        phase = 5;
                    }
                    5 if frame.contains("No match") => {
                        let dropdown = actual_dialog_text(&frame, "Filter:");
                        assert!(
                            dropdown.contains("Filter: zznomatchzz")
                                && dropdown.contains("No match")
                                && !dropdown.contains("Select"),
                            "{dropdown}"
                        );
                        let (row, line) = frame
                            .lines()
                            .enumerate()
                            .find(|(_, line)| line.contains("No match"))
                            .unwrap();
                        let column = unicode_width::UnicodeWidthStr::width(
                            line.get(..line.find("No match").unwrap()).unwrap(),
                        );
                        assert_eq!(
                            canvas
                                .cell(column, row)
                                .unwrap()
                                .text_style()
                                .unwrap()
                                .color,
                            Some(Color::Grey),
                            "empty result is visibly inert: {frame}"
                        );
                        evidence.push(("empty", frame.clone()));
                        for key in [KeyCode::Enter, KeyCode::Esc] {
                            sender.send(provider_filter_key(key)).unwrap();
                        }
                        phase = 6;
                    }
                    6 if frame.contains("Credit usage")
                        && frame.contains("Disallow ▾")
                        && !frame.contains("Filter:") =>
                    {
                        let dialog = actual_dialog_text(&frame, "Credit usage");
                        assert!(
                            dialog.lines().any(|line| line == captured_identity),
                            "dropdown cancellation preserves the same captured account: {dialog}"
                        );
                        evidence.push(("choice-cancelled", frame.clone()));
                        for key in [KeyCode::Enter, KeyCode::Down, KeyCode::Enter] {
                            sender.send(provider_filter_key(key)).unwrap();
                        }
                        phase = 7;
                    }
                    7 if frame.contains("Credit usage")
                        && frame.contains("Allow ▾")
                        && !frame.contains("Filter:") =>
                    {
                        let dialog = actual_dialog_text(&frame, "Credit usage");
                        assert!(
                            !dialog.contains("Disallow"),
                            "only the draft changes: {dialog}"
                        );
                        assert!(
                            dialog.lines().any(|line| line == captured_identity),
                            "{dialog}"
                        );
                        evidence.push(("draft", frame.clone()));
                        sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
                        phase = 8;
                    }
                    8 if frame.contains("Options · Credits") => {
                        sender.send(primary_key_event()).unwrap();
                        phase = 9;
                    }
                    9 if frame.contains("Credit usage") && frame.contains("Disallow ▾") => {
                        let dialog = actual_dialog_text(&frame, "Credit usage");
                        assert!(
                            dialog.lines().any(|line| line == captured_identity),
                            "cancel preserves committed policy and captured identity: {dialog}"
                        );
                        evidence.push(("draft-cancelled", frame.clone()));
                        sender
                            .send(TerminalEvent::Key(clarity_control_key('c')))
                            .unwrap();
                        phase = 10;
                    }
                    _ => {}
                }
                last = frame;
            }
        };
        let result = tokio::time::timeout(std::time::Duration::from_secs(4), journey).await;
        driver.abort();
        if let Err(error) = driver.await {
            assert!(error.is_cancelled(), "{error}");
        }
        for (stage, frame) in evidence {
            println!("CREDIT CHOICE {width}x{height} {stage}:\n{frame}");
        }
        assert!(
            result.is_ok() && phase == 10,
            "{width}x{height} phase{phase}: {last}"
        );
        assert_eq!(
            *outcome.lock().unwrap(),
            Some(crate::QuotaOutcome::Quit),
            "Ctrl+C must reach quota's own outcome"
        );
        assert_eq!(
            saves.load(Ordering::SeqCst),
            0,
            "choice Enter/cancellation must never save"
        );
    }
}

#[tokio::test]
async fn no_match_select_is_disabled_and_pointer_and_enter_preserve_query_through_help() {
    // R14 has no Select button: the real no-match row is inert for pointer and Enter.
    for (width, height) in [(120usize, 36usize), (48, 12), (40, 24), (32, 24)] {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let outcome = std::sync::Arc::new(std::sync::Mutex::new(None));
        let mut component = element!(QuotaStatusComponent(view_model:quota_two_account_view_model(),width,height,outcome_sink:Some(outcome.clone())));
        let frames = crate::quota_test_support::recording_backend::recording_terminal_render_loop(
            &mut component,
            MockTerminalConfig::with_events(events),
        );
        tokio::pin!(frames);
        let mut phase = 0;
        let mut last = String::new();
        let journey = async {
            while let Some(canvas) =
                crate::quota_frame_observation::next_current_frame(frames.as_mut())
                    .await
                    .unwrap()
            {
                let frame = canvas.to_string();
                match phase {
                    0 => {
                        sender.send(provider_filter_key(KeyCode::F(3))).unwrap();
                        phase = 1;
                    }
                    1 if frame.contains("Account filters") => {
                        sender.send(provider_filter_key(KeyCode::Enter)).unwrap();
                        phase = 2;
                    }
                    2 if frame.contains("Filter:") => {
                        sender
                            .send(TerminalEvent::BracketedPaste("zznomatch".into()))
                            .unwrap();
                        phase = 3;
                    }
                    3 if frame.contains("No match") => {
                        let query = frame.lines().find(|line| line.contains("Filter:")).unwrap();
                        assert!(
                            query.contains("zznomatch"),
                            "query remains on its Filter row: {frame}"
                        );
                        assert!(
                            !frame.contains("Select"),
                            "empty dropdown has no selection action: {frame}"
                        );
                        let (row, line) = frame
                            .lines()
                            .enumerate()
                            .find(|(_, line)| line.contains("No match"))
                            .unwrap();
                        let column = unicode_width::UnicodeWidthStr::width(
                            line.get(..line.find("No match").unwrap()).unwrap(),
                        );
                        assert_eq!(
                            canvas
                                .cell(column, row)
                                .unwrap()
                                .text_style()
                                .unwrap()
                                .color,
                            Some(Color::Grey),
                            "no available option remains visibly muted: {frame}"
                        );
                        sender
                            .send(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                                MouseEventKind::Down(MouseButton::Left),
                                u16::try_from(column).unwrap(),
                                u16::try_from(row).unwrap(),
                            )))
                            .unwrap();
                        sender.send(provider_filter_key(KeyCode::Enter)).unwrap();
                        sender.send(provider_filter_key(KeyCode::F(1))).unwrap();
                        phase = 4;
                    }
                    4 if crate::quota_browse_presentation_test::quota_help_visible(&frame) => {
                        sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
                        phase = 5;
                    }
                    5 if frame.contains("No match") && frame.contains("zznomatch") => {
                        sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
                        phase = 6;
                    }
                    6 if frame.contains("Account filters") && !frame.contains("Filter:") => {
                        assert!(
                            frame.contains("All ▾"),
                            "invalid Enter and pointer input cannot change the provider field: {frame}"
                        );
                        sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
                        phase = 7;
                    }
                    7 if !frame.contains("Account filters")
                        && frame.chars().filter(|character| *character == '╮').count() == 1 =>
                    {
                        assert!(
                            frame.contains("2/2"),
                            "invalid selection cannot filter an account: {frame}"
                        );
                        sender.send(provider_filter_key(KeyCode::F(3))).unwrap();
                        phase = 8;
                    }
                    8 if frame.contains("Account filters") => {
                        assert!(
                            frame.contains("All ▾")
                                && frame.contains("Shown ▾")
                                && !frame.contains("zznomatch"),
                            "cancellation preserves committed values: {frame}"
                        );
                        sender
                            .send(TerminalEvent::Key(clarity_control_key('c')))
                            .unwrap();
                        phase = 9;
                    }
                    _ => {}
                }
                last = frame;
            }
        };
        let result = tokio::time::timeout(std::time::Duration::from_secs(3), journey).await;
        assert!(
            result.is_ok() && phase == 9,
            "{width}x{height} phase{phase}: {last}"
        );
        assert_eq!(*outcome.lock().unwrap(), Some(crate::QuotaOutcome::Quit));
    }
}
