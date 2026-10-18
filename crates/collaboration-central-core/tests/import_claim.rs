#![allow(clippy::expect_used, clippy::panic)]
mod registry_support;
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
use registry_support::*;
fn pages() -> [ProjectImportPage; 2] {
    let sql = "CREATE TABLE records(id INTEGER PRIMARY KEY, label TEXT NOT NULL)".to_owned();
    let header = ImportSnapshotHeader {
        project: project(),
        head: position(1, 2),
        schema: SchemaIdentity::new(
            1,
            schema_fingerprint(&[SchemaEntry {
                kind: SchemaObjectKind::Table,
                name: "records".into(),
                sql: sql.clone(),
            }])
            .expect("fingerprint"),
        )
        .expect("schema"),
        schema_steps: vec![sql],
    };
    let summary = ProjectSummary {
        status: "imported".into(),
        latest_record_at: registry_support::start().0,
    };
    let data = [
        vec![SqlValue::Integer(1), SqlValue::Text("first".into())],
        vec![SqlValue::Integer(2), SqlValue::Text("second".into())],
    ];
    let mut digest = ImportDigestBuilder::new(&header).expect("header");
    for row in &data {
        digest.append_row("records", row).expect("row");
    }
    let digest = digest.finish(&summary).expect("digest");
    data.into_iter()
        .enumerate()
        .map(|(index, row)| ProjectImportPage {
            digest: digest.clone(),
            index: index as u64,
            page: SnapshotPage {
                project: project(),
                position: header.head,
                schema: header.schema.clone(),
                schema_steps: header.schema_steps.clone(),
                table: "records".into(),
                rows: vec![row],
                next: if index == 0 {
                    Some(SnapshotCursor {
                        position: header.head,
                        table: "records".into(),
                        after_key: vec![SqlValue::Integer(1)],
                    })
                } else {
                    None
                },
                as_of: registry_support::start().0,
            },
            final_summary: if index == 1 {
                Some(summary.clone())
            } else {
                None
            },
        })
        .collect::<Vec<_>>()
        .try_into()
        .expect("two import pages")
}
fn import(page: &ProjectImportPage) -> WireCall {
    admin(AdminRequest::ImportProject {
        project: project(),
        upload: Box::new(page.clone()),
    })
}
fn seed_fleet_identity(harness: &mut Harness) {
    use fleet_admission::*;
    use message_board::{BoardId, Description, MachineLabel, ResourceName, ServiceId, WorkspaceId};
    let workspace = WorkspaceId::try_from(identity(700)).expect("workspace");
    let board = BoardId::try_from(identity(701)).expect("board");
    let bundle = FleetImportBundle {
        provenance: ImportProvenance {
            installation: ServiceId::try_from(machine(1).as_str().to_owned())
                .expect("installation"),
            machine: MachineLabel::try_from("claim fixture".to_owned()).expect("machine"),
        },
        records: vec![
            FleetImportRecord::Workspace {
                id: workspace.clone(),
                name: ResourceName::try_from("Claim Workspace".to_owned()).expect("name"),
                description: Description::try_from("Claim fixture".to_owned())
                    .expect("description"),
                repositories: vec![],
                created_at: start().0,
            },
            FleetImportRecord::Board {
                id: board.clone(),
                workspace,
                name: ResourceName::try_from("Claim Board".to_owned()).expect("name"),
                description: Description::try_from("Claim fixture".to_owned())
                    .expect("description"),
                state: ImportedBoardState::Active,
                created_at: start().0,
            },
            FleetImportRecord::Directory {
                project: message_board::ProjectId::try_from(project().as_str().to_owned())
                    .expect("project"),
                board,
                title: ResourceName::try_from("Imported project".to_owned()).expect("title"),
                created_at: start().0,
            },
        ],
    };
    assert_eq!(
        harness.call(
            "fleet",
            WireCall::Fleet(FleetCall::Import {
                bundle: Box::new(bundle)
            })
        ),
        WireResult::Fleet(FleetResult::Imported(FleetImportResult::Imported {
            counts: FleetImportCounts {
                workspaces: 1,
                boards: 1,
                directory_entries: 1
            }
        }))
    );
}
fn setup() -> Harness {
    let mut h = Harness::new();
    seed_fleet_identity(&mut h);
    h.enroll(1);
    h.enroll(2);
    h.register(1, 10);
    h.register(2, 10);
    for page in pages() {
        h.call("registry", import(&page));
    }
    h
}
fn claim(n: u64, attempt: u64) -> WireCall {
    WireCall::Registry(RegistryCall::Claim {
        attempt: request(attempt),
        project: project(),
        claimant: machine(n),
    })
}
fn count(h: &mut Harness, store: &str, table: &str) -> i64 {
    h.store(store)
        .connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .expect("count")
}
fn copy(h: &mut Harness) -> CopyStatus {
    match h.call(
        "copy",
        WireCall::Copy(CopyCall::CopyStatus { project: project() }),
    ) {
        WireResult::Copy(CopyResult::CopyStatus(Some(state))) => state,
        _ => panic!("state"),
    }
}
#[test]
fn staged_import_is_invisible_and_reimport_never_changes_the_summary() {
    let mut h = Harness::new();
    let pages = pages();
    assert!(matches!(
        h.call("registry", import(&pages[0])),
        WireResult::Registry(RegistryResult::Admin(AdminResult::ProjectImport(
            ProjectImportAnswer::Staged { index: 0, .. }
        )))
    ));
    assert_eq!(count(&mut h, "copy", "_copy_state"), 0);
    assert_eq!(count(&mut h, "copy", "_copy_summary"), 0);
    assert!(matches!(
        h.call("registry", import(&pages[1])),
        WireResult::Registry(RegistryResult::Admin(AdminResult::ProjectImport(
            ProjectImportAnswer::Imported { .. }
        )))
    ));
    assert_eq!(copy(&mut h).holder, None);
    assert_eq!(h.store("copy").record_ids(), vec![1, 2]);
    let before:String=h.store("copy").connection.query_row("SELECT status||latest_record_at||position_epoch||position_commit||acked_at FROM _copy_summary",[],|row|row.get(0)).expect("summary");
    assert_eq!(
        h.call("registry", import(&pages[0])),
        WireResult::Registry(RegistryResult::Admin(AdminResult::ProjectImport(
            ProjectImportAnswer::AlreadyImported
        )))
    );
    let after:String=h.store("copy").connection.query_row("SELECT status||latest_record_at||position_epoch||position_commit||acked_at FROM _copy_summary",[],|row|row.get(0)).expect("summary");
    assert_eq!(after, before);
}
#[test]
fn claim_fences_before_grant_and_one_attempt_keeps_one_epoch_and_expiry() {
    let mut h = setup();
    let (input, effects) = h.begin("registry", claim(1, 90));
    assert_eq!(count(&mut h, "registry", "_registry_leases"), 0);
    assert_eq!(
        h.call("registry", claim(2, 91)),
        WireResult::Registry(RegistryResult::Claim(SessionAnswer::Refused(
            SessionRefusal::AlreadyHeld { holder: machine(1) }
        )))
    );
    assert_eq!(copy(&mut h).fence, epoch(1));
    assert!(
        !effects.iter().any(|effect| matches!(
            effect,
            WireEffect::Dispatch {
                call: WireCall::Copy(CopyCall::Fence { .. }),
                ..
            }
        )),
        "Reserving cannot fence before Fleet accepts"
    );
    let (call_id, reserve_call, carry) = effects
        .into_iter()
        .find_map(|effect| match effect {
            WireEffect::Dispatch {
                call_id,
                call: call @ WireCall::Fleet(FleetCall::ReserveClaim { .. }),
                carry,
                ..
            } => Some((call_id, call, carry)),
            _ => None,
        })
        .expect("real reservation dispatch");
    let result = h.call("fleet", reserve_call);
    assert_eq!(
        result,
        WireResult::Fleet(FleetResult::ClaimReserved(ReserveClaimResult::Reserved))
    );
    let effects = h.run(
        "registry",
        WireInput::CallResult {
            source: CallerKind::Registry,
            call_id,
            result: Box::new(result),
            carry,
        },
    );
    let fence = effects
        .iter()
        .find_map(|effect| match effect {
            WireEffect::Dispatch {
                call: call @ WireCall::Copy(CopyCall::Fence { .. }),
                ..
            } => Some(call.clone()),
            _ => None,
        })
        .expect("fence");
    h.call("copy", fence);
    assert_eq!(copy(&mut h).fence, epoch(2));
    assert_eq!(copy(&mut h).holder, Some(machine(1)));
    assert_eq!(count(&mut h, "registry", "_registry_leases"), 0);
    let expected = WireResult::Registry(RegistryResult::Claim(SessionAnswer::Claimed {
        epoch: epoch(2),
        expires_at: at(240).0,
        start: position(1, 2),
    }));
    assert_eq!(h.finish("registry", &input, effects).0, expected);
    h.now = at(1);
    assert_eq!(h.call("registry", claim(1, 90)), expected);
    assert_eq!(count(&mut h, "registry", "_registry_leases"), 1);
    assert_eq!(count(&mut h, "copy", "_copy_transfers"), 1);
}
#[test]
fn copy_fence_restore_guard_refuses_claim_without_grant_or_floor_repair() {
    let mut h = setup();
    h.call(
        "copy",
        WireCall::Copy(CopyCall::Fence {
            transfer_id: transfer(300),
            new_epoch: epoch(2),
            holder: machine(1),
            expect: FenceExpectation {
                epoch: epoch(1),
                acked: position(1, 2),
            },
            target_schema_max: 10,
        }),
    );
    assert_eq!(
        h.call("registry", claim(2, 92)),
        WireResult::Registry(RegistryResult::Claim(SessionAnswer::Refused(
            SessionRefusal::AlreadyHeld { holder: machine(1) }
        )))
    );
    assert_eq!(copy(&mut h).fence, epoch(2));
    assert_eq!(count(&mut h, "registry", "_registry_leases"), 0);
}
#[test]
fn late_directory_ack_cannot_remove_a_newer_summary_outbox_row() {
    let mut h = Harness::new();
    seed_fleet_identity(&mut h);
    h.enroll(1);
    h.register(1, 10);
    let pages = pages();
    h.call(
        "copy",
        WireCall::Copy(CopyCall::ImportSeed {
            project: project(),
            upload: Box::new(pages[0].clone()),
        }),
    );
    let (_, effects) = h.begin(
        "copy",
        WireCall::Copy(CopyCall::ImportSeed {
            project: project(),
            upload: Box::new(pages[1].clone()),
        }),
    );
    let old_id = effects
        .iter()
        .find_map(|effect| match effect {
            WireEffect::Dispatch { call_id, .. } => Some(call_id.clone()),
            _ => None,
        })
        .expect("old update");
    h.call(
        "registry",
        WireCall::Registry(RegistryCall::RecordUnheld(UnheldProject {
            project: project(),
            import_epoch: epoch(1),
            imported_head: position(1, 2),
            schema_version: 1,
        })),
    );
    h.call("registry", claim(1, 93));
    let piece = Piece::new(PieceContents {
        project: project(),
        epoch: epoch(2),
        continues_from: position(1, 2),
        through: position(2, 1),
        transactions: vec![ReplicatedTransaction::Rows {
            commit_id: 1.into(),
            rows: vec![RowChange::Upsert {
                table: "records".into(),
                key: vec![SqlValue::Integer(3)],
                values: vec![SqlValue::Integer(3), SqlValue::Text("new".into())],
            }],
        }],
        part: None,
        summary: Some(ProjectSummary {
            status: "active".into(),
            latest_record_at: at(0).0,
        }),
    })
    .expect("piece");
    let (_, effects) = h.begin("copy", apply_call(piece));
    let new_id = effects
        .iter()
        .find_map(|effect| match effect {
            WireEffect::Dispatch { call_id, .. } => Some(call_id.clone()),
            _ => None,
        })
        .expect("new update");
    assert_ne!(new_id, old_id);
    h.run(
        "copy",
        WireInput::CallResult {
            source: CallerKind::Copy,
            call_id: old_id,
            result: Box::new(WireResult::Fleet(FleetResult::DirectoryUpdated)),
            carry: None,
        },
    );
    assert_eq!(count(&mut h, "copy", "_copy_directory_outbox"), 1);
    h.run(
        "copy",
        WireInput::CallResult {
            source: CallerKind::Copy,
            call_id: new_id,
            result: Box::new(WireResult::Fleet(FleetResult::DirectoryUpdated)),
            carry: None,
        },
    );
    assert_eq!(count(&mut h, "copy", "_copy_directory_outbox"), 0);
}

