use super::*;
use crate::session_owner_publication::SessionOwnerPublisher;
use collaboration_contracts::SessionEventHub;
use collaboration_runtime::ProviderSessionEventHub;
use provider_session_machines::session_lifecycle_machine::CloseAttempt;
use provider_session_machines::session_owner::{PromptCompletion, PublicationStatus};
use router_clock::ClockInstant;
use session_event_model::{SessionEvent, SessionState, StopReason};
use std::time::Duration;
use tokio_util::task::TaskTracker;

struct RealHubPorts {
    wire_and_store: ScenarioPorts,
    publisher: SessionOwnerPublisher,
}

impl SessionOwnerPorts for RealHubPorts {
    fn execute(
        &mut self,
        owner: OwnerAddress,
        effect: OwnerEffect,
        completions: &OwnerCompletionPort,
    ) -> Result<Option<OwnerEvent>, OwnerTaskError> {
        match effect {
            OwnerEffect::Publish(batch) => {
                // The connection observes activation before later wire takes.
                self.wire_and_store.execute(
                    owner,
                    OwnerEffect::Publish(batch.clone()),
                    completions,
                )?;
                self.publisher.publish(owner, batch, completions)
            }
            OwnerEffect::Wire(SessionWireRequest::Close(run)) => {
                let rejected = self.wire_and_store.execute(
                    owner,
                    OwnerEffect::Wire(SessionWireRequest::Close(run)),
                    completions,
                )?;
                Ok(rejected.or(Some(OwnerEvent::Wire(SessionWireResult::Close {
                    run,
                    outcome: WireOutcome::Confirmed(()),
                }))))
            }
            effect => self.wire_and_store.execute(owner, effect, completions),
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_refused_closed_batch_rebases_the_real_hub_without_another_owner_command() {
    let fixture = StoreFixture::new().await;
    let target = fixture.record_session("publication").await;
    let board_session: message_board::SessionRef =
        collaboration_protocol::ProviderIdentity::session_to_board(&target).expect("board session");
    let registry = SessionOwnerRegistry::new(
        fixture.store.clone(),
        fixture.clock.clone(),
        NonZeroUsize::new(8).expect("mailbox"),
        NonZeroUsize::new(16).expect("completions"),
    );
    let SessionRegistration { session, launch } =
        registry.get_or_insert(target.clone(), connection());
    let (launch, lane_task) = launch.expect("owner and lane");
    let shutdown = CancellationToken::new();
    let lane_worker = tokio::spawn(lane_task.run(shutdown.clone()));

    let hub = Arc::new(ProviderSessionEventHub::new(fixture.store.clone()));
    let (inbox, inbox_task) = hub.owner_inbox(board_session.clone(), NonZeroUsize::MIN);
    let address = session.owner.address;
    inbox
        .install_incarnation(None, address.incarnation.0)
        .expect("hub incarnation");
    let mut available = inbox.capacity_returned();
    let hub_worker = tokio::spawn(inbox_task.expect("new inbox").run(shutdown.clone()));
    let (transport, mut writer) = AcpTransport::new(NonZeroUsize::new(4).expect("capacity"));
    let publications = TaskTracker::new();
    let ports = RealHubPorts {
        wire_and_store: ScenarioPorts {
            lane: session.lane,
            transport: Arc::new(transport),
            target: target.clone(),
            fail_input: None,
            close_probe: None,
        },
        publisher: SessionOwnerPublisher::new(inbox, fixture.clock.clone(), publications.clone()),
    };
    let task = tokio::spawn(launch.task.run(ports, shutdown.clone()));
    let mut snapshots = session.owner.handle.snapshots();

    let held_store = fixture.store.lock().await;
    let load = SessionOwnerCommand::Restore {
        attempt: LoadAttempt(Uuid::now_v7()),
        via: RestoreVia::Load,
    };
    session
        .owner
        .handle
        .try_submit(address, load)
        .expect("queued")
        .await
        .expect("handled")
        .expect("admitted");
    available.changed().await.expect("first rebase applied");
    drop(held_store);
    available.changed().await.expect("activation batch applied");
    assert_eq!(
        hub.state(board_session.clone()).await.expect("idle"),
        SessionState::Idle
    );
    let before = hub
        .attach(board_session.clone())
        .await
        .expect("initial attachment");
    let generation = if let SessionLifecycle::Live { generation } = snapshots.borrow().lifecycle {
        generation
    } else {
        panic!("before holding publication the owner must be live");
    };

    let operation = Uuid::now_v7();
    admit_prompt(&fixture, &target, operation).await;
    let held_history = hub.hold_history_for_test(board_session.clone()).await;
    let turn = TurnId(Uuid::now_v7());
    session
        .owner
        .handle
        .try_submit(
            address,
            SessionOwnerCommand::Prompt {
                input: OwnerInput {
                    generation,
                    operation,
                    input: InputId::generate(),
                    turn,
                },
                admission: PromptAdmission::Direct,
            },
        )
        .expect("prompt queued")
        .await
        .expect("prompt handled")
        .expect("prompt admitted");
    let prompt = writer
        .recv()
        .await
        .expect("prompt taken")
        .frame
        .expect("prompt authorization");

    session
        .owner
        .handle
        .try_submit(
            address,
            SessionOwnerCommand::Close {
                generation,
                attempt: CloseAttempt(Uuid::now_v7()),
            },
        )
        .expect("close queued")
        .await
        .expect("close handled")
        .expect("close admitted");
    writer
        .recv()
        .await
        .expect("cancel taken")
        .frame
        .expect("cancel authorization");
    session
        .owner
        .handle
        .completion_port()
        .reserve(address)
        .expect("prompt completion permit")
        .send(OwnerEvent::Dispatch(DispatchResult::PromptFinished {
            authorization: prompt,
            outcome: WireOutcome::Confirmed(PromptCompletion {
                stop_reason: StopReason::EndTurn,
                local_cause: None,
                response: String::new(),
            }),
        }));
    assert!(writer.recv().await.expect("close taken").frame.is_none());
    snapshots
        .wait_for(|snapshot| {
            snapshot.projection.state == SessionState::Closed
                && matches!(snapshot.publication, PublicationStatus::Degraded { .. })
        })
        .await
        .expect("the refused terminal batch shows Degraded while the hub is unavailable");
    drop(held_history);

    let local_budget = Duration::from_secs(10);
    fixture
        .clock
        .not_due_check_now(ClockInstant::ZERO.after(local_budget))
        .await;
    fixture
        .clock
        .advance(local_budget)
        .expect("rebase deadline");
    snapshots
        .wait_for(|snapshot| snapshot.publication == PublicationStatus::Current)
        .await
        .expect("the latest applied rebase made the owner Current");
    let updated = hub
        .attach(board_session.clone())
        .await
        .expect("fresh attachment");
    assert!(updated.epoch > before.epoch);
    assert_eq!(
        hub.state(board_session)
            .await
            .expect("confirmed projection"),
        SessionState::Closed
    );
    assert_eq!(
        updated.snapshot.last().map(|event| &event.event),
        Some(&SessionEvent::StateChanged {
            state: SessionState::Closed,
        })
    );
    assert!(
        !updated
            .snapshot
            .iter()
            .any(|event| matches!(event.event, SessionEvent::TurnStarted { .. })),
        "a gap requires resync to the baseline without stale turn history"
    );

    shutdown.cancel();
    task.await.expect("owner task joined").expect("clean stop");
    hub_worker.await.expect("hub task joined");
    lane_worker.await.expect("lane task joined");
    publications.close();
    publications.wait().await;
}
