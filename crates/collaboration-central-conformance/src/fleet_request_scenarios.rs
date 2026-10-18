use crate::{ConformanceClient, ConformanceResult, HolderSession, holder_fixtures::*};
use collaboration_replication_protocol::{
    FleetPayload, FleetRequestEnvelope, ReadPage, ReadScope, SessionAnswer, SessionRequest,
};
use fleet_admission::*;
use message_board::{
    ActorKind, BoardId, BoardKind, ChangeOutcome, Description, HumanId, Identity, MachineLabel,
    ProtocolVersion, Record, RecordedActor, RequestDecision, RequestId, ResourceName, ServiceId,
    WorkspaceId,
};

pub(crate) fn envelope(request: &FleetRequest) -> ConformanceResult<FleetRequestEnvelope> {
    Ok(FleetRequestEnvelope {
        request_id: collaboration_replication_protocol::RequestId::new(
            request.request_id.as_str(),
        )?,
        operation: FleetPayload::new(serde_json::to_value(request)?),
    })
}
pub(crate) async fn write(
    socket: &mut HolderSession,
    request: &FleetRequest,
) -> ConformanceResult<RequestDecision> {
    let SessionAnswer::FleetDecision {
        request_id,
        decision,
    } = socket
        .request(SessionRequest::Fleet(envelope(request)?))
        .await?
    else {
        return Err("typed Fleet decision".into());
    };
    equal(request_id.as_str(), request.request_id.as_str())?;
    let FleetRequestResult::Decided(decision) = serde_json::from_value(decision.into_value())?
    else {
        return Err("expected decided Fleet result".into());
    };
    Ok(decision)
}

pub(crate) async fn fleet_records(
    socket: &mut HolderSession,
) -> ConformanceResult<(Vec<FleetPayload>, u64)> {
    let SessionAnswer::ReadPage(ReadPage::FleetRecords {
        records, position, ..
    }) = socket
        .request(SessionRequest::Read {
            scope: ReadScope::FleetRecords,
            after: None,
        })
        .await?
    else {
        return Err("Fleet state control".into());
    };
    Ok((records, position.value()))
}

