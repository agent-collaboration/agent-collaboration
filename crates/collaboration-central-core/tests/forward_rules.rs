#![allow(clippy::expect_used, clippy::panic)]
mod registry_support;
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
use registry_support::*;
fn address(n: u64) -> String {
    format!("session:{}", machine(n).as_str())
}
fn post() -> ForwardedPost {
    ForwardedPost {
        kind: ForwardedPostKind::ThreadPost,
        body: BoardPayload::new(
            serde_json::json!({"secretPostMarker":"never-persist-this","opaque":i64::MAX}),
        ),
    }
}
fn setup() -> Harness {
    let mut h = Harness::setup();
    for n in [1, 2] {
        h.run(
            &address(n),
            WireInput::Session(SessionCall::Hello {
                connection: connection(n),
                credential: CredentialStatus::Machine {
                    installation: machine(n),
                    expires_at: at(1440).0,
                },
                installation: machine(n),
                session_id: session(n),
                schema_support: SchemaSupport { max_version: 10 },
            }),
        );
    }
    h
}
fn start(h: &mut Harness, n: u64, frame: u64, project: ProjectId) -> Vec<WireEffect> {
    h.run(
        &address(n),
        WireInput::Session(SessionCall::Frame {
            connection: connection(n),
            frame_id: FrameId::new(identity(1000 + frame)).expect("frame"),
            request: SessionRequest::Forward {
                project,
                post: post(),
            },
        }),
    )
}
fn dispatch(effects: &[WireEffect]) -> (ObjectCallId, WireCall, Option<DispatchCarry>) {
    effects
        .iter()
        .find_map(|effect| match effect {
            WireEffect::Dispatch {
                call_id,
                call,
                carry,
                ..
            } => Some((call_id.clone(), call.clone(), carry.clone())),
            _ => None,
        })
        .expect("dispatch")
}
fn result(
    h: &mut Harness,
    n: u64,
    id: ObjectCallId,
    value: WireResult,
    carry: Option<DispatchCarry>,
) -> Vec<WireEffect> {
    h.run(
        &address(n),
        WireInput::CallResult {
            source: CallerKind::Session,
            call_id: id,
            result: Box::new(value),
            carry,
        },
    )
}
fn route_to_holder(h: &mut Harness, effects: &[WireEffect]) -> Vec<WireEffect> {
    let (id, call, carry) = dispatch(effects);
    let routed = h.call("registry", call);
    result(h, 2, id, routed, carry)
}
fn outcome(effects: &[WireEffect]) -> Option<ForwardOutcome> {
    effects.iter().find_map(|effect| match effect {
        WireEffect::Send {
            frame:
                CentralFrame::Answer {
                    body: SessionAnswer::Forwarded(outcome),
                    ..
                },
            ..
        } => Some(outcome.clone()),
        _ => None,
    })
}
fn pending(h: &mut Harness, n: u64) -> i64 {
    h.store(&address(n))
        .connection
        .query_row("SELECT count(*) FROM pending_forwards", [], |row| {
            row.get(0)
        })
        .expect("count")
}
#[test]
fn slot_is_reserved_before_route_and_payload_is_never_stored() {
    let mut h = setup();
    for frame in 0..16 {
        let effects = start(&mut h, 2, frame, project());
        assert!(outcome(&effects).is_none());
        assert!(matches!(
            dispatch(&effects).2,
            Some(DispatchCarry::ForwardPost { .. })
        ));
    }
    assert_eq!(pending(&mut h, 2), 16);
    assert_eq!(
        outcome(&start(&mut h, 2, 17, project())),
        Some(ForwardOutcome::Overloaded)
    );
    let tx = h.store(&address(2));
    let rows:Vec<String>=tx.connection.prepare("SELECT record FROM pending_forwards UNION ALL SELECT record FROM _central_continuations").expect("query").query_map([],|r|r.get(0)).expect("rows").collect::<Result<_,_>>().expect("collect");
    assert!(rows.iter().all(|row| !row.contains("secretPostMarker")
        && !row.contains("never-persist-this")
        && !row.contains("opaque")));
}
#[test]
fn terminal_route_failure_and_deadline_free_slots_and_ignore_late_results() {
    let mut h = setup();
    let effects = start(&mut h, 2, 1, project());
    let (id, _, carry) = dispatch(&effects);
    let failed = h.run(
        &address(2),
        WireInput::CallFailed {
            source: CallerKind::Session,
            call_id: id,
            carry,
        },
    );
    assert_eq!(outcome(&failed), Some(ForwardOutcome::Unanswered));
    assert_eq!(pending(&mut h, 2), 0);
    let own = start(&mut h, 1, 2, project());
    let (id, call, carry) = dispatch(&own);
    let route = h.call("registry", call);
    assert_eq!(
        outcome(&result(&mut h, 1, id, route, carry)),
        Some(ForwardOutcome::SenderHolds)
    );
    assert_eq!(pending(&mut h, 1), 0);
    let unknown = ProjectId::new(identity(900)).expect("project");
    let effects = start(&mut h, 2, 3, unknown);
    let (id, call, carry) = dispatch(&effects);
    let route = h.call("registry", call);
    assert_eq!(
        outcome(&result(&mut h, 2, id, route, carry)),
        Some(ForwardOutcome::NotHeld)
    );
    assert_eq!(pending(&mut h, 2), 0);
    let effects = start(&mut h, 2, 4, project());
    let (id, call, carry) = dispatch(&effects);
    let route = h.call("registry", call);
    h.now = at(1);
    let expired = h.run(
        &address(2),
        WireInput::Alarm {
            face: CallerKind::Session,
        },
    );
    assert_eq!(outcome(&expired), Some(ForwardOutcome::Unanswered));
    assert!(result(&mut h, 2, id, route, carry).is_empty());
    assert_eq!(pending(&mut h, 2), 0);
}
#[test]
fn delivery_and_answer_authority_preserve_original_deadline_and_drop_late_delivery() {
    let mut h = setup();
    let first = start(&mut h, 2, 1, project());
    let effects = route_to_holder(&mut h, &first);
    let (id, call, carry) = dispatch(&effects);
    let WireCall::Delivery(DeliveryCall::DeliverForward {
        forward_id,
        deadline,
        ..
    }) = call
    else {
        panic!("delivery")
    };
    assert_eq!(deadline, start_time() + chrono::TimeDelta::seconds(30));
    let wrong = WireInput::Call {
        from: ObjectAddress::Connection(machine(3)),
        call_id: ObjectCallId::new(identity(900)).expect("call"),
        call: Box::new(WireCall::Delivery(DeliveryCall::ForwardAnswered {
            forward_id,
            project: project(),
            outcome: ForwardOutcome::Answered {
                reply: BoardPayload::new(serde_json::json!({"wrong":true})),
            },
        })),
    };
    assert!(outcome(&h.run(&address(2), wrong)).is_none());
    assert_eq!(pending(&mut h, 2), 1);
    h.now = at(1);
    let expired = h.run(
        &address(2),
        WireInput::Alarm {
            face: CallerKind::Session,
        },
    );
    assert_eq!(outcome(&expired), Some(ForwardOutcome::Unanswered));
    assert!(
        result(
            &mut h,
            2,
            id,
            WireResult::Delivery(DeliveryResult::DeliverForward(
                ForwardDeliveryResult::NoConnection
            )),
            carry
        )
        .is_empty()
    );
    assert_eq!(pending(&mut h, 2), 0);
}
fn start_time() -> chrono::DateTime<chrono::Utc> {
    registry_support::start().0
}
#[test]
fn authorized_answer_and_no_connection_each_release_the_reserved_slot() {
    let mut h = setup();
    let initial = start(&mut h, 2, 1, project());
    let effects = route_to_holder(&mut h, &initial);
    let (id, call, carry) = dispatch(&effects);
    let WireCall::Delivery(DeliveryCall::DeliverForward { forward_id, .. }) = call else {
        panic!("deliver")
    };
    let unavailable = result(
        &mut h,
        2,
        id,
        WireResult::Delivery(DeliveryResult::DeliverForward(
            ForwardDeliveryResult::NoConnection,
        )),
        carry,
    );
    assert_eq!(
        outcome(&unavailable),
        Some(ForwardOutcome::HolderUnreachable {
            last_seen: registry_support::start().0
        })
    );
    assert_eq!(pending(&mut h, 2), 0);
    let initial = start(&mut h, 2, 2, project());
    let effects = route_to_holder(&mut h, &initial);
    let (_, call, _) = dispatch(&effects);
    let WireCall::Delivery(DeliveryCall::DeliverForward {
        forward_id: current,
        ..
    }) = call
    else {
        panic!("deliver")
    };
    let reply = BoardPayload::new(serde_json::json!({"fixed":"reply"}));
    let answer = WireInput::Call {
        from: ObjectAddress::Connection(machine(1)),
        call_id: ObjectCallId::new(identity(990)).expect("call"),
        call: Box::new(WireCall::Delivery(DeliveryCall::ForwardAnswered {
            forward_id: current,
            project: project(),
            outcome: ForwardOutcome::Answered {
                reply: reply.clone(),
            },
        })),
    };
    assert_eq!(
        outcome(&h.run(&address(2), answer)),
        Some(ForwardOutcome::Answered { reply })
    );
    assert_eq!(pending(&mut h, 2), 0);
    let _ = forward_id;
}

