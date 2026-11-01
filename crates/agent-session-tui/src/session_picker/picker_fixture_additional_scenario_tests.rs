//! The added scenarios reach the real picker and captured outcome boundary.
use super::*;
use crate::session_picker::{
    new_session_client_inventory::NewClientInventoryState,
    new_session_form::NewSessionClient,
    picker_fork_confirmation::{ForkAvailability, ForkDestinationKey},
};
use agent_session_inventory::sessions::{
    claude_terminal_launch::ClaudeTerminalMode,
    router_connection_registry::RouterConnectionTransport,
    source_ssh_protocol::SourceSessionAction,
};

#[tokio::test]
async fn fork_ssh_profile_fixture_reaches_qualified_fork_and_returns_its_source_identity() {
    let directory = tempfile::tempdir().unwrap();
    let mut phase = 0;
    let mut fork = KeyEvent::new(KeyEventKind::Press, KeyCode::Enter);
    fork.modifiers = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
    let outcome = exercise(
        "fork-ssh-profile",
        directory.path(),
        vec![
            key(KeyCode::Char('/')),
            TerminalEvent::BracketedPaste("SSH fork source".into()),
            TerminalEvent::Key(fork),
        ],
        |canvas, _, model| {
            if phase == 0 && canvas.to_string().contains("Fork session") {
                let model = model.read();
                let captured = model.fork_confirmation.as_ref().unwrap();
                assert_eq!(captured.directory_label, "/fixture/ssh-fork");
                assert_eq!(captured.directory_origin, "source session");
                assert!(
                    captured
                        .destinations
                        .iter()
                        .any(|destination| destination.key == ForkDestinationKey::Source
                            && destination.availability == ForkAvailability::Eligible)
                );
                assert!(!canvas.to_string().contains("needs qualification"));
                phase = 1;
                return vec![ctrl(KeyCode::Enter)].into();
            }
            FixtureStep::default()
        },
    )
    .await;
    assert_eq!(phase, 1);
    let Some(SessionsPickerOutcome::PreparedSourceSession(prepared)) = outcome else {
        panic!("expected source-qualified prepared Codex fork");
    };
    let SourceSessionAction::Fork { target, .. } = &prepared.launch.action else {
        panic!("prepared handoff must retain the captured Fork action");
    };
    assert_eq!(
        String::from(target.endpoint.endpoint_id.clone()),
        "codex-fixture-ssh"
    );
    assert_eq!(
        String::from(target.session_id.clone()),
        "00000000-0000-4000-8000-000000000011"
    );
    let profile = &prepared.profile;
    assert!(matches!(
        profile.transport,
        RouterConnectionTransport::Ssh(_)
    ));
    assert_eq!(profile.service_id, target.endpoint.service_id);
    assert_eq!(prepared.launch.endpoint, target.endpoint);
    assert_eq!(
        prepared.launch.working_directory.as_str(),
        "/fixture/ssh-fork"
    );
    let receipt: serde_json::Value = serde_json::from_slice(
        &std::fs::read(directory.path().join("picker-outcome.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        receipt,
        serde_json::json!({"kind":"fork","identity":{"kind":"hostedCodex","target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-fixture-ssh"},"sessionId":"00000000-0000-4000-8000-000000000011"}}})
    );
    assert!(!directory.path().join("source-preparation.json").exists());
}