#[test]
fn stored_import_metadata_for_another_project_is_rejected_explicitly() {
    let mut h = setup();
    let record: String = h
        .store("copy")
        .connection
        .query_row("SELECT record FROM _copy_import_metadata", [], |row| {
            row.get(0)
        })
        .expect("metadata");
    let mut record: serde_json::Value = serde_json::from_str(&record).expect("record");
    *record
        .get_mut("header")
        .and_then(|header| header.get_mut("project"))
        .expect("project field") = serde_json::json!(identity(900));
    h.store("copy")
        .connection
        .execute(
            "UPDATE _copy_import_metadata SET record=?",
            [serde_json::to_string(&record).expect("encode")],
        )
        .expect("corrupt");
    let input = h.input(WireCall::Copy(CopyCall::CopyStatus { project: project() }));
    let now = h.now;
    assert!(
        matches!(h.store("copy").run(input,now),Err(CentralError::Store(StoreError::Corrupt{field})) if field=="import metadata")
    );
}

#[test]
fn rerunning_import_repairs_the_copy_to_registry_publication_gap() {
    let mut h = Harness::new();
    seed_fleet_identity(&mut h);
    h.enroll(1);
    h.register(1, 10);
    for page in pages() {
        h.call(
            "copy",
            WireCall::Copy(CopyCall::ImportSeed {
                project: project(),
                upload: Box::new(page),
            }),
        );
    }
    assert_eq!(
        h.call("registry", claim(1, 94)),
        WireResult::Registry(RegistryResult::Claim(SessionAnswer::Refused(
            SessionRefusal::NotFound
        )))
    );
    let first = pages().first().expect("first").clone();
    assert_eq!(
        h.call("registry", import(&first)),
        WireResult::Registry(RegistryResult::Admin(AdminResult::ProjectImport(
            ProjectImportAnswer::AlreadyImported
        )))
    );
    assert!(
        matches!(h.call("registry",claim(1,95)),WireResult::Registry(RegistryResult::Claim(SessionAnswer::Claimed{epoch,..})) if epoch.value()==2)
    );
}

