//! A settlement whose durable write fails (design note §12.5, S5): the
//! operation's one completion owner keeps the outcome it observed as a live
//! overlay, wait answers that outcome and the durable stage in one line,
//! the row stays where it was, nothing is sent again, and later settlements
//! never evict the overlay. The write fails through the store's StoreFault.

use super::deferred_uncertainty_admission_tests::recorded_frames;
use super::*;
use collaboration_runtime::ProviderSessionRecord;

const UNSAVED_APPLIED: &str =
    "applied; the durable record still says may-have-dispatched (write failed)";

/// Oracle: §12.5, written by hand: show and wait never claim a blanket
/// "applied"; the row stays MayHaveDispatched; no retry and no second send;
/// the overlay outlives MAX_RETAINED_SETTLEMENTS later settlements.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_settlement_that_fails_to_save_answers_its_outcome_and_durable_stage() {
    // Arrange
    let root = tempfile::tempdir().expect("fixture root");
    let receipt = root.path().join("wire.json");
    let target = SessionRef {
        endpoint: endpoint(),
        session_id: SessionId::try_from("reactive-session-1".to_owned()).expect("session"),
    };
    let runtime = ExternalProviderRuntime::initialize(
        crate::external_provider_runtime::acp_scripted_fixture::launch_fixture_agent(
            "cursor-settings-agent.ts",
            vec!["reactive".to_owned(), receipt.display().to_string()],
        ),
        Arc::new(acp_provider_adapters::StandardAcpBehavior),
    )
    .await
    .expect("fixture provider");
    runtime
        .create_session(root.path().to_owned())
        .await
        .expect("session/new");
    let store = Arc::new(Mutex::new(
        ProviderOperationStore::open(&root.path().join("operations.sqlite"))
            .await
            .expect("store"),
    ));
    store
        .lock()
        .await
        .record_session(&ProviderSessionRecord {
            target: target.clone(),
            working_directory: ProviderWorkingDirectory::try_from(
                root.path().display().to_string(),
            )
            .expect("working directory"),
            requested_policy: ProviderRequestedPolicy {
                access: RouterAccess::WriteRestricted,
            },
            created_by: target.clone().into(),
            approver: target.clone().into(),
            updated_at_ms: 1,
        })
        .await
        .expect("session record");
    let supervisor = Arc::new(
        ExternalProviderSupervisor::new(
            vec![ExternalProviderBinding {
                identity: binding(),
                runtime,
            }],
            Arc::clone(&store),
        )
        .expect("supervisor"),
    );
    let operation_id = OperationId::generate();
    store
        .lock()
        .await
        .fail_next_terminal_write_of(&operation_id);
    let wait = |operation_id: &OperationId| {
        supervisor.wait(ConversationOperationWaitRequest {
            operation_id: operation_id.clone(),
            timeout_seconds: PositiveSeconds::try_from(10).expect("bound"),
        })
    };

    // Act: the agent answers; the terminal write fails.
    supervisor
        .prompt(ConversationPromptRequest {
            delivery: collaboration_protocol::MessageDelivery::Auto,
            operation_id: operation_id.clone(),
            input_id: None,
            target: target.clone(),
            generation: None,
            requested_by: target.clone().into(),
            approver: target.clone().into(),
            prompt: collaboration_protocol::MessageContent::HumanUser {
                text: MessageText::try_from("unsaved".to_owned()).expect("prompt"),
            },
        })
        .await
        .expect("sent");
    let waited = wait(&operation_id).await;
    let shown = supervisor
        .show(ConversationOperationShowRequest {
            operation_id: operation_id.clone(),
        })
        .await
        .expect("shown");
    let row = store
        .lock()
        .await
        .inspect(&operation_id)
        .await
        .expect("row read")
        .expect("row");
    // Later settlements fill the settled window past its bound.
    for _ in 0..=MAX_RETAINED_SETTLEMENTS {
        let later = OperationId::generate();
        let live = Arc::new(LiveOperation::pending());
        supervisor
            .inner
            .live_operations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .operations
            .insert(later.clone(), Arc::clone(&live));
        supervisor
            .inner
            .publish_result(
                later.clone(),
                &live,
                Err(unavailable_failure(later, None)),
                provider_operation_finish::Retention::Settled,
            )
            .await;
    }
    let waited_after_later_settlements = wait(&operation_id).await;
    let prompts = recorded_frames(&receipt, "session/prompt", None);
    let stopped = supervisor.shutdown().await;

    // Assert
    let unsaved = |waited: &Result<_, ConversationOperationFailure>| match waited {
        Err(failure) => Some((
            failure.kind,
            failure.effect,
            String::from(failure.message.clone()),
        )),
        Ok(_) => None,
    };
    let expected = Some((
        ConversationOperationFailureKind::Unavailable,
        ProviderOperationEffect::Applied,
        UNSAVED_APPLIED.to_owned(),
    ));
    assert_eq!(unsaved(&waited), expected, "{waited:?}");
    assert_eq!(
        row.stage,
        ProviderOperationStage::MayHaveDispatched,
        "no retry wrote the row"
    );
    assert_eq!(
        shown.stage,
        ProviderOperationStage::MayHaveDispatched,
        "show reports the durable stage, never a blanket applied"
    );
    assert_eq!(prompts, 1, "nothing was sent again");
    assert_eq!(
        unsaved(&waited_after_later_settlements),
        expected,
        "later settlements don't evict the overlay: {waited_after_later_settlements:?}"
    );
    assert_eq!(stopped, Ok(()));
}
