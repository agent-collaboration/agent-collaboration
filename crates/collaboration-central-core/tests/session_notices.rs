#![allow(clippy::expect_used, clippy::panic)]
mod registry_support;
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
use registry_support::*;

fn address() -> String {
    Harness::target(ObjectAddress::Connection(machine(1)))
}

fn connect(harness: &mut Harness, number: u64) {
    harness.call(
        "registry",
        admin(AdminRequest::EnrollMachine {
            label: "notice machine".into(),
            installation: machine(1),
            access_client_id: AccessClientId::new("notice-client").expect("client"),
            credential_hash: "notice-hash".into(),
            expires_at: at(60 * 24 * 30).0,
        }),
    );
    let effects = harness.run(
        &address(),
        WireInput::Session(SessionCall::Hello {
            connection: connection(number),
            credential: CredentialStatus::Machine {
                installation: machine(1),
                expires_at: at(60 * 24 * 30).0,
            },
            installation: machine(1),
            session_id: session(number),
            schema_support: SchemaSupport { max_version: 10 },
        }),
    );
    let mut welcomed = false;
    for effect in effects {
        if let WireEffect::Dispatch {
            call_id,
            to,
            call,
            carry,
        } = effect
        {
            let result = harness.call(&Harness::target(to), call);
            let answer = harness.run(
                &address(),
                WireInput::CallResult {
                    source: CallerKind::Session,
                    call_id,
                    result: Box::new(result),
                    carry,
                },
            );
            welcomed |= answer.iter().any(|effect| matches!(effect, WireEffect::Send { to, frame: CentralFrame::Welcome { projects, .. } } if *to == connection(number) && projects.is_empty()));
        }
    }
    assert!(
        welcomed,
        "real Hello/RegisterSession/Welcome with no held projects"
    );
}

fn deliver(harness: &mut Harness, kind: NoticeKind) -> Vec<WireEffect> {
    let (input, effects) = harness.begin(
        &address(),
        WireCall::Delivery(DeliveryCall::DeliverNotice(kind)),
    );
    validate_call_returns(&input, &effects).expect("one notice ACK");
    assert!(effects.iter().any(|effect| matches!(
        effect,
        WireEffect::Return {
            result: WireResult::Delivery(DeliveryResult::NoticeDelivered),
            ..
        }
    )));
    effects
}

fn granted() -> NoticeKind {
    NoticeKind::LeaseGranted {
        project: project(),
        epoch: epoch(2),
    }
}

fn read(harness: &mut Harness, cursor: Option<ReadCursor>) -> SessionAnswer {
    let effects = harness.run(
        &address(),
        WireInput::Session(SessionCall::Frame {
            connection: connection(1),
            frame_id: FrameId::new(identity(700)).expect("frame"),
            request: SessionRequest::Read {
                scope: ReadScope::Notices,
                after: cursor,
            },
        }),
    );
    assert!(
        !effects.iter().any(|effect| matches!(
            effect,
            WireEffect::Dispatch { .. } | WireEffect::Notify { .. }
        )),
        "session reads its own notices without an RPC or self-call"
    );
    let answers: Vec<_> = effects
        .into_iter()
        .filter_map(|effect| match effect {
            WireEffect::Send {
                to,
                frame: CentralFrame::Answer { body, .. },
            } if to == connection(1) => Some(body),
            _ => None,
        })
        .collect();
    assert_eq!(answers.len(), 1);
    answers.into_iter().next().expect("answer")
}

fn notices(answer: SessionAnswer, now: RegistryTime) -> Vec<Notice> {
    let SessionAnswer::ReadPage(ReadPage::Notices { notices, as_of }) = answer else {
        panic!("notice page: {answer:?}")
    };
    assert_eq!(as_of, now.0);
    notices
}

