#![allow(clippy::panic_in_result_fn)]
use crate::*;
use message_board::*;

type TestResult<TValue = ()> = Result<TValue, Box<dyn std::error::Error + Send + Sync>>;
fn workspace(value: u64) -> TestResult<WorkspaceId> {
    Ok(WorkspaceId::try_from(format!(
        "019f0000-0000-7000-8000-{value:012x}"
    ))?)
}
fn board(value: u64) -> TestResult<BoardId> {
    Ok(BoardId::try_from(format!(
        "019f0000-0000-7000-8000-{value:012x}"
    ))?)
}
fn project(value: u64) -> TestResult<ProjectId> {
    Ok(ProjectId::try_from(format!(
        "019f0000-0000-7000-8000-{value:012x}"
    ))?)
}
fn provenance(value: u64) -> TestResult<ImportProvenance> {
    Ok(ImportProvenance {
        installation: ServiceId::try_from(format!("00000000-0000-4000-8000-{value:012x}"))?,
        machine: MachineLabel::try_from(format!("importing computer {value}"))?,
    })
}
fn bundle() -> TestResult<FleetImportBundle> {
    let created_at = "2020-01-02T03:04:05Z".parse()?;
    Ok(FleetImportBundle {
        provenance: provenance(1)?,
        records: vec![
            FleetImportRecord::Directory {
                project: project(3)?,
                board: board(2)?,
                title: ResourceName::try_from("Original project".to_owned())?,
                created_at,
            },
            FleetImportRecord::Board {
                id: board(2)?,
                workspace: workspace(1)?,
                name: ResourceName::try_from("Original board".to_owned())?,
                description: Description::try_from("Legacy board description".to_owned())?,
                state: ImportedBoardState::Archived,
                created_at,
            },
            FleetImportRecord::Workspace {
                id: workspace(1)?,
                name: ResourceName::try_from("Original workspace".to_owned())?,
                description: Description::try_from("Legacy workspace description".to_owned())?,
                repositories: vec![RepositoryRef::Origin {
                    normalized_origin: NormalizedOrigin::try_from(
                        "example.com/owner/legacy".to_owned(),
                    )?,
                }],
                created_at,
            },
        ],
    })
}
fn adopted(decision: &FleetImportDecision) -> FleetSnapshot {
    let mut snapshot = FleetSnapshot::empty();
    for write in &decision.writes {
        snapshot.records.push(write.record.clone());
        snapshot.imports.push(ImportedFleetRecord {
            record: write.record.reference(),
            digest: write.digest.clone(),
        });
    }
    if let Some(entry) = decision.feed.last() {
        snapshot.high_water = entry.position;
    }
    snapshot
}

fn expected_imported_records(board_state: BoardState) -> TestResult<Vec<Record>> {
    let actor = RecordedActor {
        actor: ActorKind::Declared(Identity::Human {
            human_id: HumanId::try_from("owner".to_owned())?,
        }),
        installation: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
        machine: MachineLabel::try_from("importing computer 1".to_owned())?,
    };
    Ok(vec![
        Record::Workspace(Workspace {
            envelope: RecordEnvelope {
                id: RecordId::try_from("019f0000-0000-7000-8000-000000000001".to_owned())?,
                scope: RecordScope {
                    workspace: workspace(1)?,
                    board: None,
                    project: None,
                },
                actor: actor.clone(),
                request: None,
                position: RecordPosition::Fleet(FleetPosition::try_from(1)?),
                router_time: "2020-01-02T03:04:05Z".parse()?,
            },
            workspace_id: workspace(1)?,
            name: ResourceName::try_from("Original workspace".to_owned())?,
            description: Description::try_from("Legacy workspace description".to_owned())?,
            state: WorkspaceState::Active,
            repositories: vec![RepositoryRef::Origin {
                normalized_origin: NormalizedOrigin::try_from(
                    "example.com/owner/legacy".to_owned(),
                )?,
            }],
        }),
        Record::Board(Board {
            envelope: RecordEnvelope {
                id: RecordId::try_from("019f0000-0000-7000-8000-000000000002".to_owned())?,
                scope: RecordScope {
                    workspace: workspace(1)?,
                    board: Some(board(2)?),
                    project: None,
                },
                actor: actor.clone(),
                request: None,
                position: RecordPosition::Fleet(FleetPosition::try_from(2)?),
                router_time: "2020-01-02T03:04:05Z".parse()?,
            },
            board_id: board(2)?,
            workspace_id: workspace(1)?,
            name: ResourceName::try_from("Original board".to_owned())?,
            description: Description::try_from("Legacy board description".to_owned())?,
            kind: BoardKind::Area {},
            state: board_state,
        }),
        Record::DirectoryEntry(ProjectDirectoryEntry {
            envelope: RecordEnvelope {
                id: RecordId::try_from("019f0000-0000-7000-8000-000000000003".to_owned())?,
                scope: RecordScope {
                    workspace: workspace(1)?,
                    board: Some(board(2)?),
                    project: None,
                },
                actor,
                request: None,
                position: RecordPosition::Fleet(FleetPosition::try_from(3)?),
                router_time: "2020-01-02T03:04:05Z".parse()?,
            },
            id: project(3)?,
            board: board(2)?,
            title: ResourceName::try_from("Original project".to_owned())?,
        }),
    ])
}

