use super::*;
use session_event_model::InputId;
use tokio::sync::mpsc::error::TryRecvError;
use uuid::Uuid;

fn route(generation: u64) -> TransportRun {
    TransportRun {
        incarnation: OwnerIncarnation(Uuid::now_v7()),
        generation: SessionGeneration::from_number(generation),
    }
}

fn new_transport(
    capacity: usize,
) -> (
    AcpTransport<&'static str>,
    mpsc::Receiver<OrderedOutput<&'static str>>,
) {
    AcpTransport::new(NonZeroUsize::new(capacity).expect("nonzero test capacity"))
}

fn authorization(run: TransportRun, target: AuthorizationTarget) -> DispatchAuthorization {
    DispatchAuthorization {
        incarnation: run.incarnation,
        run: run.generation,
        input_id: InputId::generate(),
        authorization_seq: 1,
        target,
    }
}

fn foreground(transport: &AcpTransport<&str>, run: TransportRun, label: &'static str) -> TurnId {
    let turn = TurnId(Uuid::now_v7());
    let token = authorization(run, AuthorizationTarget::NewTurn);
    transport.apply_membership(
        run.incarnation,
        TurnMembership::Provisional {
            run: run.generation,
            turn,
        },
    );
    transport
        .offer(token.clone(), Some(turn), label)
        .expect("offer the foreground turn");
    assert_eq!(transport.take(&token), Ok(DispatchResult::Taken(token)));
    turn
}

#[test]
fn held_steer_cannot_enqueue_on_a_successor_turn_on_the_same_run() {
    let (transport, mut writer) = new_transport(1);
    let run = route(1);
    transport.open_route(run, "session".into());
    transport.activate_route(run);
    let first = foreground(&transport, run, "prompt A");

    let steer = authorization(run, AuthorizationTarget::ExistingTurn { turn: first });
    transport
        .offer(steer.clone(), None, "steer A")
        .expect("offer steer");
    assert_eq!(transport.take(&steer), Err(TransportRefusal::WriterFull));
    assert_eq!(
        writer.try_recv().expect("first queued frame").frame,
        "prompt A"
    );
    transport.apply_membership(
        run.incarnation,
        TurnMembership::End {
            run: run.generation,
            turn: first,
        },
    );
    let second = foreground(&transport, run, "prompt B");
    assert_eq!(
        writer.try_recv().expect("second queued frame").frame,
        "prompt B"
    );

    assert_eq!(
        transport.take(&steer),
        Ok(DispatchResult::StaleTarget(steer)),
        "an authorization for A must be known unsent once B owns the route"
    );
    assert!(matches!(writer.try_recv(), Err(TryRecvError::Empty)));
    assert_eq!(
        transport.resolve_callback("new", CallbackKind::Request),
        CallbackResolution::NeverObserved {
            sole_eligible: Some((run, second))
        }
    );
}

#[test]
fn only_exact_slot_enqueues_and_taken_evidence_survives_close_and_retirement() {
    let (transport, mut writer) = new_transport(4);
    let run = route(1);
    let replacement = TransportRun {
        generation: SessionGeneration::from_number(2),
        ..run
    };
    transport.open_route(run, "session".into());
    transport.activate_route(run);
    let taken = authorization(run, AuthorizationTarget::Session);
    let revoked = authorization(run, AuthorizationTarget::Session);
    let closing = authorization(run, AuthorizationTarget::Session);
    transport
        .offer(taken.clone(), None, "taken")
        .expect("offer");
    transport
        .offer(revoked.clone(), None, "revoked")
        .expect("offer");
    transport
        .offer(closing.clone(), None, "blocked by close")
        .expect("offer");

    let impostors = [
        DispatchAuthorization {
            incarnation: replacement.incarnation,
            run: replacement.generation,
            ..taken.clone()
        },
        DispatchAuthorization {
            incarnation: OwnerIncarnation(Uuid::now_v7()),
            ..taken.clone()
        },
        DispatchAuthorization {
            input_id: revoked.input_id.clone(),
            authorization_seq: 2,
            ..taken.clone()
        },
        DispatchAuthorization {
            authorization_seq: 2,
            ..taken.clone()
        },
    ];
    for impostor in impostors {
        assert_eq!(
            transport.take(&impostor),
            Err(TransportRefusal::AuthorizationMismatch)
        );
        assert_eq!(
            transport.revoke(&impostor),
            Err(TransportRefusal::AuthorizationMismatch)
        );
    }

    assert_eq!(
        transport.take(&taken),
        Ok(DispatchResult::Taken(taken.clone()))
    );
    assert_eq!(
        transport.revoke(&revoked),
        Ok(DispatchResult::Revoked(revoked.clone()))
    );
    assert_eq!(
        transport.take(&revoked),
        Ok(DispatchResult::DefinitelyUnsent(revoked.clone()))
    );
    transport
        .enqueue_close(run, CloseAttempt(Uuid::now_v7()), "close")
        .expect("enqueue close");
    transport.open_route(replacement, "session".into());
    transport.activate_route(replacement);
    assert_eq!(
        transport.take(&closing),
        Ok(DispatchResult::DefinitelyUnsent(closing.clone()))
    );
    transport.apply_membership(run.incarnation, TurnMembership::Retire(run.generation));
    assert_eq!(
        transport.revoke(&taken),
        Ok(DispatchResult::AlreadyTaken(taken.clone()))
    );

    let evidence = transport
        .revoke_incarnation(run.incarnation)
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    assert_eq!(evidence.get(&taken), Some(&SlotEvidence::Taken));
    assert_eq!(evidence.get(&revoked), Some(&SlotEvidence::Revoked));
    assert_eq!(evidence.get(&closing), Some(&SlotEvidence::Revoked));
    for expected in ["taken", "close"] {
        let frame = writer.try_recv().expect("output frame");
        assert_eq!((frame.run, frame.frame), (run, expected));
    }
    assert!(matches!(writer.try_recv(), Err(TryRecvError::Empty)));
}

