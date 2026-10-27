use crate::{
    ExistingFleetImport, FleetAdmissionError, FleetImportBundle, FleetImportConflict,
    FleetImportCounts, FleetImportDecision, FleetImportRecord, FleetImportRefusal,
    FleetImportResult, FleetImportWrite, FleetNameOwner, FleetNameScope, FleetReadKey, FleetReads,
    FleetSnapshot, ImportedBoardState, ImportedFleetRecord,
};
use message_board::{
    ActorOrigin, Board, BoardKind, BoardState, FleetPosition, Identity, ProjectDirectoryEntry,
    Record, RecordEnvelope, RecordPlacement, RecordRef, RecordScope, RecordedActor, Workspace,
    WorkspaceState,
};

#[must_use]
pub fn reads_for_import(bundle: &FleetImportBundle) -> FleetReads {
    let mut keys = vec![FleetReadKey::FeedHighWater {}];
    for record in &bundle.records {
        for key in [
            FleetReadKey::Record {
                record: record.reference(),
            },
            FleetReadKey::ImportDigest {
                record: record.reference(),
            },
            name_key(record),
        ] {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        if let Some(parent) = parent(record) {
            let key = FleetReadKey::Record { record: parent };
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
    }
    FleetReads { keys }
}

pub fn decide_import(
    bundle: &FleetImportBundle,
    snapshot: &FleetSnapshot,
) -> Result<FleetImportDecision, FleetAdmissionError> {
    let origin = ActorOrigin {
        installation: bundle.provenance.installation.clone(),
        machine: bundle.provenance.machine.clone(),
    };
    let actor = RecordedActor::declared(Identity::Human {
        human_id: bundle.declared_owner.clone(),
    });
    let mut offered = bundle.records.iter().collect::<Vec<_>>();
    offered.sort_by_key(|record| (rank(record), record.reference().id.as_str().to_owned()));
    let mut candidate = snapshot.clone();
    let mut writes = Vec::new();
    let mut conflicts = Vec::new();
    for record in offered {
        let reference = record.reference();
        let digest = record.digest()?;
        let imported = candidate
            .imports
            .iter()
            .find(|stored| stored.record == reference);
        if candidate.record(&reference).is_some() {
            match imported {
                Some(stored) if stored.digest == digest => {}
                existing => conflicts.push(FleetImportConflict {
                    record: reference,
                    reason: FleetImportRefusal::ImportConflict {
                        existing: existing.map_or(ExistingFleetImport::NotImported, |stored| {
                            ExistingFleetImport::Imported(stored.digest.clone())
                        }),
                    },
                }),
            }
            continue;
        }
        if imported.is_some() {
            return Err(FleetAdmissionError::InvalidSnapshot);
        }
        let built = build_record(record, &actor, &candidate, &origin);
        let built = match built {
            Ok(built) => built,
            Err(reason) => {
                conflicts.push(FleetImportConflict {
                    record: reference,
                    reason,
                });
                continue;
            }
        };
        let FleetReadKey::Name { scope, name } = name_key(record) else {
            return Err(FleetAdmissionError::InvalidSnapshot);
        };
        if let Some(existing) = candidate
            .names
            .iter()
            .find(|owner| owner.scope == scope && owner.name == name)
        {
            conflicts.push(FleetImportConflict {
                record: reference,
                reason: FleetImportRefusal::NameConflict {
                    existing: existing.record.clone(),
                },
            });
            continue;
        }
        candidate.names.push(FleetNameOwner {
            scope,
            name,
            record: reference.clone(),
        });
        candidate.records.push(built.clone());
        candidate.imports.push(ImportedFleetRecord {
            record: reference,
            digest: digest.clone(),
        });
        writes.push(FleetImportWrite {
            record: built,
            digest,
        });
    }
    if !conflicts.is_empty() {
        return Ok(FleetImportDecision {
            result: FleetImportResult::Refused { conflicts },
            writes: Vec::new(),
            feed: Vec::new(),
        });
    }
    if writes.is_empty() {
        return Ok(FleetImportDecision {
            result: FleetImportResult::AlreadyImported,
            writes,
            feed: Vec::new(),
        });
    }
    let mut counts = FleetImportCounts::default();
    let mut position = snapshot.high_water;
    let mut feed = Vec::with_capacity(writes.len());
    for write in &mut writes {
        position = position.successor()?;
        write.record.envelope_mut().position =
            RecordPlacement::Fleet(message_board::FleetFeedEntry {
                position,
                origin: origin.clone(),
            });
        match &write.record {
            Record::Workspace(_) => counts.workspaces += 1,
            Record::Board(_) => counts.boards += 1,
            Record::DirectoryEntry(_) => counts.directory_entries += 1,
            _ => return Err(FleetAdmissionError::InvalidSnapshot),
        }
        feed.push(crate::FleetFeedEntry {
            position,
            record: write.record.clone(),
        });
    }
    Ok(FleetImportDecision {
        result: FleetImportResult::Imported { counts },
        writes,
        feed,
    })
}

fn rank(record: &FleetImportRecord) -> u8 {
    match record {
        FleetImportRecord::Workspace { .. } => 0,
        FleetImportRecord::Board { .. } => 1,
        FleetImportRecord::Directory { .. } => 2,
    }
}
fn parent(record: &FleetImportRecord) -> Option<RecordRef> {
    match record {
        FleetImportRecord::Workspace { .. } => None,
        FleetImportRecord::Board { workspace, .. } => Some(workspace.clone().into()),
        FleetImportRecord::Directory { board, .. } => Some(board.clone().into()),
    }
}
fn name_key(record: &FleetImportRecord) -> FleetReadKey {
    let (scope, name) = match record {
        FleetImportRecord::Workspace { name, .. } => (FleetNameScope::Workspaces {}, name.clone()),
        FleetImportRecord::Board {
            workspace, name, ..
        } => (
            FleetNameScope::WorkspaceBoards(workspace.clone()),
            name.clone(),
        ),
        FleetImportRecord::Directory { board, title, .. } => {
            (FleetNameScope::BoardProjects(board.clone()), title.clone())
        }
    };
    FleetReadKey::Name { scope, name }
}

fn build_record(
    offered: &FleetImportRecord,
    actor: &RecordedActor,
    snapshot: &FleetSnapshot,
    origin: &ActorOrigin,
) -> Result<Record, FleetImportRefusal> {
    let reference = offered.reference();
    let missing = |parent: RecordRef| FleetImportRefusal::ParentMissing { parent };
    let (workspace, board, time) = match offered {
        FleetImportRecord::Workspace { id, time, .. } => (id.clone(), None, *time),
        FleetImportRecord::Board {
            id,
            workspace,
            time,
            ..
        } => {
            let parent = workspace.clone().into();
            if !matches!(snapshot.record(&parent), Some(Record::Workspace(_))) {
                return Err(missing(parent));
            }
            (workspace.clone(), Some(id.clone()), *time)
        }
        FleetImportRecord::Directory { board, time, .. } => {
            let parent = board.clone().into();
            let Some(Record::Board(stored)) = snapshot.record(&parent) else {
                return Err(missing(parent));
            };
            (stored.workspace_id.clone(), Some(board.clone()), *time)
        }
    };
    let envelope = RecordEnvelope {
        id: reference.id,
        scope: RecordScope {
            workspace,
            board,
            project: None,
        },
        actor: actor.clone(),
        request: None,
        position: RecordPlacement::Fleet(message_board::FleetFeedEntry {
            position: FleetPosition::ZERO,
            origin: origin.clone(),
        }),
        time,
    };
    Ok(match offered {
        FleetImportRecord::Workspace {
            id,
            name,
            description,
            repositories,
            ..
        } => Record::Workspace(Workspace {
            envelope,
            workspace_id: id.clone(),
            name: name.clone(),
            description: description.clone(),
            state: WorkspaceState::Active,
            repositories: repositories.clone(),
        }),
        FleetImportRecord::Board {
            id,
            workspace,
            name,
            description,
            state,
            ..
        } => Record::Board(Board {
            envelope,
            board_id: id.clone(),
            workspace_id: workspace.clone(),
            name: name.clone(),
            description: description.clone(),
            kind: BoardKind::Area {},
            state: match state {
                ImportedBoardState::Active => BoardState::Open,
                ImportedBoardState::Archived => BoardState::Archived,
            },
        }),
        FleetImportRecord::Directory {
            project,
            board,
            title,
            ..
        } => Record::DirectoryEntry(ProjectDirectoryEntry {
            envelope,
            id: project.clone(),
            board: board.clone(),
            title: title.clone(),
        }),
    })
}