#[test]
fn fleet_import_preserves_legacy_fields_and_first_provenance_across_other_machine_replay()
-> TestResult {
    for (offered_state, expected_state) in [
        (ImportedBoardState::Active, BoardState::Open),
        (ImportedBoardState::Archived, BoardState::Archived),
    ] {
        let mut original = bundle()?;
        for record in &mut original.records {
            if let FleetImportRecord::Board { state, .. } = record {
                *state = offered_state;
            }
        }
        let imported = decide_import(&original, &FleetSnapshot::empty())?;
        assert_eq!(
            imported.result,
            FleetImportResult::Imported {
                counts: FleetImportCounts {
                    workspaces: 1,
                    boards: 1,
                    directory_entries: 1,
                }
            }
        );
        let expected = expected_imported_records(expected_state)?;
        assert_eq!(
            imported
                .writes
                .iter()
                .map(|write| write.record.clone())
                .collect::<Vec<_>>(),
            expected,
            "complete imported records for {offered_state:?}"
        );
        assert_eq!(
            imported.feed,
            vec![
                FleetFeedEntry {
                    position: FleetPosition::try_from(1)?,
                    record: expected[0].clone()
                },
                FleetFeedEntry {
                    position: FleetPosition::try_from(2)?,
                    record: expected[1].clone()
                },
                FleetFeedEntry {
                    position: FleetPosition::try_from(3)?,
                    record: expected[2].clone()
                },
            ]
        );
        let snapshot = adopted(&imported);
        let mut replay = original.clone();
        replay.provenance = provenance(2)?;
        replay.records.reverse();
        assert_eq!(replay.digest()?, original.digest()?);
        let repeated = decide_import(&replay, &snapshot)?;
        assert_eq!(repeated.result, FleetImportResult::AlreadyImported);
        assert!(repeated.writes.is_empty() && repeated.feed.is_empty());
        assert_eq!(snapshot.records, expected);
    }
    Ok(())
}

#[test]
fn fleet_import_changed_description_state_and_time_are_digest_conflicts_without_writes()
-> TestResult {
    let original = bundle()?;
    let initial = decide_import(&original, &FleetSnapshot::empty())?;
    let snapshot = adopted(&initial);
    for field in [
        "workspaceDescription",
        "boardDescription",
        "state",
        "time",
        "repositories",
    ] {
        let mut changed = original.clone();
        for record in &mut changed.records {
            match record {
                FleetImportRecord::Workspace { repositories, .. } if field == "repositories" => {
                    repositories.clear()
                }
                FleetImportRecord::Workspace { description, .. }
                    if field == "workspaceDescription" =>
                {
                    *description =
                        Description::try_from("Changed workspace description".to_owned())?
                }
                FleetImportRecord::Board { description, .. } if field == "boardDescription" => {
                    *description = Description::try_from("Changed board description".to_owned())?
                }
                FleetImportRecord::Board { state, .. } if field == "state" => {
                    *state = ImportedBoardState::Active
                }
                FleetImportRecord::Board { created_at, .. } if field == "time" => {
                    *created_at = "2020-01-02T03:04:06Z".parse()?
                }
                _ => {}
            }
        }
        assert_ne!(changed.digest()?, original.digest()?);
        let decision = decide_import(&changed, &snapshot)?;
        assert!(
            matches!(decision.result, FleetImportResult::Refused { ref conflicts } if matches!(conflicts.as_slice(), [FleetImportConflict { reason: FleetImportRefusal::ImportConflict { existing: ExistingFleetImport::Imported(_) }, .. } ])),
            "{field} must conflict"
        );
        assert!(decision.writes.is_empty() && decision.feed.is_empty());
    }
    Ok(())
}

