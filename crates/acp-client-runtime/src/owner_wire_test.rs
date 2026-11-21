use super::*;
use crate::session_owner_task::{
    OwnerCompletionPort, OwnerTaskError, SessionOwnerPorts, SessionOwnerTask,
};
use provider_session_machines::{
    session_lifecycle_machine::SessionLifecycle,
    session_owner::{
        AuthorizedWork, ConnectionEpoch, CreateProgress, EffectOutcome, OwnerAddress, OwnerEffect,
        OwnerInput, PromptAdmission, PublicationBatch, PublicationResult, SessionOwnerCommand,
        SessionSnapshot, SessionStoreRequest, SessionStoreResult, SessionWireRequest,
    },
    session_turn_machine::SessionTurn,
};
use session_event_model::{InputId, SessionState, TurnLostReason, TurnOutcome};

struct PromptPorts {
    taken: mpsc::UnboundedSender<DispatchAuthorization>,
}

impl SessionOwnerPorts for PromptPorts {
    fn execute(
        &mut self,
        _: OwnerAddress,
        effect: OwnerEffect,
        _: &OwnerCompletionPort,
    ) -> Result<Option<OwnerEvent>, OwnerTaskError> {
        Ok(match effect {
            OwnerEffect::Wire(SessionWireRequest::Create { operation }) => {
                Some(OwnerEvent::Create(CreateProgress::ProviderId {
                    operation,
                    provider_id: "fixture".into(),
                }))
            }
            OwnerEffect::AdoptProviderId { operation, .. } => {
                Some(OwnerEvent::Create(CreateProgress::Adopted {
                    operation,
                    facts: Box::default(),
                }))
            }
            OwnerEffect::Publish(
                PublicationBatch::Commit { id, .. } | PublicationBatch::Rebase { id, .. },
            ) => Some(OwnerEvent::Publication(PublicationResult::Applied(id))),
            OwnerEffect::Store(
                SessionStoreRequest::MarkDispatch(input)
                | SessionStoreRequest::MarkQueuedDispatch(input),
            ) => Some(OwnerEvent::Store(SessionStoreResult::Dispatch {
                input,
                outcome: EffectOutcome::Applied,
            })),
            OwnerEffect::Authorize {
                authorization,
                work: AuthorizedWork::Prompt(_),
            } => {
                self.taken
                    .send(authorization.clone())
                    .map_err(|_| OwnerTaskError::ExecutorStopped)?;
                Some(OwnerEvent::Dispatch(DispatchResult::Taken(authorization)))
            }
            OwnerEffect::Store(SessionStoreRequest::RecordTerminal { write, .. }) => {
                Some(OwnerEvent::Store(SessionStoreResult::Terminal {
                    write,
                    outcome: EffectOutcome::Applied,
                }))
            }
            _ => None,
        })
    }
}

async fn settle_prompt_error(provider_shutdown: CancellationToken) -> SessionSnapshot {
    let address = OwnerAddress {
        incarnation: OwnerIncarnation(Uuid::now_v7()),
        connection_epoch: ConnectionEpoch(Uuid::now_v7()),
    };
    let capacity = NonZeroUsize::new(8).expect("mailbox capacity");
    let clock = Arc::new(router_clock::ManualClock::new(chrono::DateTime::UNIX_EPOCH));
    let (task, handle) = SessionOwnerTask::new(address, clock, capacity, capacity);
    let (taken, mut takes) = mpsc::unbounded_channel();
    let owner_shutdown = CancellationToken::new();
    let worker = tokio::spawn(task.run(PromptPorts { taken }, owner_shutdown.clone()));

    handle
        .try_submit(
            address,
            SessionOwnerCommand::Create {
                operation: Uuid::now_v7(),
            },
        )
        .expect("submit create")
        .await
        .expect("create acknowledgement")
        .expect("create admitted");
    let mut snapshots = handle.snapshots();
    let created = snapshots
        .wait_for(|snapshot| matches!(snapshot.lifecycle, SessionLifecycle::Live { .. }))
        .await
        .expect("published created session")
        .lifecycle;
    let SessionLifecycle::Live { generation } = created else {
        panic!("create must reach the live generation");
    };

    let turn = TurnId(Uuid::now_v7());
    handle
        .try_submit(
            address,
            SessionOwnerCommand::Prompt {
                input: OwnerInput {
                    generation,
                    operation: Uuid::now_v7(),
                    input: InputId::generate(),
                    turn,
                },
                admission: PromptAdmission::Direct,
            },
        )
        .expect("submit prompt")
        .await
        .expect("prompt acknowledgement")
        .expect("prompt admitted");
    let authorization = takes.recv().await.expect("writer take");
    snapshots
        .wait_for(|snapshot| snapshot.turn == (SessionTurn::Running { turn }))
        .await
        .expect("published running turn");

    let correlation = ResponseKind::Prompt {
        authorization: authorization.clone(),
        turn,
        compact: false,
        reader: None,
    };
    // Both waits are ready in the shutdown case. The reply wins the worker's
    // select and enters the owner before any independent retirement event.
    let (reply, connection_lost) = correlation
        .read_reply(
            std::future::ready(Err(Error::internal_error())),
            &provider_shutdown,
            |_| false,
        )
        .await;
    assert!(
        matches!(reply, Some(Err(_))),
        "consume the ready reply first"
    );
    PromptTerminal {
        authorization,
        turn,
        compact: false,
        reply,
        completion: handle
            .completion_port()
            .reserve(address)
            .expect("reserve prompt completion"),
    }
    .route(None, connection_lost);

    let settled = snapshots
        .wait_for(|snapshot| {
            snapshot
                .last_live_turn
                .as_ref()
                .is_some_and(|observed| observed.turn == turn)
        })
        .await
        .expect("owner settles the prompt")
        .clone();
    owner_shutdown.cancel();
    worker
        .await
        .expect("join owner task")
        .expect("stop owner task");
    settled
}

#[tokio::test]
async fn an_error_reply_read_after_shutdown_retires_the_turn_and_unloads_the_session() {
    let shutdown = CancellationToken::new();
    shutdown.cancel();
    let snapshot = settle_prompt_error(shutdown).await;

    assert_eq!(
        snapshot.last_live_turn.expect("terminal turn").outcome,
        TurnOutcome::Lost {
            reason: TurnLostReason::ProviderRetired
        },
    );
    assert!(matches!(
        snapshot.lifecycle,
        SessionLifecycle::NotLoaded { .. }
    ));
    assert_eq!(snapshot.projection.state, SessionState::Unloaded);
}

#[tokio::test]
async fn a_live_connection_keeps_a_coded_prompt_failure() {
    let snapshot = settle_prompt_error(CancellationToken::new()).await;

    assert_eq!(
        snapshot.last_live_turn.expect("terminal turn").outcome,
        TurnOutcome::Lost {
            reason: TurnLostReason::ProviderTurnFailed
        },
    );
    assert!(matches!(snapshot.lifecycle, SessionLifecycle::Live { .. }));
}
