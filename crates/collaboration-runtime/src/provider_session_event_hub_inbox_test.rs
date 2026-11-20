use super::*;
use crate::ProviderOperationStore;
use collaboration_contracts::SessionEventHub;

async fn hub() -> (tempfile::TempDir, Arc<ProviderSessionEventHub>) {
    let root = tempfile::tempdir().expect("temporary store");
    let store = ProviderOperationStore::open(&root.path().join("operations.sqlite"))
        .await
        .expect("open real store");
    (
        root,
        Arc::new(ProviderSessionEventHub::new(Arc::new(
            tokio::sync::Mutex::new(store),
        ))),
    )
}

fn session() -> SessionRef {
    serde_json::from_value(serde_json::json!({
        "endpoint": {"serviceId": Uuid::now_v7(), "endpointId": "claude-local"},
        "sessionId": Uuid::now_v7(),
    }))
    .expect("session")
}

fn stamp(incarnation: Uuid, sequence: u64) -> HubPublicationStamp {
    HubPublicationStamp {
        incarnation,
        sequence,
    }
}

fn rebase(incarnation: Uuid, sequence: u64, state: SessionState) -> HubPublication {
    HubPublication::Rebase {
        stamp: stamp(incarnation, sequence),
        baseline: Box::new(HubPublicationBaseline {
            state,
            turn_running: false,
            capabilities: None,
            settings: None,
            skills: SkillView::Unknown,
            history: HubHistoryAvailability::Unavailable,
        }),
    }
}

fn commit(incarnation: Uuid, sequence: u64, states: &[SessionState]) -> HubPublication {
    HubPublication::Commit {
        stamp: stamp(incarnation, sequence),
        facts: states
            .iter()
            .map(|state| {
                HubPublicationFact::Event(SessionEvent::StateChanged {
                    state: state.clone(),
                })
            })
            .collect(),
        observed_at_seconds: 0,
    }
}

fn start(
    hub: &Arc<ProviderSessionEventHub>,
    session: &SessionRef,
    capacity: NonZeroUsize,
    incarnation: Uuid,
) -> (
    HubSessionInbox,
    CancellationToken,
    tokio::task::JoinHandle<()>,
) {
    let (inbox, task) = hub.owner_inbox(session.clone(), capacity);
    inbox
        .install_incarnation(None, incarnation)
        .expect("install initial owner");
    assert!(
        hub.owner_inbox(session.clone(), capacity).1.is_none(),
        "a session has one inbox worker"
    );
    let stop = CancellationToken::new();
    (
        inbox,
        stop.clone(),
        tokio::spawn(task.expect("first task").run(stop)),
    )
}

