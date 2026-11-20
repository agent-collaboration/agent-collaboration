use super::*;
use provider_session_machines::session_owner::{
    ConnectionEpoch, CreateOutcome, CreateProgress, OwnerIncarnation, PublicationBatch,
    PublicationResult, PublicationStatus, RemoteEffect, SessionWireRequest,
};
use router_clock::ManualClock;
use std::time::Duration;
use uuid::Uuid;

struct RecordingPorts {
    observed: mpsc::UnboundedSender<OwnerEffect>,
    refuse_first_batch: bool,
}

impl SessionOwnerPorts for RecordingPorts {
    fn execute(
        &mut self,
        _: OwnerAddress,
        effect: OwnerEffect,
        _: &OwnerCompletionPort,
    ) -> Result<Option<OwnerEvent>, OwnerTaskError> {
        let immediate = match &effect {
            OwnerEffect::Publish(
                PublicationBatch::Commit { id, .. } | PublicationBatch::Rebase { id, .. },
            ) => {
                let refused = std::mem::take(&mut self.refuse_first_batch);
                Some(OwnerEvent::Publication(if refused {
                    PublicationResult::Refused(*id)
                } else {
                    PublicationResult::Applied(*id)
                }))
            }
            _ => None,
        };
        self.observed
            .send(effect)
            .map_err(|_| OwnerTaskError::ExecutorStopped)?;
        Ok(immediate)
    }
}

struct Fixture {
    clock: Arc<ManualClock>,
    address: OwnerAddress,
    handle: SessionOwnerHandle,
    effects: mpsc::UnboundedReceiver<OwnerEffect>,
    stop: CancellationToken,
    worker: tokio::task::JoinHandle<Result<(), OwnerTaskError>>,
}

impl Fixture {
    fn start(refuse_first_batch: bool) -> Self {
        let clock = Arc::new(ManualClock::new(chrono::DateTime::UNIX_EPOCH));
        let address = OwnerAddress {
            incarnation: OwnerIncarnation(Uuid::now_v7()),
            connection_epoch: ConnectionEpoch(Uuid::now_v7()),
        };
        let (task, handle) =
            SessionOwnerTask::new(address, clock.clone(), NonZeroUsize::MIN, NonZeroUsize::MIN);
        let (observed, effects) = mpsc::unbounded_channel();
        let stop = CancellationToken::new();
        let worker = tokio::spawn(task.run(
            RecordingPorts {
                observed,
                refuse_first_batch,
            },
            stop.clone(),
        ));
        Self {
            clock,
            address,
            handle,
            effects,
            stop,
            worker,
        }
    }

    fn submit_create(
        &self,
        operation: Uuid,
    ) -> oneshot::Receiver<Result<(), SessionOwnerRejection>> {
        self.handle
            .try_submit(self.address, SessionOwnerCommand::Create { operation })
            .expect("the mailbox accepted the create")
    }

    async fn next_effect(&mut self) -> OwnerEffect {
        self.effects.recv().await.expect("owner emitted an effect")
    }

    async fn begin_create(&mut self) -> Uuid {
        let operation = Uuid::now_v7();
        self.submit_create(operation)
            .await
            .expect("received admission")
            .expect("accepted");
        assert!(matches!(
            self.next_effect().await,
            OwnerEffect::Publish(PublicationBatch::Rebase { .. })
        ));
        assert_eq!(
            self.next_effect().await,
            OwnerEffect::Wire(SessionWireRequest::Create { operation })
        );
        operation
    }

    async fn finish(self) {
        self.stop.cancel();
        assert_eq!(self.worker.await.expect("owner task joined"), Ok(()));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_due_deadline_settles_before_the_full_business_mailbox_is_read() {
    let mut fixture = Fixture::start(false);
    let first = fixture.begin_create().await;
    let budget = Duration::from_secs(30);
    fixture
        .clock
        .not_due_check_now(ClockInstant::ZERO.after(budget))
        .await;

    let second = Uuid::now_v7();
    let admitted_after_deadline = fixture.submit_create(second);
    assert!(matches!(
        fixture.handle.try_submit(
            fixture.address,
            SessionOwnerCommand::Create {
                operation: Uuid::now_v7()
            }
        ),
        Err(OwnerMailboxUnavailable::Busy)
    ));
    fixture
        .clock
        .advance(budget)
        .expect("advance to create deadline");

    assert_eq!(
        fixture.next_effect().await,
        OwnerEffect::CreateSettled {
            operation: first,
            outcome: CreateOutcome::Failed(RemoteEffect::Unknown),
        }
    );
    admitted_after_deadline
        .await
        .expect("new create received")
        .expect("deadline freed admission");
    assert_eq!(
        fixture.next_effect().await,
        OwnerEffect::Wire(SessionWireRequest::Create { operation: second })
    );
    fixture.finish().await;
}

#[tokio::test(flavor = "current_thread")]
async fn reserved_completion_is_delivered_with_a_full_business_mailbox() {
    let mut fixture = Fixture::start(false);
    let first = fixture.begin_create().await;
    let completions = fixture.handle.completion_port();
    let completed = completions
        .reserve(fixture.address)
        .expect("reserve before accepting effect");
    assert!(matches!(
        completions.reserve(fixture.address),
        Err(OwnerTaskError::CompletionCapacity)
    ));

    let second = Uuid::now_v7();
    let admitted_after_completion = fixture.submit_create(second);
    completed.send(OwnerEvent::Create(CreateProgress::Failed {
        operation: first,
        remote: RemoteEffect::None,
    }));

    assert_eq!(
        fixture.next_effect().await,
        OwnerEffect::CreateSettled {
            operation: first,
            outcome: CreateOutcome::Failed(RemoteEffect::None),
        }
    );
    admitted_after_completion
        .await
        .expect("new create received")
        .expect("completion freed admission");
    assert_eq!(
        fixture.next_effect().await,
        OwnerEffect::Wire(SessionWireRequest::Create { operation: second })
    );
    fixture.finish().await;
}

#[tokio::test(flavor = "current_thread")]
async fn publication_rebases_on_the_injected_clock_without_another_command() {
    let mut fixture = Fixture::start(true);
    let mut snapshots = fixture.handle.snapshots();
    fixture.begin_create().await;
    let refused = snapshots
        .wait_for(|snapshot| matches!(snapshot.publication, PublicationStatus::Degraded { .. }))
        .await
        .expect("refusal is visible")
        .publication;
    let PublicationStatus::Degraded { since, .. } = refused else {
        panic!("publication must show the refused batch");
    };
    let budget = Duration::from_secs(10);
    fixture
        .clock
        .not_due_check_now(ClockInstant::ZERO.after(budget))
        .await;

    fixture.clock.advance(budget).expect("advance to rebase");
    let rebased = fixture.next_effect().await;
    assert!(
        matches!(rebased, OwnerEffect::Publish(PublicationBatch::Rebase { id, .. })
        if id.incarnation == since.incarnation && id.sequence > since.sequence)
    );
    snapshots
        .wait_for(|snapshot| snapshot.publication == PublicationStatus::Current)
        .await
        .expect("confirmed rebase restores Current");
    fixture.finish().await;
}