#[test]
fn fleet_import_ordinary_record_missing_parents_and_name_conflicts_are_atomic_refusals()
-> TestResult {
    let original = bundle()?;
    let initial = decide_import(&original, &FleetSnapshot::empty())?;
    let mut snapshot = adopted(&initial);
    snapshot.imports.clear();
    let ordinary = decide_import(&original, &snapshot)?;
    assert!(
        matches!(ordinary.result, FleetImportResult::Refused { ref conflicts } if conflicts.len() == 3 && conflicts.iter().all(|conflict| matches!(conflict.reason, FleetImportRefusal::ImportConflict { existing: ExistingFleetImport::NotImported })))
    );
    assert!(ordinary.writes.is_empty() && ordinary.feed.is_empty());

    let mut missing = original.clone();
    missing
        .records
        .retain(|record| !matches!(record, FleetImportRecord::Workspace { .. }));
    let refused = decide_import(&missing, &FleetSnapshot::empty())?;
    assert!(
        matches!(refused.result, FleetImportResult::Refused { ref conflicts } if conflicts.len() == 2 && conflicts.iter().all(|conflict| matches!(conflict.reason, FleetImportRefusal::ParentMissing { .. })))
    );
    assert!(refused.writes.is_empty() && refused.feed.is_empty());

    let mut collision = FleetSnapshot::empty();
    collision.names.push(FleetNameOwner {
        scope: FleetNameScope::Workspaces {},
        name: ResourceName::try_from("Original workspace".to_owned())?,
        record: workspace(100)?.into(),
    });
    let refused = decide_import(&original, &collision)?;
    let name_conflict = FleetImportRefusal::NameConflict {
        existing: workspace(100)?.into(),
    };
    assert!(
        matches!(refused.result, FleetImportResult::Refused { ref conflicts } if conflicts.iter().any(|conflict| conflict.reason == name_conflict))
    );
    assert!(refused.writes.is_empty() && refused.feed.is_empty());

    let mut partial = original;
    partial
        .records
        .retain(|record| !matches!(record, FleetImportRecord::Directory { .. }));
    for record in &mut partial.records {
        if let FleetImportRecord::Board {
            workspace: parent, ..
        } = record
        {
            *parent = workspace(999)?;
        }
    }
    let refused = decide_import(&partial, &FleetSnapshot::empty())?;
    assert!(
        matches!(refused.result, FleetImportResult::Refused { ref conflicts } if conflicts.len() == 1)
    );
    assert!(
        refused.writes.is_empty() && refused.feed.is_empty(),
        "a valid workspace must not be partly imported beside a refused board"
    );
    Ok(())
}

#[test]
fn fleet_import_read_set_names_records_parents_names_and_digest_keys() -> TestResult {
    let offered = bundle()?;
    let reads = reads_for_import(&offered);
    for record in &offered.records {
        assert!(reads.keys.contains(&FleetReadKey::Record {
            record: record.reference()
        }));
        assert!(reads.keys.contains(&FleetReadKey::ImportDigest {
            record: record.reference()
        }));
    }
    assert!(reads.keys.contains(&FleetReadKey::FeedHighWater {}));
    assert!(reads.keys.contains(&FleetReadKey::Name {
        scope: FleetNameScope::BoardProjects(board(2)?),
        name: ResourceName::try_from("Original project".to_owned())?
    }));
    Ok(())
}

#[test]
fn fleet_import_in_bundle_scoped_name_collisions_refuse_every_write() -> TestResult {
    let mut offered = bundle()?;
    let new_board = board(4)?;
    let board_twin = offered
        .records
        .iter()
        .find_map(|record| match record {
            FleetImportRecord::Board {
                workspace,
                name,
                description,
                created_at,
                ..
            } => Some(FleetImportRecord::Board {
                id: new_board.clone(),
                workspace: workspace.clone(),
                name: name.clone(),
                description: description.clone(),
                state: ImportedBoardState::Active,
                created_at: *created_at,
            }),
            _ => None,
        })
        .ok_or("board fixture")?;
    let project_twin = FleetImportRecord::Directory {
        project: project(5)?,
        board: board(2)?,
        title: ResourceName::try_from("Original project".to_owned())?,
        created_at: "2020-01-02T03:04:05Z".parse()?,
    };
    offered.records.extend([board_twin, project_twin]);
    let decision = decide_import(&offered, &FleetSnapshot::empty())?;
    let expected = [
        FleetImportConflict {
            record: board(4)?.into(),
            reason: FleetImportRefusal::NameConflict {
                existing: board(2)?.into(),
            },
        },
        FleetImportConflict {
            record: project(5)?.into(),
            reason: FleetImportRefusal::NameConflict {
                existing: project(3)?.into(),
            },
        },
    ];
    assert!(
        matches!(decision.result, FleetImportResult::Refused { ref conflicts } if conflicts.as_slice() == expected)
    );
    assert!(decision.writes.is_empty() && decision.feed.is_empty());
    Ok(())
}
