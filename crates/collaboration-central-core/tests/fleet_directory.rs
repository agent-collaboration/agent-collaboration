#![allow(clippy::expect_used, clippy::panic)]
mod registry_support;
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
use registry_support::*;

fn deliver(store: &mut registry_support::sql_support::SqlTxFake, update: DirectoryUpdate) {
    let is_summary = matches!(update, DirectoryUpdate::Summary { .. });
    let result = without_retention_alarm(
        store
            .run(
                WireInput::Call {
                    from: if is_summary {
                        ObjectAddress::Copy(project())
                    } else {
                        ObjectAddress::Registry
                    },
                    call_id: ObjectCallId::new(identity(90)).expect("call"),
                    call: Box::new(WireCall::Fleet(FleetCall::DirectoryUpdate(update))),
                },
                start(),
            )
            .expect("real Fleet receiver"),
    );
    assert!(
        matches!(
            result.as_slice(),
            [WireEffect::Return {
                result: WireResult::Fleet(FleetResult::DirectoryUpdated),
                ..
            }]
        ) && is_summary
            || matches!(
                result.as_slice(),
                [WireEffect::Return {
                    result: WireResult::Fleet(FleetResult::LeaseUpdated(_)),
                    ..
                }]
            ) && !is_summary
    );
}
fn summary(commit: u64, label: &str, minute: i64) -> DirectoryUpdate {
    DirectoryUpdate::Summary {
        project: project(),
        summary: ProjectSummary {
            status: label.into(),
            latest_record_at: at(minute).0,
        },
        position: position(1, commit),
        acked_at: at(minute),
    }
}
fn lease(holder: u64, minute: i64) -> DirectoryUpdate {
    DirectoryUpdate::lease(
        project(),
        DirectoryLease::Held {
            holder: machine(holder),
            state: LeaseState::Held,
            expires_at: at(60).0,
        },
        LeaseVersion::new(
            epoch(1),
            RegistryIncarnation::new(1).expect("incarnation"),
            u64::try_from(minute + 1).expect("change"),
        )
        .expect("version"),
        at(minute),
    )
    .expect("lease update")
}
fn stored(store: &registry_support::sql_support::SqlTxFake, table: &str) -> DirectoryUpdate {
    let payload: String = store
        .connection
        .query_row(
            &format!("SELECT record FROM {table} WHERE project=?"),
            [project().as_str()],
            |row| row.get(0),
        )
        .expect("actual projection");
    serde_json::from_str(&payload).expect("typed projection")
}

#[test]
fn real_fleet_keeps_summary_by_position_and_lease_by_independent_registry_version() {
    let mut store = registry_support::sql_support::SqlTxFake::blank();
    let newest_summary = summary(8, "latest", 5);
    let newest_lease = lease(2, 7);
    deliver(&mut store, newest_summary.clone());
    deliver(&mut store, newest_lease.clone());
    deliver(&mut store, summary(7, "stale", 20));
    deliver(&mut store, lease(1, 6));
    assert_eq!(stored(&store, "directory_status"), newest_summary);
    assert_eq!(stored(&store, "directory_lease"), newest_lease);
    deliver(&mut store, newest_summary.clone());
    deliver(&mut store, newest_lease.clone());
    assert_eq!(stored(&store, "directory_status"), newest_summary);
    assert_eq!(stored(&store, "directory_lease"), newest_lease);
    // An identical source position/version cannot refresh an old side's stamp.
    deliver(&mut store, summary(8, "conflicting same position", 30));
    deliver(&mut store, lease(3, 7));
    assert_eq!(stored(&store, "directory_status"), newest_summary);
    assert_eq!(stored(&store, "directory_lease"), newest_lease);
}

#[test]
fn unheld_directory_update_changes_only_lease_and_never_creates_fleet_records() {
    let mut store = registry_support::sql_support::SqlTxFake::blank();
    let expected = summary(3, "imported", 2);
    deliver(&mut store, expected.clone());
    deliver(&mut store, lease(1, 2));
    let unheld = DirectoryUpdate::lease(
        project(),
        DirectoryLease::Unheld,
        LeaseVersion::new(
            epoch(1),
            RegistryIncarnation::new(1).expect("incarnation"),
            4,
        )
        .expect("version"),
        at(3),
    )
    .expect("unheld");
    deliver(&mut store, unheld.clone());
    assert_eq!(stored(&store, "directory_status"), expected);
    assert_eq!(stored(&store, "directory_lease"), unheld);
    let records: i64 = store
        .connection
        .query_row("SELECT count(*) FROM fleet_records", [], |row| row.get(0))
        .expect("records");
    assert_eq!(
        records, 0,
        "metadata producer cannot fabricate directory identity"
    );
}

