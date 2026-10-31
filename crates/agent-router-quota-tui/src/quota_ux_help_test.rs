//! Narrow contextual Help exposes current facts and captured identity without losing a draft.
use super::*;

#[tokio::test]
async fn narrow_credit_help_scrolls_to_full_identity_and_current_counts_then_restores_query() {
    const FULL_NAME: &str = "東京 Aurora Research Alpha Extended Identity";
    const COUNTS: [&str; 11] = [
        "Accounts 14",
        "Usable 2",
        "Reserve 3",
        "Blocked 4",
        "Pool unknown 5",
        "Excluded 0",
        "Clients 4294967295",
        "Fresh 6",
        "Stale 7",
        "Data unknown 8",
        "alpha-tag",
    ];
    for (width, height) in [(120usize, 36usize), (40, 24), (32, 24), (48, 12)] {
        let mut model = current_help_census_model();
        model.rows[0].account = FULL_NAME.to_owned();
        model.rows[0].details.account = FULL_NAME.to_owned();
        let save_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed_saves = save_calls.clone();
        let saver: super::super::super::quota_account_options::CreditUsagePolicySaver =
            std::sync::Arc::new(move |_, _, policy| {
                observed_saves.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Box::pin(async move { Ok(policy) })
            });
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async move {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let outcome = std::sync::Arc::new(std::sync::Mutex::new(None));
        let mut component = element!(QuotaStatusComponent(view_model:model,width,height,credit_policy_saver:Some(saver),outcome_sink:Some(outcome.clone())));
        let frames = crate::quota_test_support::recording_backend::recording_terminal_render_loop(
            &mut component,
            MockTerminalConfig::with_events(events),
        );
        tokio::pin!(frames);
        let mut phase = 0;
        let mut counts_seen = [false; COUNTS.len()];
        let name_words = FULL_NAME.split_whitespace().collect::<Vec<_>>();
        let mut name_seen = vec![false; name_words.len()];
        let mut full_name_seen = false;
        let mut selected_name_tokens: Vec<String> = Vec::new();
        let mut reading_selected_name = false;
        let mut captured_before = String::new();
        let mut captured_after = String::new();
        let mut dropdown_before = String::new();
        let guidance = [
            "Included quota on eligible accounts always comes first.",
            "Allow is limited to eligible OpenAI Responses requests.",
            "Inspect reset eligibility. No reset is consumed without explicit confirmation.",
        ];
        let mut guidance_seen = [false; 3];
        let mut scroll_steps = 0;
        let mut identity_evidence = Vec::new();
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
                    1 if frame.contains("Options · Resets") && frame.contains("Inspect") => {
                        sender
                            .send(crate::quota_test_support::shift_tab_event())
                            .unwrap();
                        sender.send(provider_filter_key(KeyCode::Enter)).unwrap();
                        phase = 2;
                    }
                    2 if frame.contains("Options · Credits")
                        && !frame.contains("Tab change pending") =>
                    {
                        sender.send(provider_filter_key(KeyCode::Tab)).unwrap();
                        sender.send(provider_filter_key(KeyCode::Tab)).unwrap();
                        sender.send(provider_filter_key(KeyCode::Enter)).unwrap();
                        phase = 3;
                    }
                    3 if frame.contains("Credit usage")
                        && frame.contains("Save")
                        && !frame.contains("Filter:") =>
                    {
                        captured_before = actual_dialog_text(&frame, "Credit usage")
                            .lines()
                            .find(|line| line.contains("[alpha-tag]"))
                            .expect("captured parent identity before the dropdown covers it")
                            .to_owned();
                        sender.send(provider_filter_key(KeyCode::Enter)).unwrap();
                        phase = 4;
                    }
                    4 if frame.contains("Filter:") => {
                        sender
                            .send(TerminalEvent::BracketedPaste("zzhelp".to_owned()))
                            .unwrap();
                        phase = 5;
                    }
                    5 if frame.contains("Filter:")
                        && frame.contains("zzhelp")
                        && frame.contains("No match") =>
                    {
                        dropdown_before = actual_dialog_text(&frame, "Filter:");
                        sender.send(provider_filter_key(KeyCode::F(1))).unwrap();
                        phase = 6;
                    }
                    6 if crate::quota_browse_presentation_test::quota_help_visible(&frame) => {
                        let dialog = actual_dialog_text(&frame, "Help");
                        let words = dialog
                            .replace(['│', '┃'], " ")
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ");
                        if words.contains("Aurora")
                            || words.contains("Alpha")
                            || words.contains("alpha")
                            || words.contains("Identity")
                            || words.contains("tag")
                        {
                            identity_evidence.push(words.clone());
                        }
                        for (seen, expected) in counts_seen.iter_mut().zip(COUNTS) {
                            // This known bracketed identity may wrap inside its tag;
                            // all of its characters must still be visible in one Help viewport.
                            let visible_tag = expected == "alpha-tag"
                                && words
                                    .split_whitespace()
                                    .collect::<String>()
                                    .contains("[alpha-tag]");
                            *seen |= words.contains(expected) || visible_tag;
                        }
                        for (seen, expected) in guidance_seen.iter_mut().zip(guidance) {
                            *seen |= words.contains(expected);
                        }
                        record_selected_account_words(
                            &canvas,
                            &mut selected_name_tokens,
                            &mut reading_selected_name,
                        );
                        full_name_seen |= selected_name_tokens.len() >= name_words.len()
                            && selected_name_tokens[..name_words.len()]
                                .iter()
                                .map(String::as_str)
                                .eq(name_words.iter().copied());
                        for (seen, expected) in name_seen.iter_mut().zip(&name_words) {
                            *seen |= words.contains(expected);
                        }
                        if counts_seen.iter().all(|seen| *seen)
                            && name_seen.iter().all(|seen| *seen)
                            && full_name_seen
                            && (width < 120 || guidance_seen.iter().all(|seen| *seen))
                        {
                            sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
                            phase = 7;
                        } else {
                            assert!(
                                scroll_steps < 128,
                                "bounded Help scan cannot reach facts: counts={counts_seen:?},name={name_seen:?}: {dialog}"
                            );
                            sender.send(provider_filter_key(KeyCode::Down)).unwrap();
                            scroll_steps += 1;
                        }
                    }
                    7 if frame.contains("Filter:")
                        && frame.contains("zzhelp")
                        && frame.contains("No match") =>
                    {
                        assert_eq!(
                            actual_dialog_text(&frame, "Filter:"),
                            dropdown_before,
                            "Help preserves the exact dropdown query/results"
                        );
                        sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
                        phase = 8;
                    }
                    8 if frame.contains("Credit usage")
                        && frame.contains("Save")
                        && !frame.contains("Filter:") =>
                    {
                        captured_after = actual_dialog_text(&frame, "Credit usage")
                            .lines()
                            .find(|line| line.contains("[alpha-tag]"))
                            .unwrap()
                            .to_owned();
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
            "{width}x{height} phase{phase} scroll={scroll_steps} counts={counts_seen:?} name={name_seen:?} identity_evidence={identity_evidence:?}: {last}"
        );
        assert!(
            full_name_seen,
            "full captured name must be visible in actual Help, not background: {identity_evidence:?}"
        );
        assert_eq!(
            captured_before, captured_after,
            "Help returns to the same captured account and query"
        );
        if width < 120 {
            assert!(
                captured_before.contains('…'),
                "actual truncated draft identity needs a continuation cue: {captured_before}"
            );
        }
        assert_eq!(
            *outcome.lock().unwrap(),
            Some(crate::QuotaOutcome::Quit),
            "typed Ctrl+C must reach quota itself"
        );
        assert_eq!(
            save_calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "Help and no-match query cannot persist policy"
        );
    }
}

#[tokio::test]
async fn phone_header_preserves_named_nonzero_pool_and_data_problems() {
    // C6 keeps operational failures visible but moves routine census to Pool details.
    for width in [32usize, 40, 48] {
        let mut model = current_help_census_model();
        model.header.credential_notice = Some("Credentials locked".to_owned());
        model.selection_projection_degraded = true;
        let frame = render_quota_capture_model_at(
            model,
            width,
            24,
            vec![provider_filter_key(KeyCode::Esc)],
        )
        .await;
        let words = frame
            .replace(['│', '┃'], " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        for fact in [
            "Menu F10",
            "Credentials locked",
            "Selection projection degraded",
        ] {
            assert!(words.contains(fact), "{frame}");
        }
        for routine in ["Accounts 14", "Usable 2", "Stale 7", "Data unknown 8"] {
            assert!(!words.contains(routine), "{frame}");
        }
    }
}

fn current_help_census_model() -> QuotaStatusViewModel {
    let mut model = quota_two_account_view_model();
    model.header.observed_clients = Some(4294967295);
    model.header.pool = codex_router_quota_service::quota_display_facts::QuotaPoolCensus {
        usable: 2,
        reserve: 3,
        blocked: 4,
        unknown: 5,
        excluded: 0,
    };
    model.header.freshness =
        codex_router_quota_service::quota_display_facts::QuotaFreshnessCensus {
            fresh: 6,
            stale: 7,
            unknown: 8,
        };
    model
}

#[tokio::test]
async fn actual_help_body_explains_the_reset_apply_exception_to_exit() {
    for (width, height) in [(120usize, 36usize), (32, 24)] {
        let frames = render_quota_filter_phases(
            quota_two_account_view_model(),
            width,
            height,
            vec![
                ("Quota ·", vec![provider_filter_key(KeyCode::F(1))]),
                ("Help", vec![provider_filter_key(KeyCode::End)]),
                ("Ctrl+C", vec![provider_filter_key(KeyCode::Esc)]),
                (
                    "Quota ·",
                    vec![TerminalEvent::Key(clarity_control_key('d'))],
                ),
            ],
            None,
        )
        .await;
        let help_bodies = frames
            .iter()
            .filter(|frame| crate::quota_browse_presentation_test::quota_help_visible(frame))
            .map(|frame| {
                actual_dialog_text(frame, "Help")
                    .replace(['│', '┃'], " ")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect::<Vec<_>>();
        println!("ACTUAL HELP EXIT {width}x{height}: {help_bodies:?}");
        assert!(
            help_bodies
                .iter()
                .any(|body| body.contains("Exit Quota unless a reset is being applied.")),
            "actual Help must describe its commit-time exception: {help_bodies:?}"
        );
        assert!(
            help_bodies
                .iter()
                .all(|body| !body.contains("Existing forced-exit rules"))
        );
    }
}

// Read the real value cells rather than flattening the gray Account label
// into a repeated first name word when the Help viewport scrolls.
pub(crate) fn record_selected_account_words(
    canvas: &iocraft::Canvas,
    captured: &mut Vec<String>,
    reading: &mut bool,
) {
    let frame = canvas.to_string();
    let lines = frame.lines().collect::<Vec<_>>();
    let title = lines
        .iter()
        .position(|line| crate::quota_browse_presentation_test::quota_help_visible(line))
        .unwrap();
    let top = (0..title)
        .rev()
        .find(|&row| lines[row].contains('╭') && lines[row].contains('╮'))
        .unwrap();
    let column_at = |line: &str, byte: usize| {
        unicode_width::UnicodeWidthStr::width(line.get(..byte).expect("border character boundary"))
    };
    let left = column_at(lines[top], lines[top].find('╭').unwrap());
    let right = column_at(lines[top], lines[top].find('╮').unwrap());
    let bottom = (title..lines.len())
        .find(|&row| {
            lines[row]
                .find('╰')
                .is_some_and(|byte| column_at(lines[row], byte) == left)
        })
        .unwrap();
    let mut visible = Vec::new();
    for (row, line) in lines.iter().enumerate().take(bottom).skip(title + 1) {
        let mut selected_row = String::new();
        let mut row_text = String::new();
        let mut column = 0;
        for character in line.chars() {
            let start = column;
            column += unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
            if start <= left || start >= right {
                continue;
            }
            row_text.push(character);
            let value_cell = canvas
                .cell(start, row)
                .and_then(|cell| cell.text_style())
                .is_some_and(|style| {
                    style.color == Some(Color::White) && style.weight == Weight::Normal
                });
            selected_row.push(if value_cell { character } else { ' ' });
        }
        if row_text.trim_start().starts_with("Account ID") {
            *reading = false;
            break;
        }
        if row_text.trim_start().starts_with("Account ") {
            *reading = true;
        }
        if row_text.contains("Screen") || row_text.contains("Input conventions") {
            *reading = false;
            break;
        }
        if *reading && row_text.replace(['│', '┃'], " ").trim() != "Close" {
            visible.extend(selected_row.split_whitespace().map(str::to_owned));
        }
    }
    let overlap = (0..=visible.len().min(captured.len()))
        .rev()
        .find(|&count| captured[captured.len() - count..] == visible[..count])
        .unwrap();
    captured.extend(visible.into_iter().skip(overlap));
}
