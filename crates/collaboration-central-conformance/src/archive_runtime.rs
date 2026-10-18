//! Authenticated report frames and exact public Fleet outcomes, on either shell.
use crate::{
    ConformanceClient, ConformanceResult, HolderSession,
    fleet_directory_fixture::import_identity,
    fleet_request_scenarios::{fleet_records, write},
    holder_fixtures::*,
    import_fixtures::{ImportedProjectFixture, attempt, claim_request},
    registry_rpc_driver::future_time,
};
use collaboration_central_core::RegistryTime;
use collaboration_replication_protocol::{
    BoardId as WireBoardId, ProjectId as WireProjectId, SessionAnswer, SessionRequest,
};
use fleet_admission::*;
use message_board::*;

pub(crate) async fn snapshot(socket: &mut HolderSession) -> ConformanceResult<(Vec<Record>, u64)> {
    let (records, through) = fleet_records(socket).await?;
    Ok((
        records
            .into_iter()
            .map(|record| serde_json::from_value(record.into_value()))
            .collect::<Result<_, _>>()?,
        through,
    ))
}

pub(crate) fn archive_request(
    holder: &HolderFixture,
    identity: &DirectoryEntry,
) -> ConformanceResult<FleetRequest> {
    Ok(FleetRequest {
        request_id: RequestId::try_from(uuid::Uuid::now_v7().to_string())?,
        workspace: identity.workspace.clone(),
        actor: RecordedActor {
            actor: ActorKind::Declared(Identity::Human {
                human_id: HumanId::try_from("archive requester".to_owned())?,
            }),
            installation: ServiceId::try_from(holder.installation.as_str().to_owned())?,
            machine: MachineLabel::try_from("archive fixture machine".to_owned())?,
        },
        acting_for: None,
        resubmits: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation: FleetOperation::WorkspaceArchive {},
    })
}

pub(crate) fn finalized_records(
    before: &[Record],
    identity: &DirectoryEntry,
    first_position: u64,
    now: RegistryTime,
) -> ConformanceResult<Vec<Record>> {
    let mut records = before.to_vec();
    let mut board_found = false;
    let mut workspace_found = false;
    for record in &mut records {
        let final_position = match record {
            Record::Board(board) if board.board_id == identity.board => {
                check(
                    board.state == BoardState::Archiving,
                    "original board is Archiving",
                )?;
                board.state = BoardState::Archived;
                board_found = true;
                Some(first_position)
            }
            Record::Workspace(workspace) if workspace.workspace_id == identity.workspace => {
                check(
                    workspace.state == WorkspaceState::Archiving,
                    "original workspace is Archiving",
                )?;
                workspace.state = WorkspaceState::Archived;
                workspace_found = true;
                Some(first_position + 1)
            }
            _ => None,
        };
        if let Some(position) = final_position {
            let envelope = record.envelope_mut();
            envelope.position = RecordPosition::Fleet(FleetPosition::try_from(position)?);
            envelope.request = None;
            envelope.router_time = now.0;
        }
    }
    check(
        board_found && workspace_found,
        "both complete typed Archiving records present",
    )?;
    Ok(records)
}

pub(crate) fn report_outcome(answer: SessionAnswer) -> ConformanceResult<ArchiveReportOutcome> {
    let SessionAnswer::ArchiveReport(value) = answer else {
        return Err(format!("typed automated report result: {answer:?}").into());
    };
    Ok(serde_json::from_value(value.into_value())?)
}

fn report(
    board: &BoardId,
    project: &WireProjectId,
    epoch_number: u64,
) -> ConformanceResult<SessionRequest> {
    Ok(SessionRequest::ArchiveSeen {
        board: WireBoardId::new(board.as_str())?,
        project: project.clone(),
        position: position(epoch_number, 3)?,
    })
}

