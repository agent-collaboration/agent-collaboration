use super::*;
use crate::session_store_lane::{
    SessionStoreSettlement, SessionStoreTicket, SessionStoreWrite, tests::StoreFixture,
};
use acp_client_runtime::acp_transport::{TransportRefusal, TransportRun};
use acp_client_runtime::session_owner_task::{
    OwnerCompletionPort, OwnerTaskError, SessionOwnerPorts,
};
use collaboration_protocol::{
    ConversationBindingIdentity, ProviderOperationEffect, ProviderOperationKind,
    ProviderOperationStage, ProviderReconciliationState,
};
use collaboration_runtime::ProviderOperationAdmission;
use provider_session_machines::{
    session_lifecycle_machine::{LoadAttempt, RestoreVia, SessionLifecycle},
    session_owner::{
        AuthorizedWork, CloseOutcome, DispatchResult, EffectOutcome, OwnerEffect, OwnerEvent,
        OwnerInput, PromptAdmission, PublicationBatch, PublicationFact, PublicationResult,
        RestoreLoadReadback, RestorePreparation, RestoredMode, SessionOwnerCommand,
        SessionOwnerReply, SessionPublication, SessionStoreRequest, SessionStoreResult,
        SessionWireRequest, SessionWireResult, WireOutcome,
    },
    session_turn_machine::TurnId,
};
use session_event_model::InputId;

fn connection() -> ConnectionEpoch {
    ConnectionEpoch(Uuid::now_v7())
}

#[tokio::test]
async fn adoption_is_insert_if_absent_for_the_exact_provisional_owner() {
    let fixture = StoreFixture::new().await;
    let target = fixture.record_session("adopted").await;
    let registry = SessionOwnerRegistry::new(
        fixture.store,
        fixture.clock,
        NonZeroUsize::MIN,
        NonZeroUsize::MIN,
    );
    let attempt = OperationId::generate();
    let another = OperationId::generate();
    let first = registry
        .begin_create(attempt.clone(), connection())
        .expect("first provisional owner");
    let second = registry
        .begin_create(another.clone(), connection())
        .expect("second provisional owner");

    assert!(matches!(
        registry.adopt(&attempt, second.owner.address.incarnation, target.clone()),
        Err(SessionOwnerRegistryError::Replaced)
    ));
    let (adopted, _lane) = registry
        .adopt(&attempt, first.owner.address.incarnation, target.clone())
        .expect("adopt first owner");
    assert_eq!(adopted.owner.address, first.owner.address);
    assert!(matches!(
        registry.adopt(&another, second.owner.address.incarnation, target.clone()),
        Err(SessionOwnerRegistryError::AlreadyOwned)
    ));
    let registered = registry.get_or_insert(target, connection());
    assert!(
        registered.launch.is_none(),
        "an adopted session keeps its task and lane"
    );
    assert_eq!(registered.session.owner.address, adopted.owner.address);
}

struct ScenarioPorts {
    lane: SessionStoreLane,
    transport: Arc<AcpTransport<Option<DispatchAuthorization>>>,
    target: SessionRef,
    fail_input: Option<InputId>,
    close_probe: Option<tokio::sync::mpsc::UnboundedSender<CloseObservation>>,
}

#[derive(Debug, Eq, PartialEq)]
enum CloseObservation {
    Enqueue(Result<(), TransportRefusal>),
    Finished(CloseOutcome),
}

impl ScenarioPorts {
    fn offer_mark(
        &self,
        address: OwnerAddress,
        input: &OwnerInput,
    ) -> Result<SessionStoreTicket, OwnerTaskError> {
        self.lane
            .offer(
                address.incarnation,
                SessionStoreWrite::MarkDispatch(MarkedInput {
                    input: input.input.clone(),
                    operation: OperationId::try_from(input.operation.to_string())
                        .map_err(|_| OwnerTaskError::ExecutorStopped)?,
                }),
            )
            .map_err(|_| OwnerTaskError::ExecutorStopped)
    }

    fn start_mark(
        &self,
        address: OwnerAddress,
        input: OwnerInput,
        steer: bool,
        completions: &OwnerCompletionPort,
    ) -> Result<(), OwnerTaskError> {
        let completion = completions.reserve(address)?;
        let ticket = self.offer_mark(address, &input)?;
        if self.fail_input.as_ref() == Some(&input.input) {
            // The real lane accepted the mark. The owner fails before it
            // handles the result and cannot issue an authorization.
            return Err(OwnerTaskError::ExecutorStopped);
        }
        tokio::spawn(async move {
            let outcome = if ticket.settlement().await.is_ok() {
                EffectOutcome::Applied
            } else {
                EffectOutcome::Failed
            };
            completion.send(OwnerEvent::Store(if steer {
                SessionStoreResult::SteerDispatch { input, outcome }
            } else {
                SessionStoreResult::Dispatch { input, outcome }
            }));
        });
        Ok(())
    }
}

