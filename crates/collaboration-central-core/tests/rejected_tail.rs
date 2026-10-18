#![allow(clippy::expect_used, clippy::panic)]
mod registry_support;
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
use registry_support::*;

fn address(sender: u64) -> String {
    Harness::target(ObjectAddress::Connection(machine(sender)))
}
fn drive(harness: &mut Harness, sender: u64, mut next: Vec<WireEffect>) -> Vec<WireEffect> {
    let mut trace = next.clone();
    loop {
        let calls: Vec<_> = next
            .into_iter()
            .filter_map(|effect| match effect {
                WireEffect::Dispatch {
                    call_id,
                    to,
                    call,
                    carry,
                } => Some((call_id, to, call, carry)),
                _ => None,
            })
            .collect();
        if calls.is_empty() {
            break;
        }
        next = Vec::new();
        for (call_id, to, call, carry) in calls {
            let result = harness.call(&Harness::target(to), call);
            let effects = harness.run(
                &address(sender),
                WireInput::CallResult {
                    source: CallerKind::Session,
                    call_id,
                    result: Box::new(result),
                    carry,
                },
            );
            trace.extend(effects.clone());
            next.extend(effects);
        }
    }
    trace
}
fn connect(harness: &mut Harness, sender: u64) {
    let effects = harness.run(
        &address(sender),
        WireInput::Session(SessionCall::Hello {
            connection: connection(sender),
            credential: CredentialStatus::Machine {
                installation: machine(sender),
                expires_at: at(1440).0,
            },
            installation: machine(sender),
            session_id: session(sender),
            schema_support: SchemaSupport { max_version: 10 },
        }),
    );
    assert!(drive(harness,sender,effects).iter().any(|effect|matches!(effect,WireEffect::Send {to,frame:CentralFrame::Welcome {..}} if *to==connection(sender))));
}
fn setup() -> Harness {
    let mut harness = Harness::setup();
    let takeover = harness.takeover_call(77);
    assert!(
        matches!(harness.call("registry",takeover),WireResult::Registry(RegistryResult::Admin(AdminResult::Granted {new_epoch,..})) if new_epoch==epoch(2))
    );
    connect(&mut harness, 1);
    harness
}
fn begin(harness: &mut Harness, sender: u64, reported_epoch: u64, digest: &str) -> Vec<WireEffect> {
    harness.run(
        &address(sender),
        WireInput::Session(SessionCall::Frame {
            connection: connection(sender),
            frame_id: FrameId::new(identity(700)).expect("frame"),
            request: SessionRequest::TailReport {
                project: project(),
                epoch: epoch(reported_epoch),
                request_ids: vec![request(77)],
                digest: TailDigest::new(digest.to_owned()).expect("digest"),
            },
        }),
    )
}
fn assert_verification(effects: &[WireEffect], sender: u64, reported_epoch: u64) {
    let [
        WireEffect::Dispatch {
            to: ObjectAddress::Registry,
            call,
            ..
        },
    ] = effects
    else {
        panic!("tail must first ask Registry, not store in Fleet: {effects:?}");
    };
    assert_eq!(
        serde_json::to_value(call).expect("call"),
        serde_json::json!({"kind":"registry","value":{"kind":"verifyTail","value":{"project":project().as_str(),"epoch":reported_epoch,"installation":machine(sender).as_str()}}})
    );
}
fn answer(trace: &[WireEffect], sender: u64) -> SessionAnswer {
    let answers: Vec<_> = trace
        .iter()
        .filter_map(|effect| match effect {
            WireEffect::Send {
                to,
                frame: CentralFrame::Answer { frame_id, body },
            } if *to == connection(sender) && frame_id.as_str() == identity(700) => {
                Some(body.clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(answers.len(), 1, "one correlated answer");
    answers.into_iter().next().expect("answer")
}

#[test]
fn historical_holder_is_verified_before_its_tail_reaches_the_real_fleet() {
    let mut harness = setup();
    let effects = begin(&mut harness, 1, 1, &"ab".repeat(32));
    assert_verification(&effects, 1, 1);
    let trace = drive(&mut harness, 1, effects);
    assert_eq!(answer(&trace, 1), SessionAnswer::TailStored);
    let calls: Vec<_> = trace
        .iter()
        .filter_map(|effect| match effect {
            WireEffect::Dispatch { call, .. } => Some(call.kind()),
            _ => None,
        })
        .collect();
    assert_eq!(calls, vec![CallerKind::Registry, CallerKind::Fleet]);
}

#[test]
fn never_writer_is_refused_without_a_fleet_store_or_tail_notice() {
    let mut harness = setup();
    harness.enroll(3);
    connect(&mut harness, 3);
    let effects = begin(&mut harness, 3, 1, &"ab".repeat(32));
    assert_verification(&effects, 3, 1);
    let trace = drive(&mut harness, 3, effects);
    assert_eq!(
        answer(&trace, 3),
        SessionAnswer::Refused(SessionRefusal::NotWriter {
            current: Some(epoch(2))
        })
    );
    assert!(
        !trace.iter().any(|effect| matches!(
            effect,
            WireEffect::Dispatch {
                to: ObjectAddress::Fleet,
                ..
            } | WireEffect::Notify { .. }
        )),
        "failed verification cannot reach Fleet or announce a report"
    );
}

fn read(harness: &mut Harness, cursor: Option<ReadCursor>) -> SessionAnswer {
    match harness.call(
        "fleet",
        WireCall::Fleet(FleetCall::Read {
            installation: machine(2),
            scope: ReadScope::TailReports { project: project() },
            after: cursor,
        }),
    ) {
        WireResult::Fleet(FleetResult::Read(answer)) => answer,
        other => panic!("tail read: {other:?}"),
    }
}
fn reports(harness: &mut Harness) -> Vec<TailReportRow> {
    match read(harness, None) {
        SessionAnswer::ReadPage(ReadPage::TailReports { reports, as_of }) => {
            assert_eq!(as_of, harness.now.0);
            reports
        }
        other => panic!("tail page: {other:?}"),
    }
}
fn count(harness: &mut Harness, table: &str) -> i64 {
    harness
        .store("fleet")
        .connection
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .expect("count")
}

#[test]
fn first_tail_wins_with_exact_conflict_and_no_duplicate_hint_or_request_ledger() {
    let mut harness = setup();
    let original = TailReportRow {
        project: project(),
        epoch: epoch(1),
        request_ids: vec![request(77)],
        digest: TailDigest::new("ab".repeat(32)).expect("digest"),
    };
    for digest in ["ab", "ab", "cd"] {
        let effects = begin(&mut harness, 1, 1, &digest.repeat(32));
        let trace = drive(&mut harness, 1, effects);
        assert_eq!(
            answer(&trace, 1),
            if digest == "ab" {
                SessionAnswer::TailStored
            } else {
                SessionAnswer::Refused(SessionRefusal::TailConflict {
                    existing: original.digest.clone(),
                })
            }
        );
        assert_eq!(reports(&mut harness), vec![original.clone()]);
        assert_eq!(count(&mut harness, "notice_outbox"), 1);
    }
    assert_eq!(
        harness.call(
            "fleet",
            WireCall::Fleet(FleetCall::StoreTail {
                project: project(),
                epoch: epoch(1),
                request_ids: vec![request(999)],
                digest: original.digest.clone(),
                current_holder: machine(3),
            })
        ),
        WireResult::Fleet(FleetResult::TailReport(SessionAnswer::TailStored))
    );
    assert_eq!(reports(&mut harness), vec![original]);
    assert_eq!(
        count(&mut harness, "notice_outbox"),
        1,
        "same digest never replaces content or owes a second hint"
    );
    assert_eq!(
        count(&mut harness, "fleet_request_ledger"),
        0,
        "automated evidence has no request decision"
    );
    assert_eq!(
        count(&mut harness, "fleet_feed"),
        0,
        "tail evidence is read from its own scope"
    );
    for cursor in [
        ReadCursor::Fleet(FleetPosition::new(0)),
        ReadCursor::Notices(NoticePosition::new(1).expect("notice")),
    ] {
        assert_eq!(
            read(&mut harness, Some(cursor)),
            SessionAnswer::Refused(SessionRefusal::InvalidShape {
                field: "after".into()
            })
        );
    }
}

#[test]
fn full_tail_read_is_project_scoped_and_orders_numeric_epochs() {
    let mut harness = setup();
    for epoch_number in [10, 2, 1] {
        harness.call(
            "fleet",
            WireCall::Fleet(FleetCall::StoreTail {
                project: project(),
                epoch: epoch(epoch_number),
                request_ids: vec![request(epoch_number)],
                digest: TailDigest::new("ab".repeat(32)).expect("digest"),
                current_holder: machine(2),
            }),
        );
    }
    harness.call(
        "fleet",
        WireCall::Fleet(FleetCall::StoreTail {
            project: ProjectId::new(identity(999)).expect("other"),
            epoch: epoch(5),
            request_ids: vec![],
            digest: TailDigest::new("ab".repeat(32)).expect("digest"),
            current_holder: machine(2),
        }),
    );
    let rows = reports(&mut harness);
    assert_eq!(
        rows.iter().map(|row| row.epoch).collect::<Vec<_>>(),
        vec![epoch(1), epoch(2), epoch(10)]
    );
    assert!(rows.iter().all(|row| row.project == project()));
}

#[test]
fn holder_change_after_verification_cannot_strand_the_durable_report() {
    let mut harness = setup();
    let WireEffect::Dispatch {
        call_id,
        call,
        carry,
        ..
    } = begin(&mut harness, 1, 1, &"ab".repeat(32))
        .into_iter()
        .next()
        .expect("dispatch")
    else {
        panic!("verification");
    };
    let verified = harness.call("registry", call);
    assert_eq!(
        verified,
        WireResult::Registry(RegistryResult::TailVerification(
            TailVerificationResult::Verified {
                current_holder: machine(2)
            }
        ))
    );
    harness.enroll(3);
    harness.register(3, 10);
    let preview = harness.preview();
    assert!(
        matches!(harness.call("registry",admin(AdminRequest::Takeover {transfer_id:transfer(99),project:project(),to_installation:machine(3),reason:"successor".into(),confirm_token:preview.confirm_token})),WireResult::Registry(RegistryResult::Admin(AdminResult::Granted {new_epoch,..})) if new_epoch==epoch(3))
    );
    let effects = harness.run(
        &address(1),
        WireInput::CallResult {
            source: CallerKind::Session,
            call_id,
            result: Box::new(verified),
            carry,
        },
    );
    assert!(
        matches!(effects.as_slice(),[WireEffect::Dispatch {call:WireCall::Fleet(FleetCall::StoreTail {current_holder,..}),..}] if *current_holder==machine(2))
    );
    let trace = drive(&mut harness, 1, effects);
    assert_eq!(answer(&trace, 1), SessionAnswer::TailStored);
    let result = harness.call(
        "fleet",
        WireCall::Fleet(FleetCall::Read {
            installation: machine(3),
            scope: ReadScope::TailReports { project: project() },
            after: None,
        }),
    );
    assert!(
        matches!(result,WireResult::Fleet(FleetResult::Read(SessionAnswer::ReadPage(ReadPage::TailReports {reports,..}))) if reports.len()==1 && reports[0].epoch==epoch(1))
    );
    assert_eq!(
        count(&mut harness, "notice_outbox"),
        1,
        "captured old holder's hint is independent of successor reads"
    );
}

#[test]
fn replaced_or_expired_session_never_stores_a_late_verification_result() {
    for replaced in [true, false] {
        let mut harness = setup();
        let effects = begin(&mut harness, 1, 1, &"ab".repeat(32));
        let WireEffect::Dispatch {
            call_id,
            call,
            carry,
            ..
        } = effects.into_iter().next().expect("dispatch")
        else {
            panic!("verification");
        };
        let verified = harness.call("registry", call);
        if replaced {
            let effects = harness.run(
                &address(1),
                WireInput::Session(SessionCall::Hello {
                    connection: connection(99),
                    credential: CredentialStatus::Machine {
                        installation: machine(1),
                        expires_at: at(1440).0,
                    },
                    installation: machine(1),
                    session_id: session(99),
                    schema_support: SchemaSupport { max_version: 10 },
                }),
            );
            drive(&mut harness, 1, effects);
        } else {
            harness.now = at(1441);
        }
        assert!(
            harness
                .run(
                    &address(1),
                    WireInput::CallResult {
                        source: CallerKind::Session,
                        call_id,
                        result: Box::new(verified),
                        carry
                    }
                )
                .is_empty()
        );
        assert!(reports(&mut harness).is_empty());
        assert_eq!(count(&mut harness, "notice_outbox"), 0);
    }
}

#[test]
fn a_late_hint_failure_rolls_back_tail_and_corrupt_stored_evidence_is_refused() {
    let mut harness = setup();
    let store = harness.store("fleet");
    store.connection.execute_batch("CREATE TRIGGER fail_tail_hint BEFORE INSERT ON notice_outbox BEGIN SELECT RAISE(ABORT,'injected tail hint failure'); END;").expect("late fault");
    let call = WireCall::Fleet(FleetCall::StoreTail {
        project: project(),
        epoch: epoch(1),
        request_ids: vec![request(77)],
        digest: TailDigest::new("ab".repeat(32)).expect("digest"),
        current_holder: machine(2),
    });
    let result = store.run(
        WireInput::Call {
            from: ObjectAddress::Connection(machine(1)),
            call_id: ObjectCallId::new(identity(888)).expect("call"),
            call: Box::new(call.clone()),
        },
        start(),
    );
    assert!(matches!(
        result,
        Err(CentralError::Store(StoreError::Constraint { .. }))
    ));
    assert_eq!(count(&mut harness, "tail_reports"), 0);
    assert_eq!(count(&mut harness, "notice_outbox"), 0);
    harness
        .store("fleet")
        .connection
        .execute_batch("DROP TRIGGER fail_tail_hint")
        .expect("restore");
    assert_eq!(
        harness.call("fleet", call),
        WireResult::Fleet(FleetResult::TailReport(SessionAnswer::TailStored))
    );
    for (column, value) in [
        ("digest", "bad"),
        ("request_ids", "[\"bad\"]"),
        ("epoch", "01"),
    ] {
        let tx = harness.store("fleet");
        tx.connection
            .execute_batch("SAVEPOINT corrupt")
            .expect("save");
        tx.connection
            .execute(&format!("UPDATE tail_reports SET {column}=?"), [value])
            .expect("fault");
        let input = WireInput::Call {
            from: ObjectAddress::Connection(machine(2)),
            call_id: ObjectCallId::new(identity(889)).expect("call"),
            call: Box::new(WireCall::Fleet(FleetCall::Read {
                installation: machine(2),
                scope: ReadScope::TailReports { project: project() },
                after: None,
            })),
        };
        // The savepoint belongs to this diagnostic transaction; the real core
        // decoder must fail before a page can claim to contain valid evidence.
        assert!(matches!(
            step(tx, input, start(), IdBatch::default()),
            Err(CentralError::Store(StoreError::Corrupt { .. }))
        ));
        tx.connection
            .execute_batch("ROLLBACK TO corrupt; RELEASE corrupt")
            .expect("restore");
    }
}
