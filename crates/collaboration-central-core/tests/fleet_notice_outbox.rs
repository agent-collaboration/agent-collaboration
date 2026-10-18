#![allow(clippy::expect_used, clippy::panic)]
mod archive_support;
mod registry_support;
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
use registry_support::*;

#[test]
fn fleet_mutation_only_owes_a_hint_then_alarm_retries_and_correlates_real_delivery_ack() {
    let mut fixture = archive_support::ArchiveFixture::new();
    assert!(matches!(
        fixture.call(
            FleetCall::Read {
                installation: machine(1),
                scope: ReadScope::FleetRecords,
                after: None
            },
            0
        ),
        FleetResult::Read(SessionAnswer::ReadPage(ReadPage::FleetRecords { .. }))
    ));
    fixture.archive_workspace();
    let mut harness = Harness::new();
    harness.now = at(1);
    harness.stores.insert("fleet".into(), fixture.store);
    let first = offered(harness.run(
        "fleet",
        WireInput::Alarm {
            face: CallerKind::Fleet,
        },
    ));
    assert_eq!(first.1, ObjectAddress::Connection(machine(1)));
    assert_eq!(
        first.2,
        WireCall::Delivery(DeliveryCall::DeliverNotice(NoticeKind::FleetFeedAdvanced {
            through: FleetPosition::new(7)
        }))
    );
    harness.run(
        "fleet",
        WireInput::CallFailed {
            source: CallerKind::Fleet,
            call_id: first.0.clone(),
            carry: None,
        },
    );
    assert_eq!(
        owed(&mut harness),
        1,
        "lost dispatch retains its durable debt"
    );
    harness.now = RegistryTime(at(1).0 + chrono::TimeDelta::seconds(4));
    assert!(
        !harness
            .run(
                "fleet",
                WireInput::Alarm {
                    face: CallerKind::Fleet
                }
            )
            .iter()
            .any(|effect| matches!(effect, WireEffect::Dispatch { .. })),
        "retry waits for the stored deadline"
    );
    harness.now = RegistryTime(at(1).0 + chrono::TimeDelta::seconds(5));
    let retry = offered(harness.run(
        "fleet",
        WireInput::Alarm {
            face: CallerKind::Fleet,
        },
    ));
    assert_ne!(first.0, retry.0, "a retry has fresh RPC correlation");
    assert_eq!(first.1, retry.1);
    assert_eq!(first.2, retry.2);
    // The real delivery owner commits a numbered offline notice. Its answer
    // is deliberately lost until the replacement offer has been installed.
    let result = harness.call(&Harness::target(first.1), first.2);
    acknowledge(&mut harness, first.0, result.clone());
    assert_eq!(
        owed(&mut harness),
        1,
        "obsolete ACK cannot settle newer offer"
    );
    let actual = harness.call(&Harness::target(retry.1), retry.2);
    assert_eq!(
        actual,
        WireResult::Delivery(DeliveryResult::NoticeDelivered)
    );
    acknowledge(&mut harness, retry.0.clone(), actual);
    assert_eq!(owed(&mut harness), 0);
    acknowledge(&mut harness, retry.0, result);
    assert_eq!(owed(&mut harness), 0, "duplicate ACK is inert");
    let retained: i64 = harness
        .store(&Harness::target(ObjectAddress::Connection(machine(1))))
        .connection
        .query_row("SELECT count(*) FROM _session_notices", [], |row| {
            row.get(0)
        })
        .expect("actual recipient notices");
    assert_eq!(
        retained, 2,
        "at-least-once hints are durably numbered, never fabricated receipts"
    );
}

fn offered(effects: Vec<WireEffect>) -> (ObjectCallId, ObjectAddress, WireCall) {
    let offers: Vec<_> = effects
        .into_iter()
        .filter_map(|effect| match effect {
            WireEffect::Dispatch {
                call_id,
                to,
                call,
                carry: None,
            } => Some((call_id, to, call)),
            _ => None,
        })
        .collect();
    assert_eq!(
        offers.len(),
        1,
        "Fleet's separate alarm must deliver its durable owed hint"
    );
    offers.into_iter().next().expect("offer")
}
fn acknowledge(harness: &mut Harness, call_id: ObjectCallId, result: WireResult) {
    let effects = harness.run(
        "fleet",
        WireInput::CallResult {
            source: CallerKind::Fleet,
            call_id,
            result: Box::new(result),
            carry: None,
        },
    );
    assert!(
        !effects.iter().any(|effect| matches!(
            effect,
            WireEffect::Dispatch { .. } | WireEffect::Notify { .. }
        )),
        "an inbound ACK never dispatches; every Fleet→delivery edge is from a later alarm"
    );
}
fn owed(harness: &mut Harness) -> i64 {
    harness
        .store("fleet")
        .connection
        .query_row("SELECT count(*) FROM notice_outbox", [], |row| row.get(0))
        .expect("durable outbox")
}