#[tokio::test]
async fn claude_new_success_fixture_qualifies_new_and_returns_the_foreground_handoff() {
    let directory = tempfile::tempdir().unwrap();
    let mut phase = 0;
    let outcome =
        exercise(
            "claude-new-success",
            directory.path(),
            vec![ctrl(KeyCode::Char('n'))],
            |canvas, stack, model| {
                let text = canvas.to_string();
                if phase == 0
                    && text.contains("New session")
                    && model.read().new_session.as_ref().is_some_and(|draft| {
                        matches!(draft.client_inventory, NewClientInventoryState::Ready(_))
                    })
                {
                    phase = 1;
                    return vec![key(KeyCode::Enter)].into();
                }
                if phase == 1 && dropdown_open(stack) {
                    phase = 2;
                    return vec![
                        TerminalEvent::BracketedPaste("Claude".into()),
                        key(KeyCode::Enter),
                    ]
                    .into();
                }
                if phase == 2
                    && !dropdown_open(stack)
                    && model.read().new_session.as_ref().is_some_and(|draft| {
                        draft.client == NewSessionClient::ClaudeNativeForeground
                    })
                {
                    assert!(text.contains("Claude Code"));
                    phase = 3;
                    return vec![ctrl(KeyCode::Enter)].into();
                }
                FixtureStep::default()
            },
        )
        .await;
    assert_eq!(phase, 3);
    let Some(SessionsPickerOutcome::LaunchNativeClaude(captured, mode)) = outcome else {
        panic!("expected captured Claude New handoff");
    };
    assert_eq!(mode, ClaudeTerminalMode::Foreground);
    assert_eq!(captured.source, PickerSourceContext::DefaultHosted);
    assert!(
        matches!(&captured.effect,SessionClientEffect::Native{action:SourceNativeClaudeAction::New{directory,arguments},..} if directory.as_str()=="/repo/project-a"&&arguments.is_empty())
    );
    let receipt: serde_json::Value = serde_json::from_slice(
        &std::fs::read(directory.path().join("picker-outcome.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        receipt,
        serde_json::json!({"kind":"new","request":{"client":"claudeCode","machine":"defaultHosted","directory":"/repo/project-a","runMode":"foreground"}})
    );
}

#[tokio::test]
async fn large_list_fixture_spans_four_viewports_and_reaches_both_ends_at_120x36() {
    let directory = tempfile::tempdir().unwrap();
    let mut phase = 0;
    let outcome = exercise_at(
        "large-list",
        directory.path(),
        Vec::new(),
        |canvas, _, model| {
            let text = canvas.to_string();
            let selected = model
                .read()
                .focused_record()
                .map(|record| record.session_id.clone());
            if phase == 0 && text.contains("❯ Large session 001") {
                assert_eq!(model.read().visible_record_len(), 60);
                let visible = text
                    .lines()
                    .filter(|line| line.contains("Large session"))
                    .count();
                assert!(visible > 0 && 60 >= 4 * visible, "{text}");
                assert!(!text.contains("Large session 060"));
                phase = 1;
                return vec![key(KeyCode::PageDown); 4].into();
            }
            if phase == 1 && text.contains("❯ Large session") {
                let ordinal = selected
                    .as_deref()
                    .unwrap()
                    .strip_prefix("fixture-large-")
                    .unwrap()
                    .parse::<usize>()
                    .unwrap();
                if !text.contains(&format!("❯ Large session {:03}", ordinal + 1)) {
                    return FixtureStep::default();
                }
                assert!(
                    (32..=48).contains(&ordinal),
                    "four real pages must move beyond one viewport: {ordinal}: {text}"
                );
                phase = 2;
                return vec![key(KeyCode::End)].into();
            }
            if phase == 2 && text.contains("❯ Large session 060") {
                assert_eq!(selected.as_deref(), Some("fixture-large-059"));
                phase = 3;
                return vec![key(KeyCode::Home)].into();
            }
            if phase == 3 && text.contains("❯ Large session 001") {
                assert_eq!(selected.as_deref(), Some("fixture-large-000"));
                phase = 4;
                return vec![ctrl(KeyCode::Char('d'))].into();
            }
            FixtureStep::default()
        },
        120,
        36,
    )
    .await;
    assert_eq!(phase, 4);
    assert_eq!(outcome, None);
}

#[tokio::test]
async fn claude_new_success_fixture_returns_the_background_then_attach_handoff() {
    let directory = tempfile::tempdir().unwrap();
    let mut phase = 0;
    let outcome =
        exercise(
            "claude-new-success",
            directory.path(),
            vec![ctrl(KeyCode::Char('n'))],
            |canvas, stack, model| {
                let text = canvas.to_string();
                if phase == 0
                    && text.contains("New session")
                    && model.read().new_session.as_ref().is_some_and(|draft| {
                        matches!(draft.client_inventory, NewClientInventoryState::Ready(_))
                    })
                {
                    phase = 1;
                    return vec![key(KeyCode::Enter)].into();
                }
                if phase == 1 && dropdown_open(stack) {
                    phase = 2;
                    return vec![
                        TerminalEvent::BracketedPaste("Claude".into()),
                        key(KeyCode::Enter),
                    ]
                    .into();
                }
                if phase == 2
                    && !dropdown_open(stack)
                    && model.read().new_session.as_ref().is_some_and(|draft| {
                        draft.client == NewSessionClient::ClaudeNativeForeground
                    })
                {
                    phase = 3;
                    return vec![key(KeyCode::Tab), key(KeyCode::Tab), key(KeyCode::Enter)].into();
                }
                if phase == 3 && dropdown_open(stack) {
                    phase = 4;
                    return vec![
                        TerminalEvent::BracketedPaste("Background".into()),
                        key(KeyCode::Enter),
                    ]
                    .into();
                }
                if phase == 4
                    && !dropdown_open(stack)
                    && model.read().new_session.as_ref().is_some_and(|draft| {
                        draft.client == NewSessionClient::ClaudeNativeBackground
                    })
                {
                    assert!(text.contains("Background"));
                    phase = 5;
                    return vec![ctrl(KeyCode::Enter)].into();
                }
                FixtureStep::default()
            },
        )
        .await;
    assert_eq!(phase, 5);
    let Some(SessionsPickerOutcome::LaunchNativeClaude(captured, mode)) = outcome else {
        panic!("expected captured background Claude New handoff")
    };
    assert_eq!(mode, ClaudeTerminalMode::BackgroundThenAttach);
    assert_eq!(captured.source, PickerSourceContext::DefaultHosted);
    assert!(
        matches!(&captured.effect,SessionClientEffect::Native{action:SourceNativeClaudeAction::New{directory,arguments},..} if directory.as_str()=="/repo/project-a"&&arguments.is_empty())
    );
    let receipt: serde_json::Value = serde_json::from_slice(
        &std::fs::read(directory.path().join("picker-outcome.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        receipt,
        serde_json::json!({"kind":"new","request":{"client":"claudeCode","machine":"defaultHosted","directory":"/repo/project-a","runMode":"backgroundThenAttach"}})
    );
}