impl SessionOwnerPorts for ScenarioPorts {
    fn execute(
        &mut self,
        address: OwnerAddress,
        effect: OwnerEffect,
        completions: &OwnerCompletionPort,
    ) -> Result<Option<OwnerEvent>, OwnerTaskError> {
        let event = match effect {
            OwnerEffect::Store(SessionStoreRequest::ReadRestoreSetup(run)) => {
                let completion = completions.reserve(address)?;
                let ticket = self
                    .lane
                    .read_setup(address.incarnation)
                    .map_err(|_| OwnerTaskError::ExecutorStopped)?;
                tokio::spawn(async move {
                    let outcome = match ticket.settlement().await {
                        Ok(SessionStoreSettlement::Setup(Some(_))) => EffectOutcome::Applied,
                        _ => EffectOutcome::Failed,
                    };
                    completion.send(OwnerEvent::Store(SessionStoreResult::RestoreSetup {
                        run,
                        outcome,
                    }));
                });
                None
            }
            OwnerEffect::Store(SessionStoreRequest::MarkDispatch(input)) => {
                self.start_mark(address, input, false, completions)?;
                None
            }
            OwnerEffect::Store(SessionStoreRequest::MarkSteerDispatch(input)) => {
                self.start_mark(address, input, true, completions)?;
                None
            }
            OwnerEffect::Store(SessionStoreRequest::RecordTerminal { write, .. }) => {
                // The restore's operation custody is outside this fixture. The
                // failure settlement below uses the real SQLite terminal write.
                Some(OwnerEvent::Store(SessionStoreResult::Terminal {
                    write,
                    outcome: EffectOutcome::Applied,
                }))
            }
            OwnerEffect::Wire(SessionWireRequest::Restore { run, .. }) => {
                Some(OwnerEvent::Wire(SessionWireResult::Restore {
                    run,
                    outcome: WireOutcome::Confirmed(RestoreLoadReadback {
                        catalogue: Arc::default(),
                        router_auto_requested: false,
                        mode: RestoredMode::Observed,
                        fast_on: None,
                    }),
                }))
            }
            OwnerEffect::Wire(SessionWireRequest::Close(run)) => {
                let enqueue = self.transport.enqueue_close(
                    TransportRun {
                        incarnation: address.incarnation,
                        generation: run.generation,
                    },
                    run.attempt,
                    None,
                );
                let refused = enqueue.is_err();
                if let Some(probe) = &self.close_probe {
                    probe
                        .send(CloseObservation::Enqueue(enqueue))
                        .map_err(|_| OwnerTaskError::ExecutorStopped)?;
                }
                refused.then_some(OwnerEvent::Wire(SessionWireResult::CloseNotSent(run)))
            }
            OwnerEffect::Reply(SessionOwnerReply::CloseFinished { outcome, .. }) => {
                if let Some(probe) = &self.close_probe {
                    probe
                        .send(CloseObservation::Finished(outcome))
                        .map_err(|_| OwnerTaskError::ExecutorStopped)?;
                }
                None
            }
            OwnerEffect::PrepareRoute(run) => {
                self.transport.open_route(
                    TransportRun {
                        incarnation: address.incarnation,
                        generation: run.generation,
                    },
                    String::from(self.target.session_id.clone()),
                );
                Some(OwnerEvent::Prepare(RestorePreparation::RouteReady(run)))
            }
            OwnerEffect::PrepareHost { run, .. } => {
                Some(OwnerEvent::Prepare(RestorePreparation::HostReady {
                    run,
                    capabilities: Default::default(),
                    settings: Default::default(),
                    observed: BTreeMap::new(),
                }))
            }
            OwnerEffect::Publish(batch) => {
                let id = match batch {
                    PublicationBatch::Commit { id, facts } => {
                        for fact in facts {
                            if let PublicationFact::Live(SessionPublication::Activated {
                                run,
                                ..
                            }) = fact
                            {
                                self.transport.activate_route(TransportRun {
                                    incarnation: address.incarnation,
                                    generation: run.generation,
                                });
                            }
                        }
                        id
                    }
                    PublicationBatch::Rebase { id, .. } => id,
                };
                Some(OwnerEvent::Publication(PublicationResult::Applied(id)))
            }
            OwnerEffect::CallbackMembership(change) => {
                self.transport.apply_membership(address.incarnation, change);
                None
            }
            OwnerEffect::DrainCancel(authorization) => Some(OwnerEvent::Dispatch(
                DispatchResult::CancelDrainFinished(authorization),
            )),
            OwnerEffect::TakeCancel {
                authorization,
                expired: false,
            } => {
                self.transport
                    .offer(authorization.clone(), None, Some(authorization.clone()))
                    .map_err(|_| OwnerTaskError::ExecutorStopped)?;
                let result = match self
                    .transport
                    .take(&authorization)
                    .map_err(|_| OwnerTaskError::ExecutorStopped)?
                {
                    DispatchResult::Taken(authorization) => DispatchResult::CancelTaken {
                        authorization,
                        drain: provider_session_machines::session_owner::CancelDrainReport::Drained,
                    },
                    other => other,
                };
                Some(OwnerEvent::Dispatch(result))
            }
            OwnerEffect::Authorize {
                authorization,
                work,
            } => {
                let new_turn = match work {
                    AuthorizedWork::Prompt(input) => Some(input.turn),
                    _ => None,
                };
                self.transport
                    .offer(authorization.clone(), new_turn, Some(authorization.clone()))
                    .map_err(|_| OwnerTaskError::ExecutorStopped)?;
                Some(OwnerEvent::Dispatch(
                    self.transport
                        .take(&authorization)
                        .map_err(|_| OwnerTaskError::ExecutorStopped)?,
                ))
            }
            _ => None,
        };
        Ok(event)
    }
}

