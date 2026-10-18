#![allow(clippy::expect_used, clippy::panic)]
mod registry_support;
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
use registry_support::*;
fn address() -> String {
    Harness::target(ObjectAddress::Connection(machine(1)))
}
fn hello(cid: u64, sid: u64) -> WireInput {
    WireInput::Session(SessionCall::Hello {
        connection: connection(cid),
        credential: CredentialStatus::Machine {
            installation: machine(1),
            expires_at: at(60).0,
        },
        installation: machine(1),
        session_id: session(sid),
        schema_support: SchemaSupport { max_version: 10 },
    })
}
fn drive(h: &mut Harness, input: WireInput) -> Vec<WireEffect> {
    let mut next = h.run(&address(), input);
    let mut trace = next.clone();
    loop {
        let dispatches: Vec<_> = next
            .into_iter()
            .filter_map(|e| match e {
                WireEffect::Dispatch {
                    call_id,
                    to,
                    call,
                    carry,
                } => Some((call_id, to, call, carry)),
                _ => None,
            })
            .collect();
        if dispatches.is_empty() {
            break;
        }
        next = Vec::new();
        for (call_id, to, call, carry) in dispatches {
            let result = h.call(&Harness::target(to), call);
            let effects = h.run(
                &address(),
                WireInput::CallResult {
                    source: CallerKind::Session,
                    carry,
                    call_id,
                    result: Box::new(result),
                },
            );
            trace.extend(effects.clone());
            next.extend(effects);
        }
    }
    trace
}
fn frame(cid: u64) -> WireInput {
    WireInput::Session(SessionCall::Frame {
        connection: connection(cid),
        frame_id: FrameId::new(identity(99)).expect("frame"),
        request: SessionRequest::Renew {
            attempt: request(1),
            project: project(),
            epoch: epoch(1),
        },
    })
}
#[test]
fn hello_welcome_reconstructs_ack_and_current_lease_from_separate_objects() {
    let mut h = Harness::setup();
    h.schema_piece(1);
    let effects = drive(&mut h, hello(1, 1));
    assert!(effects.iter().any(|e|matches!(e,WireEffect::Send{to,frame:CentralFrame::Welcome{session_id,projects}}if to==&connection(1)&&session_id==&session(1)&&projects.len()==1&&projects.first().is_some_and(|p|p.acked==position(1,1)&&p.lease.holder==machine(1)))));
}
#[test]
fn replacement_targets_old_socket_and_drops_its_inflight_answer_then_resend_replays() {
    let mut h = Harness::setup();
    drive(&mut h, hello(1, 1));
    let old = h.run(&address(), frame(1));
    let (call_id, call) = old
        .into_iter()
        .find_map(|e| match e {
            WireEffect::Dispatch { call_id, call, .. } => Some((call_id, call)),
            _ => None,
        })
        .expect("renew dispatch");
    let result = h.call("registry", call);
    let effects = drive(&mut h, hello(2, 1));
    assert!(effects.iter().any(|e|matches!(e,WireEffect::Send{to,frame:CentralFrame::Closed{reason:SessionCloseReason::ConnectionReplaced}}if to==&connection(1))));
    assert!(
        h.run(
            &address(),
            WireInput::CallResult {
                source: CallerKind::Session,
                carry: None,
                call_id,
                result: Box::new(result)
            }
        )
        .is_empty()
    );
    h.now = at(15);
    let effects = drive(&mut h, frame(2));
    assert!(effects.iter().any(|e|matches!(e,WireEffect::Send{to,frame:CentralFrame::Answer{body:SessionAnswer::Renewed{replayed:true,expires_at,..},..}}if to==&connection(2)&&expires_at==&at(240).0)));
}
#[test]
fn hello_rejects_unknown_expired_revoked_and_non_machine_credentials() {
    for reason in [
        CredentialRejection::Unknown,
        CredentialRejection::Expired,
        CredentialRejection::Revoked,
        CredentialRejection::NotMachine,
    ] {
        let mut h = Harness::setup();
        let mut input = hello(1, 1);
        if let WireInput::Session(SessionCall::Hello { credential, .. }) = &mut input {
            *credential = CredentialStatus::Rejected(reason.clone());
        }
        let effects = drive(&mut h, input);
        assert_eq!(
            effects,
            vec![WireEffect::Send {
                to: connection(1),
                frame: CentralFrame::Closed {
                    reason: SessionCloseReason::CredentialRejected(reason)
                }
            }]
        );
    }
}
#[test]
fn expiry_mid_session_closes_without_dispatching_request() {
    let mut h = Harness::setup();
    drive(&mut h, hello(1, 1));
    h.now = at(60);
    let effects = h.run(&address(), frame(1));
    assert!(effects.iter().any(|e| matches!(
        e,
        WireEffect::Send {
            frame: CentralFrame::Closed {
                reason: SessionCloseReason::CredentialRejected(CredentialRejection::Expired)
            },
            ..
        }
    )));
    assert!(!effects.iter().any(|e| matches!(
        e,
        WireEffect::Dispatch {
            call: WireCall::Registry(RegistryCall::Renew { .. }),
            ..
        }
    )));
}

