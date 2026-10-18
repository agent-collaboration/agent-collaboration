#![allow(clippy::expect_used, clippy::panic)]
mod registry_support;

use chrono::TimeDelta;
use collaboration_central_core::*;
use collaboration_replication_protocol::{
    FleetPayload, FleetRequestEnvelope, ReadCursor, ReadPage, ReadScope, SessionAnswer,
    SessionRefusal,
};
use fleet_admission::*;
use message_board::*;
use registry_support::{identity, machine, start};

struct FeedFixture {
    store: registry_support::sql_support::SqlTxFake,
    workspace: WorkspaceId,
    actor: RecordedActor,
}

impl FeedFixture {
    fn imported(boards: u64) -> Self {
        let workspace = WorkspaceId::try_from(identity(50_000)).expect("workspace");
        let actor = RecordedActor {
            actor: ActorKind::Declared(Identity::Human {
                human_id: HumanId::try_from("feed owner".to_owned()).expect("owner"),
            }),
            installation: ServiceId::try_from(machine(1).as_str().to_owned())
                .expect("installation"),
            machine: MachineLabel::try_from("feed source".to_owned()).expect("machine"),
        };
        let mut fixture = Self {
            store: registry_support::sql_support::SqlTxFake::blank(),
            workspace: workspace.clone(),
            actor: actor.clone(),
        };
        let original_time = start().0 - TimeDelta::days(3650);
        let mut records = vec![FleetImportRecord::Workspace {
            id: workspace.clone(),
            name: ResourceName::try_from("Feed workspace".to_owned()).expect("name"),
            description: Description::try_from("Feed records".to_owned()).expect("description"),
            repositories: vec![],
            created_at: original_time,
        }];
        records.extend((0..boards).map(|index| FleetImportRecord::Board {
            id: BoardId::try_from(identity(51_000 + index)).expect("board"),
            workspace: workspace.clone(),
            name: ResourceName::try_from(format!("Feed board {index}")).expect("name"),
            description: Description::try_from("Feed board".to_owned()).expect("description"),
            state: ImportedBoardState::Active,
            created_at: original_time,
        }));
        assert!(matches!(
            fixture.call(
                FleetCall::Import {
                    bundle: Box::new(FleetImportBundle {
                        provenance: ImportProvenance {
                            installation: actor.installation,
                            machine: actor.machine,
                        },
                        records,
                    }),
                },
                start(),
            ),
            FleetResult::Imported(FleetImportResult::Imported { .. })
        ));
        fixture
    }

    fn call(&mut self, call: FleetCall, now: RegistryTime) -> FleetResult {
        self.store
            .connection
            .execute_batch("BEGIN IMMEDIATE")
            .expect("begin");
        let result = step(
            &mut self.store,
            WireInput::Call {
                from: ObjectAddress::Connection(machine(1)),
                call_id: ObjectCallId::new(identity(59_000)).expect("call"),
                call: Box::new(WireCall::Fleet(call)),
            },
            now,
            IdBatch::default(),
        );
        self.store
            .connection
            .execute_batch(if result.is_ok() { "COMMIT" } else { "ROLLBACK" })
            .expect("finish");
        let effects = result
            .expect("real Fleet step with no IDs supplied")
            .into_trace();
        effects
            .into_iter()
            .find_map(|effect| match effect {
                WireEffect::Return {
                    result: WireResult::Fleet(result),
                    ..
                } => Some(result),
                _ => None,
            })
            .expect("Fleet answer")
    }

    fn read(&mut self, after: Option<ReadCursor>, now: RegistryTime) -> SessionAnswer {
        match self.call(
            FleetCall::Read {
                installation: machine(1),
                scope: ReadScope::FleetRecords,
                after,
            },
            now,
        ) {
            FleetResult::Read(answer) => answer,
            _ => panic!("read result"),
        }
    }