fn fleet_call(
    store: &mut registry_support::sql_support::SqlTxFake,
    call: FleetCall,
) -> Vec<WireEffect> {
    without_retention_alarm(
        store
            .run(
                WireInput::Call {
                    from: ObjectAddress::Connection(machine(1)),
                    call_id: ObjectCallId::new(identity(91)).expect("call"),
                    call: Box::new(WireCall::Fleet(call)),
                },
                start(),
            )
            .expect("real Fleet call"),
    )
}
fn without_retention_alarm(effects: Vec<WireEffect>) -> Vec<WireEffect> {
    let (alarms, answers): (Vec<_>, Vec<_>) = effects
        .into_iter()
        .partition(|effect| matches!(effect, WireEffect::SetAlarm { .. }));
    if !alarms.is_empty() {
        assert!(
            matches!(alarms.as_slice(), [WireEffect::SetAlarm { at }] if at.0 == start().0 + chrono::TimeDelta::days(30))
        );
    }
    answers
}
fn directory_identity(
    store: &mut registry_support::sql_support::SqlTxFake,
) -> fleet_admission::DirectoryEntry {
    use fleet_admission::*;
    use message_board::{BoardId, Description, MachineLabel, ResourceName, ServiceId, WorkspaceId};
    let workspace = WorkspaceId::try_from(identity(3)).expect("workspace");
    let board = BoardId::try_from(identity(2)).expect("board");
    let fleet_project =
        message_board::ProjectId::try_from(project().as_str().to_owned()).expect("project");
    let effects = fleet_call(
        store,
        FleetCall::Import {
            bundle: Box::new(FleetImportBundle {
                provenance: ImportProvenance {
                    installation: ServiceId::try_from(machine(1).as_str().to_owned())
                        .expect("installation"),
                    machine: MachineLabel::try_from("directory fixture".to_owned())
                        .expect("machine"),
                },
                records: vec![
                    FleetImportRecord::Workspace {
                        id: workspace.clone(),
                        name: ResourceName::try_from("Workspace".to_owned()).expect("name"),
                        description: Description::try_from("Description".to_owned())
                            .expect("description"),
                        repositories: Vec::new(),
                        created_at: start().0,
                    },
                    FleetImportRecord::Board {
                        id: board.clone(),
                        workspace: workspace.clone(),
                        name: ResourceName::try_from("Board".to_owned()).expect("name"),
                        description: Description::try_from("Description".to_owned())
                            .expect("description"),
                        state: ImportedBoardState::Active,
                        created_at: start().0,
                    },
                    FleetImportRecord::Directory {
                        project: fleet_project.clone(),
                        board: board.clone(),
                        title: ResourceName::try_from("Alpha Project".to_owned()).expect("name"),
                        created_at: start().0,
                    },
                ],
            }),
        },
    );
    assert!(matches!(
        effects.as_slice(),
        [WireEffect::Return {
            result: WireResult::Fleet(FleetResult::Imported(FleetImportResult::Imported {
                counts: FleetImportCounts {
                    workspaces: 1,
                    boards: 1,
                    directory_entries: 1
                }
            })),
            ..
        }]
    ));
    DirectoryEntry {
        project: fleet_project,
        name: ResourceName::try_from("Alpha Project".to_owned()).expect("name"),
        workspace,
        board,
        board_archive: message_board::BoardState::Open,
        copy_side: DirectorySide::NotYetReported,
        lease_side: DirectorySide::NotYetReported,
    }
}
fn directory_query(
    store: &mut registry_support::sql_support::SqlTxFake,
    query: fleet_admission::DirectoryQuery,
) -> Vec<fleet_admission::DirectoryEntry> {
    let effects = fleet_call(
        store,
        FleetCall::Read {
            installation: machine(1),
            scope: ReadScope::Directory {
                query: FleetPayload::new(serde_json::to_value(query).expect("query")),
            },
            after: None,
        },
    );
    let [
        WireEffect::Return {
            result:
                WireResult::Fleet(FleetResult::Read(SessionAnswer::ReadPage(ReadPage::Directory {
                    entries,
                    as_of,
                }))),
            ..
        },
    ] = effects.as_slice()
    else {
        panic!("actual public directory page");
    };
    assert_eq!(*as_of, start().0);
    entries
        .iter()
        .map(|entry| serde_json::from_value(entry.as_value().clone()).expect("typed public entry"))
        .collect()
}