pub async fn run_fleet_conflict_conformance(client: &ConformanceClient) -> ConformanceResult<()> {
    client.set_time(time()?).await?;
    let machine = HolderFixture::enroll(client, "fleet conflict").await?;
    let (mut socket, _) = machine.connect(client).await?;
    let create = FleetRequest {
        request_id: RequestId::try_from(uuid::Uuid::now_v7().to_string())?,
        workspace: WorkspaceId::try_from(uuid::Uuid::now_v7().to_string())?,
        actor: RecordedActor {
            actor: ActorKind::Declared(Identity::Human {
                human_id: HumanId::try_from("owner".to_owned())?,
            }),
            installation: ServiceId::try_from(machine.installation.as_str().to_owned())?,
            machine: MachineLabel::try_from("fleet conflict".to_owned())?,
        },
        acting_for: None,
        resubmits: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation: FleetOperation::WorkspaceCreate {
            name: ResourceName::try_from(format!("conflict {}", uuid::Uuid::now_v7()))?,
            description: Description::try_from("original exact content".to_owned())?,
        },
    };
    let original = write(&mut socket, &create).await?;
    accepted(&original, ChangeOutcome::Changed)?;
    let before = fleet_records(&mut socket).await?;
    equal(write(&mut socket, &create).await?, original.clone())?;
    equal(fleet_records(&mut socket).await?, before.clone())?;
    println!(
        "FLEET conflict positive controls: accepted create and identical replay, exact unchanged records/feed before changed request"
    );
    let mut changed = create.clone();
    changed.operation = FleetOperation::WorkspaceUpdate {
        name: ResourceName::try_from("conflicting content".to_owned())?,
        description: Description::try_from("changed".to_owned())?,
    };
    let SessionAnswer::FleetDecision {
        request_id,
        decision,
    } = socket
        .request(SessionRequest::Fleet(envelope(&changed)?))
        .await?
    else {
        return Err("exact Fleet conflict answer".into());
    };
    equal(request_id.as_str(), create.request_id.as_str())?;
    equal(
        serde_json::from_value::<FleetRequestResult>(decision.into_value())?,
        FleetRequestResult::ConflictingRequest,
    )?;
    equal(fleet_records(&mut socket).await?, before.clone())?;
    equal(write(&mut socket, &create).await?, original)?;
    equal(fleet_records(&mut socket).await?, before)?;
    socket.close().await?;
    println!(
        "FLEET conflict: positive create, original replay, changed content typed ConflictingRequest, exact records/feed position unchanged, original still replays: passed"
    );
    Ok(())
}
fn accepted(decision: &RequestDecision, change: ChangeOutcome) -> ConformanceResult<()> {
    check(
        matches!(decision, RequestDecision::Accepted { change: actual, .. } if *actual == change),
        "literal Fleet acceptance/change",
    )
}
pub async fn run_fleet_request_conformance(client: &ConformanceClient) -> ConformanceResult<()> {
    client.set_time(time()?).await?;
    let machine = HolderFixture::enroll(client, "fleet ordinary").await?;
    let (mut socket, _) = machine.connect(client).await?;
    let workspace = WorkspaceId::try_from(uuid::Uuid::now_v7().to_string())?;
    let board = BoardId::try_from(uuid::Uuid::now_v7().to_string())?;
    let project = project()?;
    let request = |operation| -> ConformanceResult<FleetRequest> {
        Ok(FleetRequest {
            request_id: RequestId::try_from(uuid::Uuid::now_v7().to_string())?,
            workspace: workspace.clone(),
            actor: RecordedActor {
                actor: ActorKind::Declared(Identity::Human {
                    human_id: HumanId::try_from("owner".to_owned())?,
                }),
                installation: ServiceId::try_from(machine.installation.as_str().to_owned())?,
                machine: MachineLabel::try_from("fleet ordinary".to_owned())?,
            },
            acting_for: None,
            resubmits: None,
            protocol_version: ProtocolVersion::CURRENT,
            operation,
        })
    };
    let create_workspace = request(FleetOperation::WorkspaceCreate {
        name: ResourceName::try_from(format!("ordinary {workspace:?}"))?,
        description: Description::try_from("ordinary description".to_owned())?,
    })?;
    let first = write(&mut socket, &create_workspace).await?;
    accepted(&first, ChangeOutcome::Changed)?;
    equal(write(&mut socket, &create_workspace).await?, first)?;
    accepted(
        &write(
            &mut socket,
            &request(FleetOperation::BoardCreate {
                board: board.clone(),
                name: ResourceName::try_from("ordinary board".to_owned())?,
                description: Description::try_from("ordinary board description".to_owned())?,
                board_kind: BoardKind::Area {},
            })?,
        )
        .await?,
        ChangeOutcome::Changed,
    )?;
    let create_project = request(FleetOperation::ProjectDirectoryCreate {
        project: message_board::ProjectId::try_from(project.as_str().to_owned())?,
        board,
        title: ResourceName::try_from("ordinary project".to_owned())?,
    })?;
    accepted(
        &write(&mut socket, &create_project).await?,
        ChangeOutcome::Changed,
    )?;
    equal(
        socket
            .request(SessionRequest::Piece(piece(
                &project,
                position(1, 0)?,
                position(1, 1)?,
                vec![schema_transaction()],
            )?))
            .await?,
        SessionAnswer::Acked {
            through: position(1, 1)?,
        },
    )?;
    accepted(
        &write(&mut socket, &create_project).await?,
        ChangeOutcome::Changed,
    )?;
    let replayed_copy = socket
        .request(SessionRequest::Read {
            scope: ReadScope::ProjectCopy {
                project: project.clone(),
            },
            after: None,
        })
        .await?;
    check(
        matches!(replayed_copy,SessionAnswer::ReadPage(ReadPage::ProjectCopy(collaboration_replication_protocol::ProjectCopyRead::Page(ref page))) if page.position == position(1,1)? && page.table == "records" && page.rows.is_empty()),
        "creation replay must retain initialized schema/ack and same epoch",
    )?;
    equal(
        socket
            .request(SessionRequest::Piece(piece(
                &project,
                position(1, 1)?,
                position(1, 2)?,
                vec![rows(2, vec![row(1, "after creation replay")])],
            )?))
            .await?,
        SessionAnswer::Acked {
            through: position(1, 2)?,
        },
    )?;
    let SessionAnswer::ReadPage(ReadPage::FleetRecords { records, .. }) = socket
        .request(SessionRequest::Read {
            scope: ReadScope::FleetRecords,
            after: None,
        })
        .await?
    else {
        return Err("real Fleet records".into());
    };
    let records = records
        .into_iter()
        .map(|value| serde_json::from_value::<Record>(value.into_value()))
        .collect::<Result<Vec<_>, _>>()?;
    check(records.iter().any(|record| matches!(record, Record::DirectoryEntry(value) if value.id.as_str() == project.as_str() && value.title.as_str() == "ordinary project")), "accepted project persisted through actual Fleet")?;
    socket.close().await?;
    println!(
        "FLEET ordinary: real request create/replay, directory accepted before first epoch, authenticated installation writes copy: passed"
    );
    Ok(())
}