#[test]
fn delivery_assigns_its_own_positive_positions_and_session_reads_locally() {
    let mut harness = Harness::new();
    connect(&mut harness, 1);
    for expected in 1..=2 {
        let effects = deliver(&mut harness, granted());
        assert!(effects.iter().any(|effect| matches!(effect, WireEffect::Send { to, frame: CentralFrame::Notice { position, kind } } if *to == connection(1) && position.value() == expected && *kind == granted())));
    }
    let page = notices(read(&mut harness, None), start());
    assert_eq!(
        page.iter()
            .map(|notice| notice.position.value())
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert!(page.iter().all(|notice| notice.kind == granted()));
    assert_eq!(
        notices(
            read(
                &mut harness,
                Some(ReadCursor::Notices(
                    NoticePosition::new(1).expect("notice position")
                ))
            ),
            start()
        ),
        vec![page.get(1).expect("second notice").clone()]
    );
    assert_eq!(
        read(&mut harness, Some(ReadCursor::Fleet(1.into()))),
        SessionAnswer::Refused(SessionRefusal::InvalidShape {
            field: "after".into()
        })
    );
}

#[test]
fn disconnected_notice_is_durable_and_prune_all_never_resets_its_counter() {
    let mut harness = Harness::new();
    let effects = deliver(&mut harness, granted());
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, WireEffect::Send { .. }))
    );
    deliver(&mut harness, granted());
    connect(&mut harness, 1);
    assert_eq!(notices(read(&mut harness, None), start()).len(), 2);
    let expiry = at(7 * 24 * 60);
    assert!(
        read_object_alarm_plan(
            harness.store(&address()),
            &ObjectAddress::Connection(machine(1))
        )
        .expect("alarm plan")
        .contains(&FaceAlarm {
            face: CallerKind::Delivery,
            at: expiry
        })
    );
    harness.now = expiry;
    harness.run(
        &address(),
        WireInput::Alarm {
            face: CallerKind::Delivery,
        },
    );
    assert!(notices(read(&mut harness, None), expiry).is_empty());
    deliver(&mut harness, granted());
    let after = notices(
        read(
            &mut harness,
            Some(ReadCursor::Notices(
                NoticePosition::new(2).expect("notice position"),
            )),
        ),
        expiry,
    );
    assert_eq!(after.len(), 1);
    assert_eq!(after.first().expect("new notice").position.value(), 3);
    assert_eq!(
        read(
            &mut harness,
            Some(ReadCursor::Notices(
                NoticePosition::new(1).expect("position")
            ))
        ),
        SessionAnswer::Refused(SessionRefusal::CursorExpired)
    );
    assert_eq!(
        read(
            &mut harness,
            Some(ReadCursor::Notices(
                NoticePosition::new(4).expect("notice position")
            ))
        ),
        SessionAnswer::Refused(SessionRefusal::InvalidShape {
            field: "after".into()
        })
    );
}

#[test]
fn fleet_hint_uses_delivery_sequence_instead_of_fleet_cursor_and_old_rpc_is_closed() {
    let mut harness = Harness::new();
    connect(&mut harness, 1);
    let kind = NoticeKind::FleetFeedAdvanced {
        through: FleetPosition::new(900),
    };
    let effects = deliver(&mut harness, kind.clone());
    assert!(effects.iter().any(|effect| matches!(effect,WireEffect::Send { frame: CentralFrame::Notice { position,kind: actual },.. } if position.value()==1 && *actual==kind)));
    let retained = notices(read(&mut harness, None), start());
    assert_eq!(
        retained,
        vec![Notice {
            position: NoticePosition::new(1).expect("position"),
            kind
        }]
    );
    assert!(serde_json::from_value::<DeliveryCall>(serde_json::json!({"kind":"deliverNotice","value":{"position":999,"kind":{"kind":"fleetFeedAdvanced","through":900}}})).is_err(),"old caller-numbered private RPC has no compatibility path");
}

#[test]
fn a_late_counter_failure_rolls_back_the_notice_and_next_position() {
    let mut harness = Harness::new();
    connect(&mut harness, 1);
    deliver(&mut harness, granted());
    harness.store(&address()).connection.execute_batch("CREATE TRIGGER fail_notice_counter BEFORE UPDATE OF last_position ON _session_notice_head BEGIN SELECT RAISE(ABORT,'injected counter failure'); END;").expect("counter fault");
    let result = harness.store(&address()).run(
        WireInput::Call {
            from: ObjectAddress::Registry,
            call_id: ObjectCallId::new(identity(990_003)).expect("call"),
            call: Box::new(WireCall::Delivery(DeliveryCall::DeliverNotice(granted()))),
        },
        start(),
    );
    assert!(matches!(
        result,
        Err(CentralError::Store(StoreError::Constraint { .. }))
    ));
    assert_eq!(notices(read(&mut harness, None), start()).len(), 1);
    harness
        .store(&address())
        .connection
        .execute_batch("DROP TRIGGER fail_notice_counter")
        .expect("remove fault");
    deliver(&mut harness, granted());
    let retained = notices(read(&mut harness, None), start());
    assert_eq!(
        retained
            .iter()
            .map(|notice| notice.position.value())
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
}
