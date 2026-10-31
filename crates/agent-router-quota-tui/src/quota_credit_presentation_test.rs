//! Credit policy fields, wrapped notices and real short-editor interaction.
use super::*;

#[tokio::test]
async fn credit_policy_options_render_in_keyboard_selection_order() {
    let model = quota_two_account_view_model();
    let mut options = AccountOptionsState::new(&model.rows[0], 1);
    options.tab = AccountOptionsTab::Credits;
    update_credit_policy_editor(&mut options, AccountOptionsKeyAction::BeginEditing, None);
    for (action, policy, label) in [
        (
            AccountOptionsKeyAction::SelectDisallow,
            CreditUsagePolicy::Disallow,
            "Disallow",
        ),
        (
            AccountOptionsKeyAction::SelectAllow,
            CreditUsagePolicy::Allow,
            "Allow",
        ),
    ] {
        update_credit_policy_editor(&mut options, action, None);
        let editor = options.editor.as_mut().unwrap();
        assert_eq!(editor.draft_policy, policy);
        let mut fixture_model = model.clone();
        fixture_model.rows[0].credit_usage.policy = policy;
        let mut fixture =
            crate::test_support::quota_control_fixture(crate::test_support::QuotaFixtureOptions {
                view_model: fixture_model,
                ports: Default::default(),
                width: 100,
                height: 30,
                control: crate::test_support::QuotaFixtureControl::CreditChoices,
            })
            .element;
        let frames = fixture.mock_terminal_render_loop(MockTerminalConfig::with_events(
            futures_util::stream::pending(),
        ));
        tokio::pin!(frames);
        let frame = super::super::super::quota_frame_observation::next_matching_frame(
            frames.as_mut(),
            Duration::from_secs(2),
            |canvas| canvas.to_string().contains("Allow"),
        )
        .await
        .unwrap()
        .to_string();
        let disallow = frame
            .lines()
            .position(|line| line.contains("Disallow"))
            .expect("Disallow choice");
        let allow = frame
            .lines()
            .position(|line| line.contains("Allow"))
            .expect("Allow choice");
        assert!(
            disallow < allow,
            "choices follow the shared keyboard order: {frame}"
        );
        assert!(
            frame.lines().any(|line| line
                .contains(agent_collaboration_tui::theme::UiGlyph::Current.text())
                && line.contains(label)),
            "captured typed policy remains selected: {frame}"
        );
    }
}

#[test]
fn credit_refresh_is_blocked_locally_for_disabled_or_credentialless_targets() {
    use codex_router_quota_service::quota_reset::reset_session_supervisor::WorkflowPhase;

    let view_model = quota_two_account_view_model();
    for (enabled, credential_generation, reason) in [
        (
            false,
            Some(7),
            CreditUsageRefreshUnavailableReason::AccountDisabled,
        ),
        (
            true,
            None,
            CreditUsageRefreshUnavailableReason::CredentialsUnavailable,
        ),
    ] {
        let mut options = AccountOptionsState::new(&view_model.rows[0], 1);
        options.tab = AccountOptionsTab::Credits;
        options.target.enabled = enabled;
        options.target.credential_generation = credential_generation;
        let action = account_options_key_action(
            &options,
            WorkflowPhase::Browse,
            KeyCode::Char('r'),
            KeyModifiers::empty(),
        );

        let command = update_credit_policy_editor(&mut options, action, None);

        assert_eq!(command, None);
        assert_eq!(
            options.message,
            Some(AccountOptionsMessage::RefreshUnavailable(reason))
        );
        let frame = render_account_options_panel(AccountOptionsPanelProps {
            options: &options,
            reset_snapshot: None,
            reset_target: None,
            width: 100,
            height: 24,
            inventory_page_start: 0,
            spinner_tick: 0,
        })
        .render(None)
        .to_string();
        let message = account_options_message_text(options.message.unwrap());
        assert!(message.contains("unavailable"));
        assert!(frame.contains(message), "{frame}");
    }

    let mut unsupported_provider_options = AccountOptionsState::new(&view_model.rows[0], 1);
    unsupported_provider_options.tab = AccountOptionsTab::Credits;
    unsupported_provider_options.message = Some(AccountOptionsMessage::RefreshUnavailable(
        CreditUsageRefreshUnavailableReason::ProviderUnsupported,
    ));
    let unsupported_provider_frame = render_account_options_panel(AccountOptionsPanelProps {
        options: &unsupported_provider_options,
        reset_snapshot: None,
        reset_target: None,
        width: 100,
        height: 24,
        inventory_page_start: 0,
        spinner_tick: 0,
    })
    .render(None)
    .to_string();
    assert!(
        unsupported_provider_frame.contains("OpenAI accounts only can refresh credits."),
        "{unsupported_provider_frame}"
    );
}