fn run_owner(
    launch: OwnerTaskLaunch,
    lane: SessionStoreLane,
    transport: Arc<AcpTransport<Option<DispatchAuthorization>>>,
    target: SessionRef,
    fail_input: Option<InputId>,
) -> tokio::task::JoinHandle<Result<(), OwnerTaskError>> {
    tokio::spawn(launch.task.run(
        ScenarioPorts {
            lane,
            transport,
            target,
            fail_input,
            close_probe: None,
        },
        launch.shutdown,
    ))
}

async fn settle_restore(owner: &RegisteredOwner) -> provider_session_machines::SessionGeneration {
    let mut snapshots = owner.handle.snapshots();
    owner
        .handle
        .try_submit(
            owner.address,
            SessionOwnerCommand::Restore {
                attempt: LoadAttempt(Uuid::now_v7()),
                via: RestoreVia::Load,
            },
        )
        .expect("accepted at mailbox")
        .await
        .expect("command handled")
        .expect("restore admitted");
    let snapshot = snapshots
        .wait_for(|snapshot| matches!(snapshot.lifecycle, SessionLifecycle::Live { .. }))
        .await
        .expect("restore used the ordered saved setup read");
    let SessionLifecycle::Live { generation } = snapshot.lifecycle else {
        panic!("restore must leave the replacement live");
    };
    generation
}

async fn admit_prompt(fixture: &StoreFixture, target: &SessionRef, operation: Uuid) -> OperationId {
    let operation_id = OperationId::try_from(operation.to_string()).expect("operation ID");
    let mut store = fixture.store.lock().await;
    store
        .admit(ProviderOperationAdmission {
            operation_id: operation_id.clone(),
            operation_kind: ProviderOperationKind::ConversationPrompt,
            binding: ConversationBindingIdentity::ExternalProvider {
                binding: fixture.binding.clone(),
            },
            admitted_at_ms: 0,
        })
        .await
        .expect("operation admitted");
    store
        .record_target(&operation_id, target, 0)
        .await
        .expect("operation target");
    operation_id
}