#[test]
fn decode_failures_preserve_connection_and_expiry_guards_without_request_dispatch() {
    let mut h = Harness::setup();
    drive(&mut h, hello(1, 1));
    let pending = h.run(&address(), frame(1));
    assert!(
        matches!(
            pending.as_slice(),
            [WireEffect::Dispatch {
                call: WireCall::Registry(RegistryCall::Renew { .. }),
                ..
            }]
        ),
        "retain a legitimate unanswered renewal while rejecting the malformed frame"
    );
    let before = session_ledger_rows(&mut h);
    assert_eq!(before["_central_continuations"].len(), 1);
    let failure = MachineFrameDecodeFailure::InvalidRequest {
        frame_id: FrameId::new(identity(99)).expect("frame"),
        field: "body".into(),
    };
    let malformed = |cid| {
        WireInput::Session(SessionCall::DecodeFailure {
            connection: connection(cid),
            failure: failure.clone(),
        })
    };
    let effects = h.run(&address(), malformed(1));
    assert!(
        matches!(effects.as_slice(), [WireEffect::Send { to, frame: CentralFrame::Answer { frame_id, body: SessionAnswer::Refused(SessionRefusal::InvalidShape { field }) } }] if *to == connection(1) && frame_id.as_str() == identity(99) && field == "body")
    );
    assert_eq!(
        session_ledger_rows(&mut h),
        before,
        "malformed request must not insert, overwrite or clear any continuation or forward-ledger row"
    );
    drive(&mut h, hello(2, 1));
    assert!(
        h.run(&address(), malformed(1)).is_empty(),
        "superseded connection cannot answer"
    );
    h.now = at(60);
    let expired = h.run(&address(), malformed(2));
    assert!(expired.iter().any(|effect| matches!(
        effect,
        WireEffect::Send {
            frame: CentralFrame::Closed {
                reason: SessionCloseReason::CredentialRejected(CredentialRejection::Expired)
            },
            ..
        }
    )));
    assert!(
        !expired.iter().any(|effect| matches!(
            effect,
            WireEffect::Send {
                frame: CentralFrame::Answer { .. },
                ..
            }
        )),
        "expiry wins over malformed-body refusal"
    );
}