#[test]
fn claimed_copy_preserves_holder_change_floor_repair_semantics() {
    let mut h = setup();
    h.call("registry", claim(1, 96));
    h.call(
        "copy",
        WireCall::Copy(CopyCall::Fence {
            transfer_id: transfer(400),
            new_epoch: epoch(4),
            holder: machine(2),
            expect: FenceExpectation {
                epoch: epoch(2),
                acked: position(1, 2),
            },
            target_schema_max: 10,
        }),
    );
    assert_eq!(
        h.call(
            "copy",
            WireCall::Copy(CopyCall::Fence {
                transfer_id: transfer(401),
                new_epoch: epoch(3),
                holder: machine(1),
                expect: FenceExpectation {
                    epoch: epoch(2),
                    acked: position(1, 2)
                },
                target_schema_max: 10
            })
        ),
        WireResult::Copy(CopyResult::Fence(FenceResult::FenceNotAdvanced {
            current_epoch: epoch(4)
        }))
    );
    assert_eq!(
        h.call(
            "copy",
            WireCall::Copy(CopyCall::Fence {
                transfer_id: transfer(401),
                new_epoch: epoch(5),
                holder: machine(1),
                expect: FenceExpectation {
                    epoch: epoch(4),
                    acked: position(1, 2)
                },
                target_schema_max: 10
            })
        ),
        WireResult::Copy(CopyResult::Fence(FenceResult::Fenced {
            old_epoch: epoch(4),
            new_epoch: epoch(5),
            start: position(1, 2)
        }))
    );
}