async fn owner_dies_after_accepting_mark(steer: bool) {
    let fixture = StoreFixture::new().await;
    let target = fixture.record_session("handover").await;
    let registry = SessionOwnerRegistry::new(
        fixture.store.clone(),
        fixture.clock.clone(),
        NonZeroUsize::new(4).expect("test mailbox capacity"),
        NonZeroUsize::new(4).expect("test completion capacity"),
    );
    let SessionRegistration { session, launch } =
        registry.get_or_insert(target.clone(), connection());
    let (owner_launch, lane_task) = launch.expect("first owner and lane");
    let stop_lane = CancellationToken::new();
    let lane_worker = tokio::spawn(lane_task.run(stop_lane.clone()));
    let (transport, mut writer) = AcpTransport::new(NonZeroUsize::new(4).expect("writer capacity"));
    let transport = Arc::new(transport);
    let operation = Uuid::now_v7();
    let operation_id = admit_prompt(&fixture, &target, operation).await;
    let input_id = InputId::generate();

    let old_owner = run_owner(
        owner_launch,
        session.lane.clone(),
        transport.clone(),
        target.clone(),
        Some(input_id.clone()),
    );
    let generation = settle_restore(&session.owner).await;
    let turn = TurnId(Uuid::now_v7());
    let owner = session.owner.address.incarnation;
    if steer {
        let foreground = Uuid::now_v7();
        admit_prompt(&fixture, &target, foreground).await;
        session
            .owner
            .handle
            .try_submit(
                session.owner.address,
                SessionOwnerCommand::Prompt {
                    input: OwnerInput {
                        generation,
                        operation: foreground,
                        input: InputId::generate(),
                        turn,
                    },
                    admission: PromptAdmission::Direct,
                },
            )
            .expect("foreground queued")
            .await
            .expect("foreground handled")
            .expect("foreground admitted");
        writer
            .recv()
            .await
            .expect("foreground took the real transport");
    }

    let input = OwnerInput {
        generation,
        operation,
        input: input_id.clone(),
        turn,
    };
    let command = if steer {
        SessionOwnerCommand::Steer {
            input,
            supported: true,
        }
    } else {
        SessionOwnerCommand::Prompt {
            input,
            admission: PromptAdmission::Direct,
        }
    };
    let held_store = fixture.store.lock().await;
    let admission = session
        .owner
        .handle
        .try_submit(session.owner.address, command)
        .expect("command queued");
    assert_eq!(
        old_owner.await.expect("owner stopped"),
        Err(OwnerTaskError::ExecutorStopped)
    );
    assert!(
        admission.await.is_err(),
        "the failed owner could not acknowledge its command"
    );
    let FailedOwnerSettlement {
        replacement,
        handover,
        slot_evidence,
        inputs,
    } = registry
        .replace_failed_session(&target, owner, connection(), [transport.as_ref()])
        .expect("single handover");
    assert!(
        slot_evidence
            .iter()
            .all(|(authorization, _)| authorization.input_id != input_id),
        "the failed input never gained a transport authorization"
    );
    assert!(
        inputs.contains(&FailedOwnerInput::KnownUnsent(MarkedInput {
            input: input_id,
            operation: operation_id.clone(),
        })),
        "T1 must settle the accepted mark without a slot as known unsent"
    );
    assert!(matches!(
        registry.replace_failed_session(&target, owner, connection(), [transport.as_ref()]),
        Err(SessionOwnerRegistryError::Replaced)
    ));

    let new_shutdown = replacement.shutdown.clone();
    let new_owner = replacement.owner.clone();
    let new_task = run_owner(
        replacement,
        session.lane.clone(),
        transport.clone(),
        target,
        None,
    );
    drop(held_store);
    assert!(matches!(
        handover.barrier.settlement().await,
        Ok(SessionStoreSettlement::HandedOver)
    ));
    let expected = (
        ProviderOperationStage::Terminal,
        ProviderOperationEffect::None,
        ProviderReconciliationState::Confirmed,
    );
    let row = fixture
        .store
        .lock()
        .await
        .inspect(&operation_id)
        .await
        .expect("current store")
        .expect("row remains");
    assert_eq!(
        (row.stage, row.effect, row.reconciliation_state),
        expected,
        "T1 cleanup must follow the late mark before handover"
    );
    let mut reopened = ProviderOperationStore::open(&fixture.path)
        .await
        .expect("reopen store");
    let row = reopened
        .inspect(&operation_id)
        .await
        .expect("reopened store")
        .expect("row remains");
    assert_eq!((row.stage, row.effect, row.reconciliation_state), expected);
    assert!(
        writer.try_recv().is_err(),
        "nothing escaped for the failed input"
    );
    settle_restore(&new_owner).await;
    new_shutdown.cancel();
    new_task
        .await
        .expect("replacement task stopped")
        .expect("clean stop");
    stop_lane.cancel();
    lane_worker.await.expect("lane stopped");
}

#[tokio::test(flavor = "current_thread")]
async fn failed_owner_settles_late_prompt_mark_across_handover_and_reopen() {
    owner_dies_after_accepting_mark(false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn failed_owner_settles_late_steer_mark_across_handover_and_reopen() {
    owner_dies_after_accepting_mark(true).await;
}

#[path = "session_owner_registry_close_test.rs"]
mod close_enqueue;

#[path = "session_owner_registry_publication_test.rs"]
mod publication;