pub async fn run_archive_report_conformance(client: &ConformanceClient) -> ConformanceResult<()> {
    let at = future_time()?;
    client.set_time(at).await?;
    let first = ImportedProjectFixture::new()?;
    let second = ImportedProjectFixture::new()?;
    let identity = import_identity(client, &first.project).await?;
    equal(
        client
            .import_fleet(&FleetImportBundle {
                provenance: ImportProvenance {
                    installation: ServiceId::try_from(
                        "00000000-0000-4000-8000-000000000001".to_owned(),
                    )?,
                    machine: MachineLabel::try_from("archive original computer".to_owned())?,
                },
                records: vec![FleetImportRecord::Directory {
                    project: ProjectId::try_from(second.project.as_str().to_owned())?,
                    board: identity.board.clone(),
                    title: ResourceName::try_from("Second archive project".to_owned())?,
                    created_at: at.0,
                }],
            })
            .await?,
        FleetImportResult::Imported {
            counts: FleetImportCounts {
                workspaces: 0,
                boards: 0,
                directory_entries: 1,
            },
        },
    )?;
    first.upload(client).await?;
    second.upload(client).await?;
    let holder = HolderFixture::enroll(client, "archive holder").await?;
    let successor = HolderFixture::enroll(client, "archive successor").await?;
    let (mut socket, _) = holder.connect(client).await?;
    let (mut other_socket, _) = successor.connect(client).await?;
    for fixture in [&first, &second] {
        equal(
            socket
                .request(claim_request(&fixture.project, attempt()?))
                .await?,
            SessionAnswer::Claimed {
                epoch: epoch(2)?,
                expires_at: at.0 + chrono::TimeDelta::hours(4),
                start: position(1, 3)?,
            },
        )?;
        equal(fixture.state(client).await?.fence, epoch(2)?)?;
    }
    let request = archive_request(&holder, &identity)?;
    let original_decision = write(&mut socket, &request).await?;
    check(
        matches!(original_decision, RequestDecision::Accepted { .. }),
        "ordinary workspace archive accepted",
    )?;
    let before = snapshot(&mut socket).await?;
    check(before.0.iter().any(|record| matches!(record, Record::Board(board) if board.board_id == identity.board && board.state == BoardState::Archiving)), "both held projects keep board Archiving")?;
    let missing = project()?;
    equal(
        report_outcome(
            socket
                .request(report(&identity.board, &missing, 1)?)
                .await?,
        )?,
        ArchiveReportOutcome::Refused(ArchiveReportRefusal::NotFound(
            ProjectId::try_from(missing.as_str().to_owned())?.into(),
        )),
    )?;
    equal(snapshot(&mut socket).await?, before.clone())?;
    equal(
        report_outcome(
            socket
                .request(report(&identity.board, &first.project, 1)?)
                .await?,
        )?,
        ArchiveReportOutcome::Recorded,
    )?;
    equal(snapshot(&mut socket).await?, before.clone())?;
    equal(
        report_outcome(
            other_socket
                .request(report(&identity.board, &first.project, 9)?)
                .await?,
        )?,
        ArchiveReportOutcome::AlreadyRecorded,
    )?;
    equal(snapshot(&mut socket).await?, before.clone())?;
    let final_time = RegistryTime(at.0 + chrono::TimeDelta::seconds(3));
    client.set_time(final_time).await?;
    equal(
        report_outcome(
            socket
                .request(report(&identity.board, &second.project, 1)?)
                .await?,
        )?,
        ArchiveReportOutcome::Recorded,
    )?;
    let expected = (
        finalized_records(&before.0, &identity, before.1 + 1, final_time)?,
        before.1 + 2,
    );
    equal(snapshot(&mut socket).await?, expected.clone())?;
    equal(
        report_outcome(
            other_socket
                .request(report(&identity.board, &second.project, 9)?)
                .await?,
        )?,
        ArchiveReportOutcome::AlreadyRecorded,
    )?;
    equal(snapshot(&mut socket).await?, expected.clone())?;
    equal(write(&mut socket, &request).await?, original_decision)?;
    equal(snapshot(&mut socket).await?, expected)?;
    socket.close().await?;
    other_socket.close().await?;
    println!(
        "ARCHIVE real sessions: held imports, parent-gate report exemption, typed refusal, partial/successor repeat no-op, complete exact attributed board/workspace records and two feed positions, unchanged original ledger: passed"
    );
    Ok(())
}