#[test]
fn public_directory_keeps_partial_sides_visible_and_filters_only_known_metadata() {
    use fleet_admission::*;
    let mut store = registry_support::sql_support::SqlTxFake::blank();
    let mut expected = directory_identity(&mut store);
    assert_eq!(
        directory_query(&mut store, DirectoryQuery::default()),
        vec![expected.clone()]
    );
    for query in [
        DirectoryQuery {
            status: Some("latest".into()),
            ..DirectoryQuery::default()
        },
        DirectoryQuery {
            holder: Some(machine(1)),
            ..DirectoryQuery::default()
        },
    ] {
        assert!(directory_query(&mut store, query).is_empty());
    }
    deliver(&mut store, summary(8, "latest", 5));
    expected.copy_side = DirectorySide::Known(CopyMetadata {
        status: "latest".into(),
        latest_record_at: at(5).0,
        as_of: CopyMetadataAsOf {
            position: position(1, 8),
            acked_at: at(5).0,
        },
    });
    assert_eq!(
        directory_query(
            &mut store,
            DirectoryQuery {
                text: Some("ALPHA".into()),
                workspace: Some(expected.workspace.clone()),
                board: Some(expected.board.clone()),
                status: Some("latest".into()),
                holder: None
            }
        ),
        vec![expected.clone()]
    );
    for query in [
        DirectoryQuery {
            text: Some("missing".into()),
            ..DirectoryQuery::default()
        },
        DirectoryQuery {
            workspace: Some(message_board::WorkspaceId::try_from(identity(99)).expect("workspace")),
            ..DirectoryQuery::default()
        },
        DirectoryQuery {
            board: Some(message_board::BoardId::try_from(identity(99)).expect("board")),
            ..DirectoryQuery::default()
        },
        DirectoryQuery {
            status: Some("stale".into()),
            ..DirectoryQuery::default()
        },
        DirectoryQuery {
            holder: Some(machine(1)),
            ..DirectoryQuery::default()
        },
    ] {
        assert!(directory_query(&mut store, query).is_empty());
    }
    deliver(
        &mut store,
        DirectoryUpdate::lease(
            project(),
            DirectoryLease::Unheld,
            LeaseVersion::new(
                epoch(1),
                RegistryIncarnation::new(1).expect("incarnation"),
                8,
            )
            .expect("version"),
            at(7),
        )
        .expect("unheld"),
    );
    expected.lease_side = DirectorySide::Known(LeaseMetadata {
        lease: DirectoryLease::Unheld,
        as_of: at(7).0,
    });
    assert_eq!(
        directory_query(&mut store, DirectoryQuery::default()),
        vec![expected]
    );
    assert!(
        directory_query(
            &mut store,
            DirectoryQuery {
                holder: Some(machine(1)),
                ..DirectoryQuery::default()
            }
        )
        .is_empty()
    );
}

#[test]
fn public_directory_exposes_independent_freshness_without_late_or_equal_rewinds() {
    use fleet_admission::*;
    let mut store = registry_support::sql_support::SqlTxFake::blank();
    let mut expected = directory_identity(&mut store);
    deliver(&mut store, summary(8, "latest", 5));
    deliver(&mut store, lease(2, 7));
    expected.copy_side = DirectorySide::Known(CopyMetadata {
        status: "latest".into(),
        latest_record_at: at(5).0,
        as_of: CopyMetadataAsOf {
            position: position(1, 8),
            acked_at: at(5).0,
        },
    });
    expected.lease_side = DirectorySide::Known(LeaseMetadata {
        lease: DirectoryLease::Held {
            holder: machine(2),
            state: LeaseState::Held,
            expires_at: at(60).0,
        },
        as_of: at(7).0,
    });
    assert_eq!(
        directory_query(
            &mut store,
            DirectoryQuery {
                holder: Some(machine(2)),
                ..DirectoryQuery::default()
            }
        ),
        vec![expected.clone()]
    );
    for update in [
        summary(7, "stale", 20),
        lease(1, 6),
        summary(8, "conflicting equal", 30),
        lease(3, 7),
    ] {
        deliver(&mut store, update);
        assert_eq!(
            directory_query(&mut store, DirectoryQuery::default()),
            vec![expected.clone()]
        );
    }
    assert!(
        directory_query(
            &mut store,
            DirectoryQuery {
                holder: Some(machine(1)),
                ..DirectoryQuery::default()
            }
        )
        .is_empty()
    );
}

#[test]
fn public_directory_refuses_an_invalid_typed_query() {
    let mut store = registry_support::sql_support::SqlTxFake::blank();
    directory_identity(&mut store);
    let effects = fleet_call(
        &mut store,
        FleetCall::Read {
            installation: machine(1),
            scope: ReadScope::Directory {
                query: FleetPayload::new(serde_json::json!({"status":7})),
            },
            after: None,
        },
    );
    assert!(
        matches!(effects.as_slice(), [WireEffect::Return { result: WireResult::Fleet(FleetResult::Read(SessionAnswer::Refused(SessionRefusal::InvalidShape { field }))), .. }] if field == "directoryQuery")
    );
}