fn session_ledger_rows(h: &mut Harness) -> std::collections::BTreeMap<String, Vec<Vec<String>>> {
    [
        (
            "_central_continuations",
            "SELECT call_id,record FROM _central_continuations ORDER BY call_id",
        ),
        (
            "pending_forwards",
            "SELECT forward_id,call_id,record FROM pending_forwards ORDER BY forward_id",
        ),
        (
            "delivered_forwards",
            "SELECT forward_id,record FROM delivered_forwards ORDER BY forward_id",
        ),
    ]
    .into_iter()
    .map(|(table, sql)| {
        let mut query = h
            .store(&address())
            .connection
            .prepare(sql)
            .expect("ledger query");
        let columns = query.column_count();
        let rows = query
            .query_map([], |row| {
                (0..columns)
                    .map(|index| row.get::<_, String>(index))
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .expect("read ledger")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("exact ledger rows");
        (table.to_owned(), rows)
    })
    .collect()
}
#[test]
fn revoke_closes_current_session_and_authentication_stays_revoked() {
    let mut h = Harness::setup();
    drive(&mut h, hello(1, 1));
    h.call(
        "registry",
        admin(AdminRequest::RevokeMachine {
            installation: machine(1),
        }),
    );
    assert_eq!(
        h.call(
            "registry",
            WireCall::Registry(RegistryCall::AuthenticateMachine {
                credential_hash: "hash-1".into(),
                access_client_id: AccessClientId::new("client-1").expect("client")
            })
        ),
        WireResult::Registry(RegistryResult::Authenticated(CredentialStatus::Rejected(
            CredentialRejection::Revoked
        )))
    );
    assert!(h.run(&address(), frame(1)).is_empty());
}

#[test]
fn direct_copy_unavailability_answers_the_original_frame_without_closing_the_session() {
    for reason in [
        CopyUnavailableReason::StorageFull,
        CopyUnavailableReason::MigrationPending {
            kind: BookkeepingKind::Copy,
            version: 3,
        },
    ] {
        let mut h = Harness::setup();
        drive(&mut h, hello(1, 1));
        let piece = Piece::new(PieceContents {
            project: project(),
            epoch: epoch(1),
            continues_from: position(1, 0),
            through: position(1, 1),
            transactions: vec![ReplicatedTransaction::SchemaStep {
                commit_id: 1.into(),
                migration_version: 1,
                statements: vec!["CREATE TABLE records(id INTEGER PRIMARY KEY)".into()],
            }],
            part: None,
            summary: Some(ProjectSummary {
                status: "active".into(),
                latest_record_at: start().0,
            }),
        })
        .expect("valid piece");
        for request in [
            SessionRequest::Piece(piece),
            SessionRequest::Read {
                scope: ReadScope::ProjectCopy { project: project() },
                after: None,
            },
        ] {
            let frame_id = FrameId::new(identity(99)).expect("frame");
            let dispatch = h.run(
                &address(),
                WireInput::Session(SessionCall::Frame {
                    connection: connection(1),
                    frame_id: frame_id.clone(),
                    request,
                }),
            );
            let call_id = match dispatch.as_slice() {
                [
                    WireEffect::Dispatch {
                        call_id,
                        to: ObjectAddress::Copy(target),
                        ..
                    },
                ] => {
                    assert_eq!(target, &project());
                    call_id.clone()
                }
                _ => panic!("copy dispatch"),
            };
            // The copy's failure production is proved separately; this checks its session consumer.
            let input = WireInput::CallResult {
                source: CallerKind::Session,
                carry: None,
                call_id,
                result: Box::new(WireResult::Copy(CopyResult::Unavailable(reason.clone()))),
            };
            let effects = h.run(&address(), input.clone());
            let refusal = if reason == CopyUnavailableReason::StorageFull {
                SessionRefusal::CentralCopyFull
            } else {
                SessionRefusal::CopyUnavailable {
                    project: project(),
                    reason: reason.clone(),
                }
            };
            assert_eq!(
                effects,
                vec![WireEffect::Send {
                    to: connection(1),
                    frame: CentralFrame::Answer {
                        frame_id,
                        body: SessionAnswer::Refused(refusal),
                    },
                }]
            );
            assert!(h.run(&address(), input).is_empty(), "no duplicate answer");
        }
    }
}

#[test]
fn fleet_creation_reports_unavailable_initial_lease_instead_of_its_prior_acceptance() {
    let mut h = Harness::setup();
    drive(&mut h, hello(1, 1));
    let frame_id = FrameId::new(identity(99)).expect("frame");
    let effects = h.run(
        &address(),
        WireInput::Session(SessionCall::Frame {
            connection: connection(1),
            frame_id: frame_id.clone(),
            request: SessionRequest::Fleet(FleetRequestEnvelope {
                request_id: request(90),
                operation: serde_json::json!({"fixture":"fleet-result consumer"}).into(),
            }),
        }),
    );
    let dispatch_id = |effects: &[WireEffect]| {
        effects
            .iter()
            .find_map(|effect| match effect {
                WireEffect::Dispatch { call_id, .. } => Some(call_id.clone()),
                _ => None,
            })
            .expect("dispatch")
    };
    let effects = h.run(
        &address(),
        WireInput::CallResult {
            source: CallerKind::Session,
            carry: None,
            call_id: dispatch_id(&effects),
            result: Box::new(WireResult::Fleet(FleetResult::ProjectAccepted {
                project: project(),
                answer: SessionAnswer::FleetDecision {
                    request_id: request(90),
                    decision: serde_json::json!({"accepted":true}).into(),
                },
            })),
        },
    );
    assert!(matches!(
        effects.as_slice(),
        [WireEffect::Dispatch {
            call: WireCall::Registry(RegistryCall::ReserveFirstEpoch { .. }),
            ..
        }]
    ));
    let unavailable = SessionAnswer::Unavailable {
        subject: BlockedSubject::Project { project: project() },
        reason: CopyUnavailableReason::StorageFull,
    };
    let effects = h.run(
        &address(),
        WireInput::CallResult {
            source: CallerKind::Session,
            carry: None,
            call_id: dispatch_id(&effects),
            result: Box::new(WireResult::Registry(RegistryResult::FirstLease(
                unavailable.clone(),
            ))),
        },
    );
    assert_eq!(
        effects,
        vec![WireEffect::Send {
            to: connection(1),
            frame: CentralFrame::Answer {
                frame_id,
                body: unavailable
            },
        }]
    );
}

#[test]
fn every_request_kind_dispatches_typed_result_and_answers_exact_frame_id_once() {
    let mut h = Harness::setup();
    drive(&mut h, hello(1, 1));
    let id = FrameId::new(identity(99)).expect("frame");
    let requests = vec![
        SessionRequest::ReserveFirstEpoch { project: project() },
        SessionRequest::Release {
            project: project(),
            epoch: epoch(1),
            transfer: transfer(1),
            outcome: ReleaseOutcome::Final(position(1, 0)),
        },
        SessionRequest::TailReport {
            project: project(),
            epoch: epoch(1),
            request_ids: vec![request(1)],
            digest: TailDigest::new("ab".repeat(32)).expect("tail digest"),
        },
        SessionRequest::Fleet(FleetRequestEnvelope {
            request_id: request(1),
            operation: serde_json::json!({"example":"fleet stand-in"}).into(),
        }),
        SessionRequest::Read {
            scope: ReadScope::FleetRecords,
            after: None,
        },
        SessionRequest::Read {
            scope: ReadScope::ProjectCopy { project: project() },
            after: None,
        },
        SessionRequest::ArchiveSeen {
            board: BoardId::new(identity(98)).expect("board"),
            project: project(),
            position: position(1, 0),
        },
    ];
    for request in requests {
        let effects = h.run(
            &address(),
            WireInput::Session(SessionCall::Frame {
                connection: connection(1),
                frame_id: id.clone(),
                request,
            }),
        );
        assert_eq!(effects.len(), 1);
        let (call_id, call) = effects
            .into_iter()
            .find_map(|e| match e {
                WireEffect::Dispatch { call_id, call, .. } => Some((call_id, call)),
                _ => None,
            })
            .expect("dispatch");
        let answer = SessionAnswer::Refused(SessionRefusal::NotInitialized);
        let result = match call {
            WireCall::Registry(RegistryCall::ReserveFirstEpoch { .. }) => {
                WireResult::Registry(RegistryResult::FirstLease(answer))
            }
            WireCall::Registry(RegistryCall::Release { .. }) => {
                WireResult::Registry(RegistryResult::Release(answer))
            }
            WireCall::Registry(RegistryCall::VerifyTail { .. }) => WireResult::Registry(
                RegistryResult::TailVerification(TailVerificationResult::Refused(
                    TailVerificationRefusal::NotWriter { current: None },
                )),
            ),
            WireCall::Fleet(FleetCall::Fleet { .. }) => {
                WireResult::Fleet(FleetResult::Fleet(answer))
            }
            WireCall::Fleet(FleetCall::Read { .. }) => WireResult::Fleet(FleetResult::Read(answer)),
            WireCall::Copy(CopyCall::SnapshotPage { .. }) => {
                WireResult::Copy(CopyResult::SnapshotPage(answer))
            }
            WireCall::Fleet(FleetCall::ArchiveSeen { .. }) => {
                WireResult::Fleet(FleetResult::ArchiveSeen(answer))
            }
            _ => panic!("wrong dispatch"),
        };
        let reply = h.run(
            &address(),
            WireInput::CallResult {
                source: CallerKind::Session,
                carry: None,
                call_id: call_id.clone(),
                result: Box::new(result.clone()),
            },
        );
        assert_eq!(reply.len(), 1);
        assert!(
            matches!(reply.first(),Some(WireEffect::Send{to,frame:CentralFrame::Answer{frame_id,..}})if to==&connection(1)&&frame_id==&id)
        );
        assert!(
            h.run(
                &address(),
                WireInput::CallResult {
                    source: CallerKind::Session,
                    carry: None,
                    call_id,
                    result: Box::new(result)
                }
            )
            .is_empty()
        );
    }
}