    fn archive(&mut self, now: RegistryTime) {
        let request = FleetRequest {
            request_id: RequestId::try_from(identity(59_001)).expect("request"),
            workspace: self.workspace.clone(),
            actor: self.actor.clone(),
            acting_for: None,
            resubmits: None,
            protocol_version: ProtocolVersion::CURRENT,
            operation: FleetOperation::WorkspaceArchive {},
        };
        let FleetResult::Fleet(SessionAnswer::FleetDecision { decision, .. }) = self.call(
            FleetCall::Fleet {
                installation: machine(1),
                request: FleetRequestEnvelope {
                    request_id: collaboration_replication_protocol::RequestId::new(
                        request.request_id.as_str(),
                    )
                    .expect("request"),
                    operation: FleetPayload::new(serde_json::to_value(request).expect("request")),
                },
            },
            now,
        ) else {
            panic!("archive answer");
        };
        assert!(matches!(
            serde_json::from_value::<FleetRequestResult>(decision.as_value().clone())
                .expect("decision"),
            FleetRequestResult::Decided(RequestDecision::Accepted { .. })
        ));
    }
}

fn after(position: u64) -> Option<ReadCursor> {
    Some(ReadCursor::Fleet(position.into()))
}

fn page(answer: SessionAnswer, expected_position: u64, now: RegistryTime) -> Vec<Record> {
    let SessionAnswer::ReadPage(ReadPage::FleetRecords {
        records,
        position,
        as_of,
    }) = answer
    else {
        panic!("Fleet records page: {answer:?}");
    };
    assert_eq!(position.value(), expected_position);
    assert_eq!(as_of, now.0);
    records
        .into_iter()
        .map(|record| serde_json::from_value(record.as_value().clone()).expect("record"))
        .collect()
}

#[test]
fn arbitrary_import_and_workspace_archive_have_contiguous_positions_without_id_stock() {
    // Both appends exceed the actual shells' stock of eight call IDs.
    const BOARDS: u64 = 24;
    let mut fixture = FeedFixture::imported(BOARDS);
    let imported = page(fixture.read(after(0), start()), BOARDS + 1, start());
    assert_eq!(imported.len(), 25);
    assert!(imported.iter().enumerate().all(|(index, record)| {
        record.envelope().position
            == RecordPosition::Fleet(FleetPosition::try_from(index as u64 + 1).expect("position"))
            && record.envelope().router_time == start().0 - TimeDelta::days(3650)
    }));
    let archive_at = RegistryTime(start().0 + TimeDelta::minutes(1));
    fixture.archive(archive_at);
    let archived = page(fixture.read(after(25), archive_at), 51, archive_at);
    assert_eq!(archived.len(), 26, "N boards + workspace + ledger");
    for (index, record) in archived.iter().enumerate() {
        assert_eq!(
            record.envelope().position,
            RecordPosition::Fleet(FleetPosition::try_from(index as u64 + 26).expect("position"))
        );
    }
    assert_eq!(
        archived
            .iter()
            .filter(|r| matches!(r, Record::Board(b) if b.state == BoardState::Archived))
            .count(),
        24
    );
    assert!(
        archived
            .iter()
            .any(|r| matches!(r, Record::Workspace(w) if w.state == WorkspaceState::Archived))
    );
    assert!(matches!(archived.last(), Some(Record::Request(_))));
    assert!(page(fixture.read(after(51), archive_at), 51, archive_at).is_empty());
}

#[test]
fn old_import_times_do_not_expire_feed_before_thirty_days_of_central_retention() {
    let mut fixture = FeedFixture::imported(1);
    let later = RegistryTime(start().0 + TimeDelta::days(29));
    let imported = page(fixture.read(after(0), later), 2, later);
    assert_eq!(imported.len(), 2);
    assert!(
        imported
            .iter()
            .all(|record| record.envelope().router_time < start().0)
    );
    fixture.archive(later);
    let expired_at = RegistryTime(start().0 + TimeDelta::days(31));
    assert_eq!(
        fixture.read(after(0), expired_at),
        SessionAnswer::Refused(SessionRefusal::CursorExpired)
    );
    let remaining = page(fixture.read(after(2), expired_at), 5, expired_at);
    assert_eq!(remaining.len(), 3);
    let retained: i64 = fixture
        .store
        .connection
        .query_row("SELECT count(*) FROM fleet_feed", [], |row| row.get(0))
        .expect("retention rows");
    assert_eq!(retained, 3, "only central append time controls pruning");
    let full = page(fixture.read(None, expired_at), 5, expired_at);
    assert_eq!(full.len(), 3, "full records survive feed pruning");
}

