#![allow(clippy::expect_used, clippy::panic)]
mod registry_support;
use collaboration_central_core::*;
use collaboration_replication_protocol::{FleetPayload, FleetRequestEnvelope, SessionAnswer};
use fleet_admission::*;
use message_board::*;
use registry_support::{identity, machine, start};

fn input(request: &FleetRequest) -> WireInput {
    WireInput::Call {
        from: ObjectAddress::Connection(machine(1)),
        call_id: ObjectCallId::new(identity(90)).expect("call"),
        call: Box::new(WireCall::Fleet(FleetCall::Fleet {
            installation: machine(1),
            request: FleetRequestEnvelope {
                request_id: collaboration_replication_protocol::RequestId::new(
                    request.request_id.as_str(),
                )
                .expect("request"),
                operation: FleetPayload::new(serde_json::to_value(request).expect("encode")),
            },
        })),
    }
}
fn answer(effects: Vec<WireEffect>) -> FleetRequestResult {
    let (alarms, effects): (Vec<_>, Vec<_>) = effects
        .into_iter()
        .partition(|effect| matches!(effect, WireEffect::SetAlarm { .. }));
    assert!(
        matches!(alarms.as_slice(), [WireEffect::SetAlarm { at }] if at.0 == start().0 + chrono::TimeDelta::days(30))
    );
    let [
        WireEffect::Return {
            result:
                WireResult::Fleet(FleetResult::Fleet(SessionAnswer::FleetDecision { decision, .. })),
            ..
        },
    ] = effects.as_slice()
    else {
        panic!("one typed Fleet result")
    };
    serde_json::from_value(decision.as_value().clone()).expect("result")
}
fn snapshot(
    store: &registry_support::sql_support::SqlTxFake,
) -> std::collections::BTreeMap<String, Vec<Vec<rusqlite::types::Value>>> {
    [
        "fleet_records",
        "fleet_workspaces",
        "fleet_boards",
        "fleet_directory",
        "fleet_request_ledger",
        "fleet_feed",
        "fleet_checkpoint",
        "fleet_repositories",
    ]
    .into_iter()
    .map(|table| {
        let mut query = store
            .connection
            .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
            .expect("query");
        let columns = query.column_count();
        let rows = query
            .query_map([], |row| {
                (0..columns)
                    .map(|index| row.get::<_, rusqlite::types::Value>(index))
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .expect("rows")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("exact rows");
        (table.to_owned(), rows)
    })
    .collect()
}

#[test]
fn decided_replay_and_conflicting_request_preserve_every_fleet_row_and_feed_entry() {
    let mut store = registry_support::sql_support::SqlTxFake::blank();
    let request = FleetRequest {
        request_id: RequestId::try_from(identity(1)).expect("request"),
        workspace: WorkspaceId::try_from(identity(2)).expect("workspace"),
        actor: RecordedActor {
            actor: ActorKind::Declared(Identity::Human {
                human_id: HumanId::try_from("owner".to_owned()).expect("owner"),
            }),
            installation: ServiceId::try_from(machine(1).as_str().to_owned())
                .expect("installation"),
            machine: MachineLabel::try_from("machine1".to_owned()).expect("machine"),
        },
        acting_for: None,
        resubmits: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation: FleetOperation::WorkspaceCreate {
            name: ResourceName::try_from("exact workspace".to_owned()).expect("name"),
            description: Description::try_from("exact description".to_owned())
                .expect("description"),
        },
    };
    let first = answer(store.run(input(&request), start()).expect("accept"));
    assert!(matches!(
        first,
        FleetRequestResult::Decided(RequestDecision::Accepted {
            change: ChangeOutcome::Changed,
            ..
        })
    ));
    let before = snapshot(&store);
    assert_eq!(before["fleet_request_ledger"].len(), 1);
    assert_eq!(before["fleet_records"].len(), 2);
    assert_eq!(before["fleet_feed"].len(), 2);
    assert_eq!(
        answer(store.run(input(&request), start()).expect("replay")),
        first
    );
    assert_eq!(snapshot(&store), before);
    let mut changed = request.clone();
    changed.operation = FleetOperation::WorkspaceUpdate {
        name: ResourceName::try_from("different".to_owned()).expect("name"),
        description: Description::try_from("different".to_owned()).expect("description"),
    };
    assert_eq!(
        answer(
            store
                .run(input(&changed), start())
                .expect("conflict answer")
        ),
        FleetRequestResult::ConflictingRequest
    );
    assert_eq!(
        snapshot(&store),
        before,
        "conflict cannot alter any record, ledger, feed or checkpoint row"
    );
    assert_eq!(
        answer(
            store
                .run(input(&request), start())
                .expect("original replay")
        ),
        first
    );
    assert_eq!(snapshot(&store), before);
}