#[test]
fn expired_machine_cannot_answer_a_forward_and_its_session_closes() {
    let mut h = setup();
    h.now = at(1441);
    let effects = h.run(
        &address(1),
        WireInput::Session(SessionCall::ForwardAnswer {
            connection: connection(1),
            forward_id: ForwardId::new(identity(991)).expect("forward"),
            reply: BoardPayload::new(serde_json::json!({})),
        }),
    );
    assert!(effects.iter().any(|effect| matches!(
        effect,
        WireEffect::Send {
            frame: CentralFrame::Closed {
                reason: SessionCloseReason::CredentialRejected(CredentialRejection::Expired)
            },
            ..
        }
    )));
}

#[test]
fn holder_storage_excludes_content_and_failed_delivery_frees_the_sender_slot() {
    let mut h = setup();
    let first = start(&mut h, 2, 1, project());
    let delivery = route_to_holder(&mut h, &first);
    let (call_id, call, _) = dispatch(&delivery);
    let WireCall::Delivery(DeliveryCall::DeliverForward { forward_id, .. }) = &call else {
        panic!("delivery")
    };
    let forward_id = forward_id.clone();
    let effects = h.run(
        &address(1),
        WireInput::Call {
            from: ObjectAddress::Connection(machine(2)),
            call_id,
            call: Box::new(call),
        },
    );
    assert!(effects.iter().any(|effect| matches!(effect,
        WireEffect::Send { frame: CentralFrame::Forwarded { post: delivered, .. }, .. }
        if delivered == &post()
    )));
    let stored: String = h
        .store(&address(1))
        .connection
        .query_row(
            "SELECT record FROM delivered_forwards WHERE forward_id=?",
            [forward_id.as_str()],
            |row| row.get(0),
        )
        .expect("delivered correlation exists");
    for marker in [
        "secretPostMarker",
        "never-persist-this",
        "opaque",
        "replyMarker",
    ] {
        assert!(
            !stored.contains(marker),
            "content in holder storage: {marker}"
        );
    }
    let replied = h.run(
        &address(1),
        WireInput::Session(SessionCall::ForwardAnswer {
            connection: connection(1),
            forward_id,
            reply: BoardPayload::new(serde_json::json!({"replyMarker":"transient"})),
        }),
    );
    let (call_id, call, _) = dispatch(&replied);
    let effects = h.run(
        &address(2),
        WireInput::Call {
            from: ObjectAddress::Connection(machine(1)),
            call_id,
            call: Box::new(call),
        },
    );
    assert_eq!(
        outcome(&effects),
        Some(ForwardOutcome::Answered {
            reply: BoardPayload::new(serde_json::json!({"replyMarker":"transient"})),
        })
    );
    assert_eq!(pending(&mut h, 2), 0);
    let remaining: i64 = h
        .store(&address(1))
        .connection
        .query_row("SELECT COUNT(*) FROM delivered_forwards", [], |row| {
            row.get(0)
        })
        .expect("holder count");
    assert_eq!(remaining, 0);

    let next = start(&mut h, 2, 2, project());
    let delivery = route_to_holder(&mut h, &next);
    let (call_id, _, carry) = dispatch(&delivery);
    let failed = WireInput::CallFailed {
        source: CallerKind::Session,
        call_id,
        carry,
    };
    assert_eq!(
        outcome(&h.run(&address(2), failed.clone())),
        Some(ForwardOutcome::Unanswered)
    );
    assert_eq!(pending(&mut h, 2), 0);
    assert!(h.run(&address(2), failed).is_empty());
}

