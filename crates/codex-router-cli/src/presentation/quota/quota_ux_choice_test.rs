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
        let mut component = element!(QuotaStatusComponent(view_model:model,width,height,
            credit_policy_saver:Some(saver),reset_intent_sender:Some(ports.intent_sender),
            reset_snapshot_receiver:Some(ports.snapshot_receiver)));
        let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        let mut phase = 0;
        let mut evidence = Vec::new();
        let journey = async {
            while let Some(canvas) =
                crate::presentation::quota::quota_frame_observation::next_current_frame(
                    frames.as_mut(),
                )
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
                        sender.send(provider_filter_key(KeyCode::Tab)).unwrap();
                        phase = 2;
                    }
                    2 if frame.contains("Options · Credits") => {
                        for _ in 0..256 {
                            sender.send(provider_filter_key(KeyCode::Down)).unwrap();
                        }
                        sender.send(provider_filter_key(KeyCode::Enter)).unwrap();
                        sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
                        phase = 3;
                    }
                    3 if frame.contains("Credit usage") && frame.contains("Save") => {
                        assert!(actual_dialog_text(&frame, "Credit usage").contains("Disallow"));
                        sender.send(provider_filter_key(KeyCode::Right)).unwrap();
                        phase = 4;
                    }
                    4 if frame.contains("Credit usage") && frame.contains("Select") => {
                        let dialog = actual_dialog_text(&frame, "Credit usage");
                        assert!(
                            dialog.contains("[alpha-tag]") && dialog.contains("Cancel"),
                            "{frame}"
                        );
                        let (row, line) = frame
                            .lines()
                            .enumerate()
                            .find(|(_, line)| line.contains('❯') && line.contains("Allow"))
                            .expect("actual focused Allow row");
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
                        evidence.push(("expanded", frame));
                        sender
                            .send(TerminalEvent::BracketedPaste("zznomatchzz".to_owned()))
                            .unwrap();
                        phase = 5;
                    }
                    5 if frame.contains("No matching choices") => {
                        let dialog = actual_dialog_text(&frame, "Credit usage");
                        let words = dialog.split_whitespace().collect::<Vec<_>>().join(" ");
                        assert!(
                            words.contains("[alpha-tag]")
                                && words.contains("Use credits Search: zznomatchzz")
                                && words.contains("No matching choices"),
                            "{frame}"
                        );
                        let (row, line) = frame
                            .lines()
                            .enumerate()
                            .find(|(_, line)| line.contains("Select"))
                            .unwrap();
                        let column = unicode_width::UnicodeWidthStr::width(
                            line.get(..line.find("Select").unwrap()).unwrap(),
                        );
                        assert_eq!(
                            canvas
                                .cell(column, row)
                                .unwrap()
                                .text_style()
                                .unwrap()
                                .color,
                            Some(Color::DarkGrey),
                            "{frame}"
                        );
                        evidence.push(("empty", frame));
                        sender.send(provider_filter_key(KeyCode::Enter)).unwrap();
                        sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
                        phase = 6;
                    }
                    6 if frame.contains("Credit usage") && frame.contains("Save") => {
                        assert!(
                            actual_dialog_text(&frame, "Credit usage").contains("Disallow"),
                            "invalid Enter must not select a value: {frame}"
                        );
                        evidence.push(("choice-cancelled", frame));
                        sender.send(provider_filter_key(KeyCode::Right)).unwrap();
                        sender.send(provider_filter_key(KeyCode::Enter)).unwrap();
                        phase = 7;
                    }
                    7 if frame.contains("Credit usage") && frame.contains("Save") => {
                        let dialog = actual_dialog_text(&frame, "Credit usage");
                        assert!(
                            dialog.contains("Allow") && !dialog.contains("Disallow"),
                            "real choice must change only draft: {frame}"
                        );
                        evidence.push(("draft", frame));
                        sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
                        phase = 8;
                    }
                    8 if frame.contains("Options · Credits") => {
                        sender.send(provider_filter_key(KeyCode::Enter)).unwrap();
                        sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
                        phase = 9;
                    }
                    9 if frame.contains("Credit usage") && frame.contains("Save") => {
                        assert!(
                            actual_dialog_text(&frame, "Credit usage").contains("Disallow"),
                            "cancel must preserve committed policy: {frame}"
                        );
                        evidence.push(("draft-cancelled", frame));
                        sender
                            .send(TerminalEvent::Key(clarity_control_key('d')))
                            .unwrap();
                        phase = 10;
                    }
                    _ => {}
                }
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
            "{width}x{height} phase{phase}"
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
    for (width, height) in [(120usize, 36usize), (48, 12), (40, 24), (32, 24)] {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let mut component =
            element!(QuotaStatusComponent(view_model:quota_two_account_view_model(),width,height));
        let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        let mut phase = 0;
        let mut last = String::new();
        let journey = async {
            while let Some(canvas) =
                crate::presentation::quota::quota_frame_observation::next_current_frame(
                    frames.as_mut(),
                )
                .await
                .expect("bounded current Quota canvas")
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
                    2 if frame.contains("Search:") || frame.contains("Search:") => {
                        sender
                            .send(TerminalEvent::BracketedPaste("zznomatch".to_owned()))
                            .unwrap();
                        phase = 3;
                    }
                    3 if frame.contains("No matching choices") => {
                        let dialog = actual_dialog_text(&frame, "Account filters");
                        let (row, line) = frame
                            .lines()
                            .enumerate()
                            .find(|(_, line)| line.contains("Select"))
                            .expect("open choices must expose Select rather than Apply");
                        let offset = line.find("Select").unwrap();
                        let column =
                            unicode_width::UnicodeWidthStr::width(line.get(..offset).unwrap());
                        assert_eq!(
                            canvas
                                .cell(column, row)
                                .unwrap()
                                .text_style()
                                .unwrap()
                                .color,
                            Some(Color::DarkGrey),
                            "no available choice means visibly disabled Select: {dialog}"
                        );
                        let words = dialog.split_whitespace().collect::<Vec<_>>().join(" ");
                        assert!(
                            words.contains("Account provider Search: zznomatch")
                                && words.contains("No matching choices"),
                            "field/query/result must stay together: {dialog}"
                        );
                        sender
                            .send(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                                MouseEventKind::Down(MouseButton::Left),
                                (column + 1) as u16,
                                row as u16,
                            )))
                            .unwrap();
                        sender.send(provider_filter_key(KeyCode::Enter)).unwrap();
                        sender.send(provider_filter_key(KeyCode::F(1))).unwrap();
                        phase = 4;
                    }
                    4 if frame.contains("Quota Help") => {
                        sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
                        phase = 5;
                    }
                    5 if frame.contains("Account filters")
                        && frame.contains("zznomatch")
                        && frame.contains("No matching choices") =>
                    {
                        sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
                        phase = 6;
                    }
                    6 if frame.contains("Account filters")
                        && !frame.contains("No matching choices") =>
                    {
                        sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
                        phase = 7;
                    }
                    7 if !frame.contains("Account filters") && frame.contains("Quota status") => {
                        assert!(
                            frame.contains("All") && frame.contains("2/2"),
                            "invalid choice cannot commit another filter: {frame}"
                        );
                        sender
                            .send(TerminalEvent::Key(clarity_control_key('d')))
                            .unwrap();
                        phase = 8;
                    }
                    _ => {}
                }
                last = frame;
            }
        };
        let result = tokio::time::timeout(std::time::Duration::from_secs(3), journey).await;
        assert!(
            result.is_ok() && phase == 8,
            "{width}x{height} phase{phase}: {last}"
        );
    }
}
