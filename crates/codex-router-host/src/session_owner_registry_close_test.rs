use super::*;
use provider_session_machines::session_lifecycle_machine::CloseAttempt;

async fn refused_before_enqueue(failure: TransportRefusal, running: bool) {
    let fixture = StoreFixture::new().await;
    let target = fixture.record_session("close-writer").await;
    let registry = SessionOwnerRegistry::new(
        fixture.store.clone(),
        fixture.clock.clone(),
        NonZeroUsize::new(4).expect("mailbox capacity"),
        NonZeroUsize::new(4).expect("completion capacity"),
    );
    let SessionRegistration { session, launch } =
        registry.get_or_insert(target.clone(), connection());
    let (launch, lane) = launch.expect("owner and lane");
    let stop_lane = CancellationToken::new();
    let lane_worker = tokio::spawn(lane.run(stop_lane.clone()));
    let (transport, output) = AcpTransport::new(NonZeroUsize::MIN);
    let transport = Arc::new(transport);
    let mut output = Some(output);
    let (close_probe, mut observed) = tokio::sync::mpsc::unbounded_channel();
    let stop_owner = launch.shutdown.clone();
    let owner_worker = tokio::spawn(launch.task.run(
        ScenarioPorts {
            lane: session.lane.clone(),
            transport: transport.clone(),
            target: target.clone(),
            fail_input: None,
            close_probe: Some(close_probe),
        },
        launch.shutdown,
    ));
    let generation = settle_restore(&session.owner).await;
    let mut snapshots = session.owner.handle.snapshots();
    let address = session.owner.address;
    let attempt = CloseAttempt(Uuid::now_v7());
    let close = SessionOwnerCommand::Close {
        generation,
        attempt,
    };

    let foreground = if running {
        let operation = Uuid::now_v7();
        admit_prompt(&fixture, &target, operation).await;
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
        let reader = output.as_mut().expect("writer still open");
        let prompt = reader
            .recv()
            .await
            .expect("prompt enqueued")
            .frame
            .expect("prompt token");
        session
            .owner
            .handle
            .try_submit(address, close.clone())
            .expect("close queued")
            .await
            .expect("close handled")
            .expect("close admitted");
        let cancel = reader
            .recv()
            .await
            .expect("cancel enqueued")
            .frame
            .expect("cancel token");
        assert_ne!(cancel.authorization_seq, prompt.authorization_seq);
        assert!(
            observed.try_recv().is_err(),
            "close is still draining its running turn"
        );
        Some(prompt)
    } else {
        None
    };
    match failure {
        TransportRefusal::WriterFull => transport
            .enqueue_request(
                TransportRun {
                    incarnation: address.incarnation,
                    generation,
                },
                None,
            )
            .expect("hold the single writer position"),
        TransportRefusal::WriterStopped => drop(output.take()),
        _ => panic!("this fixture exercises only writer refusal"),
    }
    if let Some(authorization) = foreground {
        session
            .owner
            .handle
            .completion_port()
            .reserve(address)
            .expect("completion capacity")
            .send(OwnerEvent::Dispatch(DispatchResult::PromptFinished {
                authorization,
                outcome: WireOutcome::Confirmed(()),
            }));
    } else {
        session
            .owner
            .handle
            .try_submit(address, close)
            .expect("close queued")
            .await
            .expect("close handled")
            .expect("close admitted");
    }

    assert_eq!(
        observed.recv().await,
        Some(CloseObservation::Enqueue(Err(failure)))
    );
    assert_eq!(
        observed.recv().await,
        Some(CloseObservation::Finished(CloseOutcome::NotEnqueued)),
        "T7 must report that close was never enqueued after draining or when already idle"
    );
    snapshots
        .wait_for(|snapshot| snapshot.lifecycle == SessionLifecycle::NotLoaded { last: generation })
        .await
        .expect("close locally retired");
    if let Some(mut reader) = output {
        assert!(
            reader
                .try_recv()
                .expect("only filler frame")
                .frame
                .is_none()
        );
        assert!(
            reader.try_recv().is_err(),
            "close did not escape into the writer"
        );
    }
    stop_owner.cancel();
    owner_worker
        .await
        .expect("owner stopped")
        .expect("clean stop");
    stop_lane.cancel();
    lane_worker.await.expect("lane stopped");
}

#[tokio::test(flavor = "current_thread")]
async fn a_full_writer_leaves_idle_close_known_unsent() {
    refused_before_enqueue(TransportRefusal::WriterFull, false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_full_writer_leaves_drained_close_known_unsent() {
    refused_before_enqueue(TransportRefusal::WriterFull, true).await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_stopped_writer_leaves_idle_close_known_unsent() {
    refused_before_enqueue(TransportRefusal::WriterStopped, false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_stopped_writer_leaves_drained_close_known_unsent() {
    refused_before_enqueue(TransportRefusal::WriterStopped, true).await;
}