async fn applied(inbox: &HubSessionInbox, batch: HubPublication) -> HubPublicationStamp {
    inbox
        .try_offer(batch)
        .expect("enqueued")
        .await
        .expect("worker settled")
        .expect("applied")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offer_during_apply_preserves_batch_order_and_history() {
    let (_root, hub) = hub().await;
    let session = session();
    let incarnation = Uuid::now_v7();
    let (inbox, stop, worker) = start(&hub, &session, NonZeroUsize::MIN, incarnation);
    applied(&inbox, rebase(incarnation, 1, SessionState::Idle)).await;
    let initial = hub
        .attach(session.clone())
        .await
        .expect("initial attachment");

    let (entered, applying) = oneshot::channel();
    let (release, hold) = std::sync::mpsc::channel();
    *inbox.inner.apply_gate.lock().expect("apply probe") = Some((entered, hold));
    let first = inbox
        .try_offer(commit(incarnation, 2, &[SessionState::Running]))
        .expect("first offer");
    applying.await.expect("worker holds the apply lock");
    let (entered, contended) = oneshot::channel();
    *inbox.inner.contended_offer.lock().expect("offer probe") = Some(entered);
    let contender = inbox.clone();
    let second = tokio::task::spawn_blocking(move || {
        contender.try_offer(commit(incarnation, 3, &[SessionState::Idle]))
    });
    let witnessed = contended.await;
    release.send(()).expect("release the held apply");
    witnessed.expect("second offer reached the held lock");

    assert_eq!(
        first.await.expect("first receipt"),
        Ok(stamp(incarnation, 2))
    );
    assert_eq!(
        second
            .await
            .expect("offer task")
            .expect("contention must not refuse the offer")
            .await
            .expect("second receipt"),
        Ok(stamp(incarnation, 3))
    );
    let history = hub
        .attach(session)
        .await
        .expect("history after both commits");
    assert_eq!(
        history.epoch, initial.epoch,
        "ordinary contention cannot force a rebase"
    );
    assert_eq!(
        history
            .snapshot
            .iter()
            .map(|event| &event.event)
            .collect::<Vec<_>>(),
        vec![
            &SessionEvent::StateChanged {
                state: SessionState::Idle
            },
            &SessionEvent::StateChanged {
                state: SessionState::Running
            },
            &SessionEvent::StateChanged {
                state: SessionState::Idle
            },
        ]
    );
    stop.cancel();
    worker.await.expect("worker stopped");
}

#[tokio::test(flavor = "current_thread")]
async fn a_previous_turn_terminal_is_retained_without_ending_the_active_turn() {
    let (_root, hub) = hub().await;
    let session = session();
    let incarnation = Uuid::now_v7();
    let (inbox, stop, task) = start(&hub, &session, NonZeroUsize::MIN, incarnation);
    applied(&inbox, rebase(incarnation, 1, SessionState::Idle)).await;
    let previous = Uuid::now_v7().to_string();
    let active = Uuid::now_v7().to_string();
    let started = |turn_id: String| {
        HubPublicationFact::Event(SessionEvent::TurnStarted {
            turn_id,
            input_id: session_event_model::InputId::generate(),
        })
    };
    let ended = session_event_model::TurnOutcome::Ended {
        stop_reason: session_event_model::StopReason::EndTurn,
        local_cause: None,
    };
    applied(
        &inbox,
        HubPublication::Commit {
            stamp: stamp(incarnation, 2),
            facts: vec![
                started(previous.clone()),
                started(active.clone()),
                HubPublicationFact::PreviousTurnEnded {
                    turn_id: previous.clone(),
                    outcome: ended.clone(),
                },
            ],
            observed_at_seconds: 0,
        },
    )
    .await;
    let state = hub.state(session.clone()).await.expect("current state");
    let retained = hub.attach(session.clone()).await.expect("terminal history");
    assert!(retained.snapshot.iter().any(|event| matches!(&event.event, SessionEvent::TurnEnded { turn_id, outcome } if turn_id == &previous && outcome == &ended)));
    assert_eq!(
        state,
        SessionState::Running,
        "the previous prompt terminal must leave the new turn running"
    );
    applied(
        &inbox,
        HubPublication::Commit {
            stamp: stamp(incarnation, 3),
            facts: vec![HubPublicationFact::Event(SessionEvent::TurnEnded {
                turn_id: active,
                outcome: ended,
            })],
            observed_at_seconds: 0,
        },
    )
    .await;
    assert_eq!(
        hub.state(session)
            .await
            .expect("current terminal now applies"),
        SessionState::Idle
    );
    stop.cancel();
    task.await.expect("worker stopped");
}

#[tokio::test(flavor = "current_thread")]
async fn a_refused_terminal_batch_leaves_a_gap_until_rebase_and_other_sessions_keep_applying() {
    let (_root, hub) = hub().await;
    let (session_a, session_b) = (session(), session());
    let (incarnation_a, incarnation_b) = (Uuid::now_v7(), Uuid::now_v7());
    let (a, stop_a, task_a) = start(&hub, &session_a, NonZeroUsize::MIN, incarnation_a);
    let (b, stop_b, task_b) = start(&hub, &session_b, NonZeroUsize::MIN, incarnation_b);
    applied(&a, rebase(incarnation_a, 1, SessionState::Idle)).await;
    applied(&b, rebase(incarnation_b, 1, SessionState::Idle)).await;
    let mut observed = hub
        .attach(session_a.clone())
        .await
        .expect("initial attachment");
    let mut available = a.capacity_returned();
    let held = hub.hold_history_for_test(session_a.clone()).await;

    let buffered = a
        .try_offer(commit(
            incarnation_a,
            2,
            &[SessionState::Running, SessionState::Idle],
        ))
        .expect("occupy A's inbox");
    assert!(matches!(
        a.try_offer(commit(incarnation_a, 3, &[SessionState::Closed])),
        Err(HubPublicationRefusal::Full)
    ));
    assert_eq!(
        applied(&b, commit(incarnation_b, 2, &[SessionState::Idle])).await,
        stamp(incarnation_b, 2)
    );
    drop(held);
    assert_eq!(
        buffered.await.expect("A settles"),
        Ok(stamp(incarnation_a, 2))
    );
    available.changed().await.expect("capacity returned");
    assert_eq!(
        observed.receiver.recv().await.expect("running").event,
        SessionEvent::StateChanged {
            state: SessionState::Running
        }
    );
    assert_eq!(
        observed.receiver.recv().await.expect("idle").event,
        SessionEvent::StateChanged {
            state: SessionState::Idle
        }
    );
    assert_eq!(
        a.try_offer(commit(incarnation_a, 4, &[SessionState::Closed]))
            .expect("enqueued")
            .await
            .expect("settled"),
        Err(HubPublicationRefusal::OutOfSequence)
    );
    assert_eq!(
        hub.state(session_a.clone())
            .await
            .expect("current projection"),
        SessionState::Idle
    );

    assert_eq!(
        applied(&a, rebase(incarnation_a, 5, SessionState::Closed)).await,
        stamp(incarnation_a, 5)
    );
    assert!(matches!(
        observed
            .receiver
            .recv()
            .await
            .expect("resync control")
            .event,
        SessionEvent::ResyncRequired { .. }
    ));
    let updated = hub.attach(session_a).await.expect("attach after rebase");
    assert!(updated.epoch > observed.epoch);
    assert_eq!(
        updated
            .snapshot
            .iter()
            .map(|event| &event.event)
            .collect::<Vec<_>>(),
        vec![&SessionEvent::StateChanged {
            state: SessionState::Closed
        }]
    );
    stop_a.cancel();
    stop_b.cancel();
    task_a.await.expect("A stopped");
    task_b.await.expect("B stopped");
}

#[tokio::test(flavor = "current_thread")]
async fn replaced_owner_loses_unapplied_batches_and_new_owner_must_start_with_rebase() {
    let (_root, hub) = hub().await;
    let session = session();
    let (old, new) = (Uuid::now_v7(), Uuid::now_v7());
    let (inbox, stop, task) = start(&hub, &session, NonZeroUsize::MIN, old);
    applied(&inbox, rebase(old, 1, SessionState::Idle)).await;
    let mut attached = hub.attach(session.clone()).await.expect("initial attach");
    let held = hub.hold_history_for_test(session.clone()).await;
    let old_batch = inbox
        .try_offer(commit(old, 2, &[SessionState::Closed]))
        .expect("old offer accepted");

    inbox
        .install_incarnation(Some(old), new)
        .expect("replace old incarnation");
    assert_eq!(
        old_batch.await.expect("old offer settled"),
        Err(HubPublicationRefusal::Superseded)
    );
    assert!(matches!(
        inbox.try_offer(commit(old, 3, &[SessionState::Idle])),
        Err(HubPublicationRefusal::Superseded)
    ));
    assert_eq!(
        inbox.install_incarnation(Some(old), Uuid::now_v7()),
        Err(HubPublicationRefusal::Superseded)
    );
    let premature = inbox
        .try_offer(commit(new, 1, &[SessionState::Closed]))
        .expect("new offer enqueued");
    drop(held);
    assert_eq!(
        premature.await.expect("premature offer settled"),
        Err(HubPublicationRefusal::OutOfSequence)
    );
    assert_eq!(
        applied(&inbox, rebase(new, 2, SessionState::Running)).await,
        stamp(new, 2)
    );
    assert!(
        matches!(
            attached
                .receiver
                .recv()
                .await
                .expect("resync control")
                .event,
            SessionEvent::ResyncRequired { .. }
        ),
        "whose pending close must never be published after replacement"
    );
    assert_eq!(
        hub.state(session).await.expect("new projection"),
        SessionState::Running
    );
    stop.cancel();
    task.await.expect("inbox stopped");
}

#[tokio::test(flavor = "current_thread")]
async fn projection_refusal_does_not_publish_a_valid_prefix() {
    let (_root, hub) = hub().await;
    let session = session();
    let incarnation = Uuid::now_v7();
    let (inbox, stop, task) = start(&hub, &session, NonZeroUsize::MIN, incarnation);
    applied(&inbox, rebase(incarnation, 1, SessionState::Idle)).await;
    let mut attached = hub.attach(session.clone()).await.expect("subscribe");
    let refused = inbox
        .try_offer(HubPublication::Commit {
            stamp: stamp(incarnation, 2),
            facts: vec![
                HubPublicationFact::Event(SessionEvent::StateChanged {
                    state: SessionState::Running,
                }),
                HubPublicationFact::Event(SessionEvent::ResyncRequired { replay_epoch: 42 }),
            ],
            observed_at_seconds: 0,
        })
        .expect("offer accepted for apply")
        .await
        .expect("apply settled");
    assert_eq!(refused, Err(HubPublicationRefusal::Projection));
    assert!(matches!(
        attached.receiver.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    assert_eq!(
        hub.state(session).await.expect("old projection"),
        SessionState::Idle
    );
    stop.cancel();
    task.await.expect("inbox stopped");
}