#[test]
fn fleet_cursor_kind_and_future_position_are_refused_without_rewinding() {
    let mut fixture = FeedFixture::imported(1);
    for cursor in [
        after(3),
        Some(ReadCursor::Notices(
            collaboration_replication_protocol::NoticePosition::new(1).expect("notice position"),
        )),
    ] {
        assert_eq!(
            fixture.read(cursor, start()),
            SessionAnswer::Refused(SessionRefusal::InvalidShape {
                field: "after".into()
            })
        );
    }
    assert_eq!(page(fixture.read(None, start()), 2, start()).len(), 2);
}

#[test]
fn retention_alarm_prunes_every_entry_without_resetting_the_persisted_head() {
    let mut fixture = FeedFixture::imported(1);
    let expiry = RegistryTime(start().0 + TimeDelta::days(30));
    assert_eq!(
        read_object_alarm_plan(&mut fixture.store, &ObjectAddress::Fleet).expect("plan"),
        vec![FaceAlarm {
            face: CallerKind::Fleet,
            at: expiry
        }]
    );
    let effects = fixture
        .store
        .run(
            WireInput::Alarm {
                face: CallerKind::Fleet,
            },
            expiry,
        )
        .expect("retention alarm");
    assert!(
        effects.is_empty(),
        "pruning creates no dispatch or feed event"
    );
    assert!(
        read_object_alarm_plan(&mut fixture.store, &ObjectAddress::Fleet)
            .expect("plan")
            .is_empty()
    );
    let retained: i64 = fixture
        .store
        .connection
        .query_row("SELECT count(*) FROM fleet_feed", [], |row| row.get(0))
        .expect("feed");
    assert_eq!(retained, 0);
    assert!(page(fixture.read(after(2), expiry), 2, expiry).is_empty());
    assert_eq!(
        fixture.read(after(1), expiry),
        SessionAnswer::Refused(SessionRefusal::CursorExpired)
    );
    fixture.archive(expiry);
    let appended = page(fixture.read(after(2), expiry), 5, expiry);
    assert_eq!(appended.len(), 3);
    assert_eq!(
        appended.first().expect("new entry").envelope().position,
        RecordPosition::Fleet(FleetPosition::try_from(3).expect("position"))
    );
}

#[test]
fn inconsistent_feed_record_position_is_rejected_without_advancing_the_reader() {
    let mut fixture = FeedFixture::imported(1);
    // Use the actual serialized path, independently changing the retained
    // entry position rather than invoking the production encoder as oracle.
    fixture
        .store
        .connection
        .execute(
            "UPDATE fleet_feed SET entry=json_set(entry,'$.position',99) WHERE position=1",
            [],
        )
        .expect("corrupt entry");
    let result = fixture.store.run(
        WireInput::Call {
            from: ObjectAddress::Connection(machine(1)),
            call_id: ObjectCallId::new(identity(59_002)).expect("call"),
            call: Box::new(WireCall::Fleet(FleetCall::Read {
                installation: machine(1),
                scope: ReadScope::FleetRecords,
                after: after(0),
            })),
        },
        start(),
    );
    assert!(
        matches!(result, Err(CentralError::Store(StoreError::Corrupt { field })) if field == "fleet feed entry position")
    );
    let readers: i64 = fixture
        .store
        .connection
        .query_row("SELECT count(*) FROM feed_readers", [], |row| row.get(0))
        .expect("reader count");
    assert_eq!(readers, 0);
}