#[test]
fn credit_options_wrap_dynamic_status_and_help_inside_narrow_pane() {
    let view_model = quota_two_account_view_model();
    let mut options = AccountOptionsState::new(&view_model.rows[0], 1);
    options.tab = AccountOptionsTab::Credits;
    options.target.weekly_quota_floor_percent = 95;
    options.target.credit_usage = codex_router_quota_service::quota::CreditUsageStatus {
        policy: CreditUsagePolicy::Disallow,
        provider_observation: codex_router_core::credit_usage::CreditProviderObservation::new(
            CreditAvailability::Unknown,
            codex_router_core::credit_usage::CreditSpendControl::Unknown,
            Some(codex_router_core::credit_usage::CreditProviderLimitReason::Unknown),
        ),
        freshness: CreditUsageFreshness::Stale,
        age_label: "very old synthetic sample".to_owned(),
    };
    options.editor = Some(CreditPolicyEditorState {
        saved_policy: CreditUsagePolicy::Disallow,
        draft_policy: CreditUsagePolicy::Allow,
        phase: CreditPolicyEditorPhase::SaveFailed(
            CreditUsagePolicySaveError::StateOperationFailed,
        ),
        operation_generation: 2,
    });
    options.message = Some(AccountOptionsMessage::RefreshFailed);

    let frame = render_account_options_panel(AccountOptionsPanelProps {
        options: &options,
        reset_snapshot: None,
        reset_target: None,
        width: 48,
        height: 32,
        inventory_page_start: 0,
        spinner_tick: 0,
    })
    .render(None)
    .to_string();

    assert!(frame.contains("Weekly floor"), "{frame}");
    assert!(
        frame.contains("blocks credit") && frame.contains("routing."),
        "{frame}"
    );
    assert!(
        !frame.contains("Included quota"),
        "routine guidance belongs in contextual Help: {frame}"
    );
    assert!(frame.contains("Provider spend-control"), "{frame}");
    assert!(
        frame.contains("credit preference save") && frame.contains("failed"),
        "{frame}"
    );
    assert!(frame.contains("Credit refresh failed"), "{frame}");
    assert!(
        !frame.contains('…'),
        "notes wrap inside the body rather than lose information: {frame}"
    );
    assert!(
        frame
            .lines()
            .all(|line| unicode_width::UnicodeWidthStr::width(line) <= 48),
        "{frame}"
    );
}

#[tokio::test]
async fn short_quota_options_keeps_credit_policy_and_footer_visible() {
    for width in [100usize, 48] {
        let events = vec![
            (TerminalEvent::Key(control_key('r')), Some("Inspect")),
            (
                TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Tab)),
                None,
            ),
            (
                TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Right)),
                None,
            ),
            (
                TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Enter)),
                Some("Credit usage"),
            ),
            (
                TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Tab)),
                None,
            ),
            (
                TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Enter)),
                Some("Save"),
            ),
            (
                TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Enter)),
                Some("Filter:"),
            ),
            (TerminalEvent::Key(control_key('c')), None),
        ];

        let frames = render_account_options_component_frames(
            quota_two_account_view_model(),
            width,
            24,
            events,
            None,
        )
        .await;

        assert!(
            frames.iter().any(|frame| {
                frame.contains("Credit usage")
                    && frame.contains("Disallow")
                    && frame.contains("Filter:")
                    && !frame.contains("Select")
            }),
            "a {width}x24 credits pane must keep the active setting and editor footer visible:\n{}",
            frames.join("\n--- frame ---\n")
        );
    }
}

#[tokio::test]
async fn short_credit_editor_keeps_save_error_with_floor_and_provider_notes_visible() {
    for width in [100usize, 48] {
        let mut view_model = quota_two_account_view_model();
        view_model.rows[0].weekly_quota_floor_percent = 95;
        view_model.rows[0].credit_usage = codex_router_quota_service::quota::CreditUsageStatus {
            policy: CreditUsagePolicy::Disallow,
            provider_observation: codex_router_core::credit_usage::CreditProviderObservation::new(
                CreditAvailability::Unknown,
                codex_router_core::credit_usage::CreditSpendControl::Unknown,
                Some(codex_router_core::credit_usage::CreditProviderLimitReason::Unknown),
            ),
            freshness: CreditUsageFreshness::Stale,
            age_label: "very old synthetic sample".to_owned(),
        };
        let expected_account = view_model.rows[0].account_id.clone();
        let policy_saver: CreditUsagePolicySaver =
            Arc::new(move |account_id, _generation, policy| {
                assert_eq!(account_id, expected_account);
                assert_eq!(policy, CreditUsagePolicy::Allow);
                Box::pin(async { Err(CreditUsagePolicySaveError::StateOperationFailed) })
            });
        let events = vec![
            (TerminalEvent::Key(control_key('r')), Some("Inspect".into())),
            (
                TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Tab)),
                None,
            ),
            (
                TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Right)),
                None,
            ),
            (
                TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Enter)),
                Some("Credit usage".into()),
            ),
            (
                TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Tab)),
                None,
            ),
            (
                TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Enter)),
                Some("Save".into()),
            ),
            (
                TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Enter)),
                Some("Filter:".into()),
            ),
            (
                TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Down)),
                Some("Allow".into()),
            ),
            (
                TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Enter)),
                Some("Save".into()),
            ),
            (
                TerminalEvent::Key({
                    let mut key = KeyEvent::new(KeyEventKind::Press, KeyCode::Enter);
                    key.modifiers = KeyModifiers::CONTROL;
                    key
                }),
                Some("credit preference".into()),
            ),
            // The narrow body cannot show all three wrapped notices together.
            // Navigate the same failed editor; keep the error and both warnings reachable.
            (
                TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Up)),
                None,
            ),
            (
                TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Up)),
                Some("Weekly floor".into()),
            ),
            (
                TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::End)),
                Some(ExpectedCreditFrame::DispatchFrame("credit preference")),
            ),
            (TerminalEvent::Key(control_key('c')), None),
        ];
        let frames = render_account_options_component_frames(
            view_model,
            width,
            24,
            events,
            Some(policy_saver),
        )
        .await;
        let failed_frame = frames
            .iter()
            .find(|frame| frame.contains("credit preference"))
            .expect("save error should remain visible at short viewport height");

        for marker in ["Provider spend-control", "failed", "Cancel", "Save"] {
            assert!(
                failed_frame.contains(marker),
                "missing {marker}:\n{failed_frame}"
            );
        }
        let error_index = frames
            .iter()
            .position(|frame| frame.contains("credit preference"))
            .unwrap();
        assert!(
            frames[error_index..]
                .iter()
                .any(|frame| frame.contains("Weekly floor")
                    && frame.contains("routing.")
                    && frame.contains("Cancel")
                    && frame.contains("Save")),
            "the floor warning must remain reachable after failure: {frames:#?}"
        );
        assert!(
            failed_frame
                .lines()
                .all(|line| line.chars().count() <= width),
            "a {width}x24 credit options frame must fit the enclosing terminal:\n{failed_frame}"
        );
    }
}