#[test]
fn delivery_alarm_owns_only_delivered_rows_and_preserves_the_session_deadline() {
    let mut harness = setup();
    let initial = start(&mut harness, 2, 90, project());
    let routed = route_to_holder(&mut harness, &initial);
    let (call_id, call, _) = dispatch(&routed);
    harness.run(
        &address(1),
        WireInput::Call {
            from: ObjectAddress::Connection(machine(2)),
            call_id,
            call: Box::new(call),
        },
    );
    let plan = read_object_alarm_plan(
        harness.store(&address(1)),
        &ObjectAddress::Connection(machine(1)),
    )
    .expect("alarm plan");
    assert!(plan.contains(&FaceAlarm {
        face: CallerKind::Delivery,
        at: RegistryTime(start_time() + chrono::TimeDelta::seconds(30))
    }));
    assert!(plan.contains(&FaceAlarm {
        face: CallerKind::Session,
        at: at(1440)
    }));
    harness.now = at(1);
    let effects = harness.run(
        &address(1),
        WireInput::Alarm {
            face: CallerKind::Delivery,
        },
    );
    assert!(
        effects.is_empty(),
        "delivery cleanup cannot invoke Registry or close the live session"
    );
    let remaining: i64 = harness
        .store(&address(1))
        .connection
        .query_row("SELECT COUNT(*) FROM delivered_forwards", [], |row| {
            row.get(0)
        })
        .expect("delivered count");
    assert_eq!(remaining, 0);
    assert_eq!(
        pending(&mut harness, 2),
        1,
        "sender's pending row belongs to its session face"
    );
    let after = read_object_alarm_plan(
        harness.store(&address(1)),
        &ObjectAddress::Connection(machine(1)),
    )
    .expect("remaining alarm plan");
    assert_eq!(
        after,
        vec![FaceAlarm {
            face: CallerKind::Session,
            at: at(1440)
        }]
    );
}
