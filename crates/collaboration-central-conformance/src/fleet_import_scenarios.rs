use crate::{ConformanceClient, ConformanceResult, HolderSession, holder_fixtures::*};
use collaboration_replication_protocol::{ReadPage, ReadScope, SessionAnswer, SessionRequest};
use fleet_admission::*;
use message_board::{
    BoardId, BoardKind, BoardState, Description, HumanId, Identity, MachineLabel, NormalizedOrigin,
    Record, RepositoryRef, ResourceName, ServiceId, WorkspaceId,
};

fn original_bundle() -> ConformanceResult<FleetImportBundle> {
    let workspace = WorkspaceId::try_from(uuid::Uuid::now_v7().to_string())?;
    let board = BoardId::try_from(uuid::Uuid::now_v7().to_string())?;
    Ok(FleetImportBundle {
        provenance: ImportProvenance {
            installation: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
            machine: MachineLabel::try_from("original import computer".to_owned())?,
        },
        records: vec![
            FleetImportRecord::Board {
                id: board.clone(),
                workspace: workspace.clone(),
                name: ResourceName::try_from("Original archived board".to_owned())?,
                description: Description::try_from("Original board description".to_owned())?,
                state: ImportedBoardState::Archived,
                created_at: "2020-01-02T03:04:06Z".parse()?,
            },
            FleetImportRecord::Workspace {
                id: workspace,
                name: ResourceName::try_from(format!(
                    "Original workspace {}",
                    uuid::Uuid::now_v7()
                ))?,
                description: Description::try_from("Original workspace description".to_owned())?,
                repositories: vec![RepositoryRef::Origin {
                    normalized_origin: NormalizedOrigin::try_from(
                        "example.com/owner/original-repository".to_owned(),
                    )?,
                }],
                created_at: "2020-01-02T03:04:05Z".parse()?,
            },
            FleetImportRecord::Directory {
                project: message_board::ProjectId::try_from(uuid::Uuid::now_v7().to_string())?,
                board,
                title: ResourceName::try_from("Original directory project".to_owned())?,
                created_at: "2020-01-02T03:04:07Z".parse()?,
            },
        ],
    })
}
async fn records(socket: &mut HolderSession) -> ConformanceResult<(Vec<Record>, u64)> {
    let SessionAnswer::ReadPage(ReadPage::FleetRecords {
        records, position, ..
    }) = socket
        .request(SessionRequest::Read {
            scope: ReadScope::FleetRecords,
            after: None,
        })
        .await?
    else {
        return Err("real Fleet records read".into());
    };
    Ok((
        records
            .into_iter()
            .map(|record| serde_json::from_value(record.into_value()))
            .collect::<Result<_, _>>()?,
        position.value(),
    ))
}
pub async fn run_fleet_import_conformance(client: &ConformanceClient) -> ConformanceResult<()> {
    if client.staging_access().is_some() {
        return Err("Fleet import scenario currently owns local fresh fixtures only".into());
    }
    let machine = HolderFixture::enroll(client, "fleet-import").await?;
    let (mut socket, projects) = machine.connect(client).await?;
    check(projects.is_empty(), "fresh authenticated session control")?;
    let offered = original_bundle()?;
    let before = records(&mut socket).await?;
    equal(
        client.import_fleet(&offered).await?,
        FleetImportResult::Imported {
            counts: FleetImportCounts {
                workspaces: 1,
                boards: 1,
                directory_entries: 1,
            },
        },
    )?;
    let (imported, head) = records(&mut socket).await?;
    equal(head, before.1 + 3)?;
    for input in &offered.records {
        let stored = imported
            .iter()
            .find(|record| record.reference() == input.reference())
            .ok_or("original ID missing from actual Fleet")?;
        equal(
            stored.envelope().actor.actor.clone(),
            message_board::ActorKind::Declared(Identity::Human {
                human_id: HumanId::try_from("owner".to_owned())?,
            }),
        )?;
        equal(
            stored.envelope().actor.installation.clone(),
            offered.provenance.installation.clone(),
        )?;
        equal(
            stored.envelope().actor.machine.clone(),
            offered.provenance.machine.clone(),
        )?;
        equal(stored.envelope().request.clone(), None)?;
        match (input, stored) {
            (
                FleetImportRecord::Workspace {
                    id,
                    name,
                    description,
                    repositories,
                    created_at,
                },
                Record::Workspace(value),
            ) => {
                equal(&value.workspace_id, id)?;
                equal(&value.name, name)?;
                equal(&value.description, description)?;
                equal(&value.repositories, repositories)?;
                equal(&value.envelope.router_time, created_at)?;
            }
            (
                FleetImportRecord::Board {
                    id,
                    workspace,
                    name,
                    description,
                    created_at,
                    ..
                },
                Record::Board(value),
            ) => {
                equal(&value.board_id, id)?;
                equal(&value.workspace_id, workspace)?;
                equal(&value.name, name)?;
                equal(&value.description, description)?;
                equal(value.state, BoardState::Archived)?;
                equal(&value.kind, &BoardKind::Area {})?;
                equal(&value.envelope.router_time, created_at)?;
            }
            (
                FleetImportRecord::Directory {
                    project,
                    board,
                    title,
                    created_at,
                },
                Record::DirectoryEntry(value),
            ) => {
                equal(&value.id, project)?;
                equal(&value.board, board)?;
                equal(&value.title, title)?;
                equal(&value.envelope.router_time, created_at)?;
            }
            _ => return Err("actual typed Fleet record differs from import".into()),
        }
    }
    let mut replay = offered.clone();
    replay.provenance.machine = MachineLabel::try_from("second import computer".to_owned())?;
    replay.provenance.installation =
        ServiceId::try_from("00000000-0000-4000-8000-000000000002".to_owned())?;
    replay.records.reverse();
    equal(
        client.import_fleet(&replay).await?,
        FleetImportResult::AlreadyImported,
    )?;
    equal(records(&mut socket).await?, (imported.clone(), head))?;

    let mut conflict = offered.clone();
    for record in &mut conflict.records {
        if let FleetImportRecord::Board { description, .. } = record {
            *description = Description::try_from("Changed description".to_owned())?;
        }
    }
    check(
        matches!(client.import_fleet(&conflict).await?, FleetImportResult::Refused { conflicts } if matches!(conflicts.as_slice(), [FleetImportConflict { reason: FleetImportRefusal::ImportConflict { existing: ExistingFleetImport::Imported(_) }, .. }])),
        "imported digest conflict",
    )?;
    equal(records(&mut socket).await?, (imported.clone(), head))?;

    let mut partial = original_bundle()?;
    partial
        .records
        .retain(|record| !matches!(record, FleetImportRecord::Directory { .. }));
    for record in &mut partial.records {
        if let FleetImportRecord::Board { workspace, .. } = record {
            *workspace = WorkspaceId::try_from(uuid::Uuid::now_v7().to_string())?;
        }
    }
    check(
        matches!(client.import_fleet(&partial).await?, FleetImportResult::Refused { conflicts } if matches!(conflicts.as_slice(), [FleetImportConflict { reason: FleetImportRefusal::ParentMissing { .. }, .. }])),
        "valid workspace plus missing board parent refuses entire transaction",
    )?;
    equal(records(&mut socket).await?, (imported.clone(), head))?;

    let mut collision = offered.clone();
    collision
        .records
        .retain(|record| matches!(record, FleetImportRecord::Workspace { .. }));
    for record in &mut collision.records {
        if let FleetImportRecord::Workspace { id, .. } = record {
            *id = WorkspaceId::try_from(uuid::Uuid::now_v7().to_string())?;
        }
    }
    check(
        matches!(client.import_fleet(&collision).await?, FleetImportResult::Refused { conflicts } if matches!(conflicts.as_slice(), [FleetImportConflict { reason: FleetImportRefusal::NameConflict { .. }, .. }])),
        "real Fleet name conflict",
    )?;
    equal(records(&mut socket).await?, (imported.clone(), head))?;
    let denied = client
        .with_machine_access(
            client
                .client
                .post(format!("{}/admin/v1/import/fleet", client.base_url))
                .bearer_auth(&machine.token),
            &machine.token,
        )?
        .json(&offered)
        .send()
        .await?;
    equal(denied.status(), reqwest::StatusCode::UNAUTHORIZED)?;
    equal(
        denied
            .json::<collaboration_central_core::AccessRefusal>()
            .await?,
        collaboration_central_core::AccessRefusal::WrongAudience,
    )?;
    equal(records(&mut socket).await?, (imported, head))?;
    let ordinary_workspace = WorkspaceId::try_from(uuid::Uuid::now_v7().to_string())?;
    let ordinary_name =
        ResourceName::try_from(format!("ordinary import conflict {}", uuid::Uuid::now_v7()))?;
    let ordinary = FleetRequest {
        request_id: message_board::RequestId::try_from(uuid::Uuid::now_v7().to_string())?,
        workspace: ordinary_workspace.clone(),
        actor: message_board::RecordedActor {
            actor: message_board::ActorKind::Declared(Identity::Human {
                human_id: HumanId::try_from("owner".to_owned())?,
            }),
            installation: ServiceId::try_from(machine.installation.as_str().to_owned())?,
            machine: MachineLabel::try_from("fleet-import".to_owned())?,
        },
        acting_for: None,
        resubmits: None,
        protocol_version: message_board::ProtocolVersion::CURRENT,
        operation: FleetOperation::WorkspaceCreate {
            name: ordinary_name.clone(),
            description: Description::try_from("ordinary initial content".to_owned())?,
        },
    };
    check(
        matches!(
            crate::fleet_request_scenarios::write(&mut socket, &ordinary).await?,
            message_board::RequestDecision::Accepted { .. }
        ),
        "ordinary creation before NotImported conflict",
    )?;
    let ordinary_before = records(&mut socket).await?;
    let import_over_ordinary = FleetImportBundle {
        provenance: offered.provenance,
        records: vec![FleetImportRecord::Workspace {
            id: ordinary_workspace.clone(),
            name: ordinary_name,
            description: Description::try_from("ordinary initial content".to_owned())?,
            repositories: Vec::new(),
            created_at: "2020-01-02T03:04:05Z".parse()?,
        }],
    };
    equal(
        client.import_fleet(&import_over_ordinary).await?,
        FleetImportResult::Refused {
            conflicts: vec![FleetImportConflict {
                record: ordinary_workspace.into(),
                reason: FleetImportRefusal::ImportConflict {
                    existing: ExistingFleetImport::NotImported,
                },
            }],
        },
    )?;
    equal(records(&mut socket).await?, ordinary_before)?;
    socket.close().await?;
    println!(
        "FLEET import owner route: exact legacy IDs/names/descriptions/repositories/state/time/provenance, other-computer replay, digest/name/parent refusals, all-or-nothing, machine denial: passed"
    );
    Ok(())
}