#[test]
fn writer_refusal_does_not_mark_close_as_enqueued() {
    let (transport, mut writer) = new_transport(1);
    let run = route(1);
    transport.open_route(run, "session".into());
    transport
        .enqueue_request(run, "pending")
        .expect("occupy the held output");
    let close = CloseAttempt(Uuid::now_v7());
    assert_eq!(
        transport.enqueue_close(run, close, "close"),
        Err(TransportRefusal::WriterFull)
    );
    assert_eq!(writer.try_recv().expect("first output").frame, "pending");
    transport
        .enqueue_request(run, "usable")
        .expect("failed close leaves the route usable");
    assert_eq!(
        writer.try_recv().expect("request after failed close").frame,
        "usable"
    );

    transport
        .enqueue_close(run, close, "close")
        .expect("retry exact close");
    let another_close = CloseAttempt(Uuid::now_v7());
    transport.reopen_route(run, another_close);
    assert_eq!(
        transport.enqueue_request(run, "too early"),
        Err(TransportRefusal::Closing)
    );
    transport.reopen_route(run, close);
    assert_eq!(writer.try_recv().expect("close output").frame, "close");
    transport
        .enqueue_request(run, "reopened")
        .expect("matching refusal reopened route");
}

#[test]
fn provisional_membership_cannot_supply_fallback_and_revoke_removes_only_its_turn() {
    let (transport, mut writer) = new_transport(2);
    let run = route(1);
    let other = route(1);
    for (key, session) in [(run, "one"), (other, "two")] {
        transport.open_route(key, session.into());
        transport.activate_route(key);
    }
    let live = foreground(&transport, run, "first");
    let provisional = TurnId(Uuid::now_v7());
    transport.apply_membership(
        other.incarnation,
        TurnMembership::Provisional {
            run: other.generation,
            turn: provisional,
        },
    );
    let token = authorization(other, AuthorizationTarget::NewTurn);
    transport
        .offer(token.clone(), Some(provisional), "second")
        .expect("offer provisional");
    let fallback = CallbackResolution::NeverObserved {
        sole_eligible: Some((run, live)),
    };
    assert_eq!(
        transport.resolve_callback("unknown", CallbackKind::Request),
        fallback
    );
    assert_eq!(transport.revoke(&token), Ok(DispatchResult::Revoked(token)));
    assert_eq!(
        transport.resolve_callback("unknown", CallbackKind::Request),
        fallback
    );
    assert_eq!(writer.try_recv().expect("first output").frame, "first");
    assert!(matches!(writer.try_recv(), Err(TryRecvError::Empty)));
}

#[test]
fn callback_collision_outlives_retirement_and_idle_resolution_checks_continuation() {
    let (transport, _writer) = new_transport(3);
    let first = route(1);
    let other = route(1);
    for (key, session) in [(first, "one"), (other, "two")] {
        transport.open_route(key, session.into());
        transport.activate_route(key);
    }
    let turn = foreground(&transport, first, "first");
    foreground(&transport, other, "other");
    for tool in ["shared", "idle-item"] {
        assert!(matches!(
            transport.observe_tool(first, tool, ToolObservation::Announcement),
            ToolOwnershipEffect::ForwardFrame { .. }
        ));
    }
    assert_eq!(
        transport.resolve_callback("shared", CallbackKind::Request),
        CallbackResolution::Exact { run: first, turn }
    );
    assert!(matches!(
        transport.observe_tool(other, "shared", ToolObservation::Announcement),
        ToolOwnershipEffect::ForwardFrame { .. }
    ));
    transport.apply_membership(other.incarnation, TurnMembership::Retire(other.generation));
    assert_eq!(
        transport.resolve_callback("shared", CallbackKind::Request),
        CallbackResolution::Ambiguous
    );

    transport.apply_membership(
        first.incarnation,
        TurnMembership::End {
            run: first.generation,
            turn,
        },
    );
    assert_eq!(
        transport.resolve_callback("idle-item", CallbackKind::Request),
        CallbackResolution::Retired
    );
    assert_eq!(
        transport.resolve_callback("idle-item", CallbackKind::Continuation),
        CallbackResolution::ExactIdle {
            run: first,
            item_id: "idle-item".into()
        }
    );
    transport.apply_membership(first.incarnation, TurnMembership::Retire(first.generation));
    assert_eq!(
        transport.resolve_callback("idle-item", CallbackKind::Continuation),
        CallbackResolution::Retired
    );
}
