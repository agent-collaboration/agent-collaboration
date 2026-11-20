use super::*;
use crate::{
    ExternalProviderRuntime, external_provider_runtime::acp_scripted_fixture::launch_fixture_agent,
    session_store_lane::tests::StoreFixture,
};
use collaboration_contracts::SessionEventHub;
use collaboration_protocol::{ConversationBindingIdentity, OperationId, ProviderOperationKind};
use collaboration_runtime::ProviderOperationAdmission;
use provider_session_machines::{
    session_lifecycle_machine::{CloseAttempt, SessionLifecycle},
    session_owner::{CloseOutcome, RestoreDisposition, SessionOwnerReply},
    session_turn_machine::TurnId,
};
use session_event_model::{InputId, PromptContent, SessionState};

async fn operation(
    fixture: &StoreFixture,
    target: &SessionRef,
    kind: ProviderOperationKind,
) -> Uuid {
    let uuid = Uuid::now_v7();
    let operation = OperationId::try_from(uuid.to_string()).expect("operation ID");
    let mut store = fixture.store.lock().await;
    store
        .admit(ProviderOperationAdmission {
            operation_id: operation.clone(),
            operation_kind: kind,
            binding: ConversationBindingIdentity::ExternalProvider {
                binding: fixture.binding.clone(),
            },
            admitted_at_ms: 0,
        })
        .await
        .expect("reserve operation");
    store
        .record_target(&operation, target, 0)
        .await
        .expect("record target");
    uuid
}

#[tokio::test]
async fn owner_door_uses_the_real_acp_connection_for_restore_prompt_and_close() {
    let fixture = StoreFixture::new().await;
    let target = fixture.record_session("saved-session").await;
    let board: message_board::SessionRef = target.clone().try_into().expect("board identity");
    let hub = Arc::new(ProviderSessionEventHub::new(fixture.store.clone()));
    let wire_receipt = fixture.path.with_file_name("owner-wire.json");
    let runtime = ExternalProviderRuntime::initialize_with_mcp_http_and_hub(
        launch_fixture_agent(
            "permission-mode-agent.ts",
            vec![
                wire_receipt.display().to_string(),
                "cursor".into(),
                "no-directories".into(),
                "advertise-restore".into(),
                "plain-prompts".into(),
                "advertise-close".into(),
            ],
        ),
        acp_client_runtime::ProviderModelPicker::Standard,
        "fixture",
        "http://127.0.0.1:1/mcp",
        hub.clone(),
        board.endpoint.clone(),
        Arc::new(acp_provider_adapters::StandardAcpBehavior),
        fixture.clock.clone(),
    )
    .await
    .expect("actual ACP connection");
    let door = runtime.session_owner_door(fixture.store.clone(), hub.clone());
    let owner = door
        .open(
            target.clone(),
            crate::provider_work_roots::ProviderWorkRoots::default(),
        )
        .expect("open owner");
    let load = operation(&fixture, &target, ProviderOperationKind::ConversationLoad).await;
    let loaded = owner
        .restore(load, RestoreVia::Load)
        .await
        .expect("owner restore reply");
    assert!(loaded.saved);
    assert!(
        matches!(loaded.reply, SessionOwnerReply::RestoreFinished(result)
        if matches!(result.disposition, RestoreDisposition::Restored { .. }))
    );
    let mut snapshots = owner.snapshots();
    let SessionLifecycle::Live { generation } = snapshots.borrow().lifecycle else {
        panic!("successful restore commits a live run")
    };

    let operation_id =
        operation(&fixture, &target, ProviderOperationKind::ConversationPrompt).await;
    let input = OwnerInput {
        generation,
        operation: operation_id,
        input: InputId::generate(),
        turn: TurnId(Uuid::now_v7()),
    };
    let content = ProviderPromptContent::from_session_contents(
        vec![PromptContent::text("Owner route".to_owned()).expect("prompt content")],
        &acp_client_runtime::ProviderCapabilityReport::default(),
    )
    .expect("validated prompt");
    let sent = owner
        .submit_input(input.clone(), content, PromptAdmission::Direct)
        .await
        .expect("owner prompt reply");
    assert!(sent.saved);
    assert!(
        matches!(sent.reply, SessionOwnerReply::PromptCompleted { input: sent, .. } if sent == input)
    );

    let close = operation(&fixture, &target, ProviderOperationKind::ConversationClose).await;
    let closed = owner
        .settle(
            close,
            SessionOwnerCommand::Close {
                generation,
                attempt: CloseAttempt(close),
            },
        )
        .await
        .expect("owner close reply");
    assert!(matches!(
        closed.reply,
        SessionOwnerReply::CloseFinished {
            outcome: CloseOutcome::Confirmed,
            ..
        }
    ));
    snapshots
        .wait_for(|snapshot| {
            snapshot.projection.state == SessionState::Closed
                && snapshot.publication
                    == provider_session_machines::session_owner::PublicationStatus::Current
        })
        .await
        .expect("confirmed terminal projection");
    assert_eq!(
        hub.state(board).await.expect("hub projection"),
        SessionState::Closed
    );
    let recorded: serde_json::Value =
        serde_json::from_slice(&std::fs::read(wire_receipt).expect("wire receipt"))
            .expect("recorded wire");
    assert_eq!(recorded["restores"].as_array().map(Vec::len), Some(1));
    assert_eq!(recorded["prompts"].as_array().map(Vec::len), Some(1));
    assert_eq!(recorded["closes"].as_array().map(Vec::len), Some(1));

    door.shutdown().await;
    runtime.shutdown().await;
}