#[tokio::test]
async fn composed_credit_sections_keep_a_rendered_blank_row_after_observation() {
    // Match the normal tour's fresh balance and saved floor. Inspect current paint,
    // not the first queued frame before measured-body layout has settled.
    for (width, height) in [(120usize, 36usize), (76, 30), (48, 24)] {
        let mut model = quota_two_account_view_model();
        model.rows[0].weekly_quota_floor_percent = 1;
        model.rows[0].credit_usage.freshness = CreditUsageFreshness::Fresh;
        model.rows[0].credit_usage.provider_observation =
            codex_router_core::credit_usage::CreditProviderObservation::new(
                CreditAvailability::Available {
                    balance: Some(
                        codex_router_core::credit_usage::CreditBalance::new("62500").unwrap(),
                    ),
                },
                codex_router_core::credit_usage::CreditSpendControl::Clear,
                None,
            );
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let mut component = element!(QuotaStatusComponent(view_model:model,width,height));
        let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        let mut phase = 0;
        let journey = async {
            while let Some(canvas) =
                crate::quota_frame_observation::next_current_frame(frames.as_mut())
                    .await
                    .unwrap()
            {
                let frame = canvas.to_string();
                match phase {
                    0 => {
                        sender.send(TerminalEvent::Key(control_key('r'))).unwrap();
                        phase = 1;
                    }
                    1 if frame.contains("Inspect") => {
                        sender
                            .send(TerminalEvent::Key(KeyEvent::new(
                                KeyEventKind::Press,
                                KeyCode::Tab,
                            )))
                            .unwrap();
                        sender
                            .send(TerminalEvent::Key(KeyEvent::new(
                                KeyEventKind::Press,
                                KeyCode::Right,
                            )))
                            .unwrap();
                        sender
                            .send(TerminalEvent::Key(KeyEvent::new(
                                KeyEventKind::Press,
                                KeyCode::Enter,
                            )))
                            .unwrap();
                        phase = 2;
                    }
                    2 if frame.contains("Observation") && frame.contains("Credit usage") => {
                        let dialog = crate::quota_browse_presentation_test::quota_ux_clarity_test::actual_dialog_text(&frame,"Options · Credits");
                        let rows = dialog.lines().collect::<Vec<_>>();
                        let observation = rows
                            .iter()
                            .position(|row| row.contains("Observation"))
                            .unwrap();
                        let usage = rows
                            .iter()
                            .position(|row| row.contains("Credit usage"))
                            .unwrap();
                        assert!(
                            usage >= observation + 2,
                            "{width}x{height} requires an actual empty row between observation and usage: {frame}"
                        );
                        let blank = rows[usage - 1].trim_end();
                        let blank = blank
                            .strip_suffix('█')
                            .or_else(|| blank.strip_suffix('│'))
                            .unwrap_or(blank);
                        assert!(
                            blank.trim().is_empty(),
                            "the final balance fact must be followed by an empty inner row: {dialog}"
                        );
                        assert!(
                            dialog.contains("Edit") && dialog.contains("Close"),
                            "actions remain visible: {frame}"
                        );
                        sender.send(TerminalEvent::Key(control_key('c'))).unwrap();
                        phase = 3;
                    }
                    _ => {}
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(2), journey)
            .await
            .unwrap();
        assert_eq!(phase, 3);
    }
}