#[test]
fn a_later_alarm_drains_more_readers_than_one_shell_id_batch_without_losing_debt() {
    let mut fixture = archive_support::ArchiveFixture::new();
    for reader in 1..=12 {
        fixture.call(
            FleetCall::Read {
                installation: machine(reader),
                scope: ReadScope::FleetRecords,
                after: None,
            },
            0,
        );
    }
    fixture.archive_workspace();
    let mut harness = Harness::new();
    harness.now = at(1);
    harness.stores.insert("fleet".into(), fixture.store);
    assert_eq!(owed(&mut harness), 12);
    for count in [8, 4] {
        let offers: Vec<_> = harness
            .run(
                "fleet",
                WireInput::Alarm {
                    face: CallerKind::Fleet,
                },
            )
            .into_iter()
            .filter_map(|effect| match effect {
                WireEffect::Dispatch {
                    call_id,
                    to,
                    call,
                    carry: None,
                } => Some((call_id, to, call)),
                _ => None,
            })
            .collect();
        assert_eq!(offers.len(), count);
        for (id, to, call) in offers {
            let actual = harness.call(&Harness::target(to), call);
            acknowledge(&mut harness, id, actual);
        }
    }
    assert_eq!(owed(&mut harness), 0);
}

#[test]
fn workspace_archive_owes_one_board_hint_to_each_known_holder() {
    let mut fixture = archive_support::ArchiveFixture::new();
    for (index, holder) in [1, 2].into_iter().enumerate() {
        let project = ProjectId::new(fixture.projects.get(index).expect("project").as_str())
            .expect("project");
        let result = fixture.call(
            FleetCall::DirectoryUpdate(
                DirectoryUpdate::lease(
                    project,
                    DirectoryLease::Held {
                        holder: machine(holder),
                        state: LeaseState::Held,
                        expires_at: at(240).0,
                    },
                    LeaseVersion::new(
                        epoch(1),
                        RegistryIncarnation::new(1).expect("incarnation"),
                        1,
                    )
                    .expect("version"),
                    start(),
                )
                .expect("update"),
            ),
            0,
        );
        assert!(matches!(
            result,
            FleetResult::LeaseUpdated(LeaseUpdateResult::Applied)
        ));
    }
    fixture.archive_workspace();
    let expected = NoticeKind::BoardArchiving {
        board: BoardId::new(fixture.board.as_str()).expect("board"),
    };
    let mut harness = Harness::new();
    harness.now = at(1);
    harness.stores.insert("fleet".into(), fixture.store);
    let mut holders = Vec::new();
    for effect in harness.run(
        "fleet",
        WireInput::Alarm {
            face: CallerKind::Fleet,
        },
    ) {
        if let WireEffect::Dispatch {
            to: ObjectAddress::Connection(holder),
            call: WireCall::Delivery(DeliveryCall::DeliverNotice(kind)),
            ..
        } = effect
        {
            assert_eq!(kind, expected);
            holders.push(holder);
        }
    }
    holders.sort();
    assert_eq!(holders, vec![machine(1), machine(2)]);
}

#[test]
fn a_late_outbox_failure_rolls_back_records_feed_ledger_and_high_water_together() {
    use fleet_admission::{FleetOperation, FleetRequest};
    use message_board::{ProtocolVersion, RequestId};
    let mut fixture = archive_support::ArchiveFixture::new();
    fixture.call(
        FleetCall::Read {
            installation: machine(1),
            scope: ReadScope::FleetRecords,
            after: None,
        },
        0,
    );
    let before = (
        fixture.board_record(),
        fixture.record("workspace", fixture.workspace.as_str()),
        fixture.feed(),
        fixture.ledger(),
    );
    fixture.store.connection.execute_batch("CREATE TRIGGER fail_owed_hint BEFORE INSERT ON notice_outbox BEGIN SELECT RAISE(ABORT,'injected owed hint failure'); END;").expect("late transaction fault");
    let request = FleetRequest {
        request_id: RequestId::try_from(identity(990_001)).expect("request"),
        workspace: fixture.workspace.clone(),
        actor: fixture.actor.clone(),
        acting_for: None,
        resubmits: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation: FleetOperation::WorkspaceArchive {},
    };
    let result = fixture.store.run(
        WireInput::Call {
            from: ObjectAddress::Connection(machine(1)),
            call_id: ObjectCallId::new(identity(990_002)).expect("call"),
            call: Box::new(WireCall::Fleet(FleetCall::Fleet {
                installation: machine(1),
                request: FleetRequestEnvelope {
                    request_id: collaboration_replication_protocol::RequestId::new(
                        request.request_id.as_str(),
                    )
                    .expect("request"),
                    operation: FleetPayload::new(serde_json::to_value(request).expect("request")),
                },
            })),
        },
        at(1),
    );
    assert!(matches!(
        result,
        Err(CentralError::Store(StoreError::Constraint { .. }))
    ));
    assert_eq!(
        (
            fixture.board_record(),
            fixture.record("workspace", fixture.workspace.as_str()),
            fixture.feed(),
            fixture.ledger()
        ),
        before
    );
    let head: i64 = fixture
        .store
        .connection
        .query_row(
            "SELECT last_position FROM fleet_checkpoint WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .expect("high water");
    assert_eq!(head, 4);
    let owed: i64 = fixture
        .store
        .connection
        .query_row("SELECT count(*) FROM notice_outbox", [], |row| row.get(0))
        .expect("outbox count");
    assert_eq!(owed, 0);
}
