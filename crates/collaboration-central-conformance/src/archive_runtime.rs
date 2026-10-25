//! Authenticated archive report frames and exact canonical Fleet outcomes.
use crate::{
    ConformanceClient, ConformanceResult,
    native_board_session::NativeBoardSession,
    scenario_assertions::{check, equal},
};
use collaboration_central_core::RegistryTime;
use fleet_admission::*;
use message_board::*;
pub(crate) fn finalized_records(
    before: &[Record],
    board_id: &BoardId,
    workspace_id: &WorkspaceId,
    first_position: u64,
    now: RegistryTime,
) -> ConformanceResult<Vec<Record>> {
    let mut records = before.to_vec();
    let mut board_found = false;
    let mut workspace_found = false;
    for record in &mut records {
        let final_position = match record {
            Record::Board(board) if board.board_id == *board_id => {
                check(
                    board.state == BoardState::Archiving,
                    "original board is Archiving",
                )?;
                board.state = BoardState::Archived;
                board_found = true;
                Some(first_position)
            }
            Record::Workspace(workspace) if workspace.workspace_id == *workspace_id => {
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
            let RecordPlacement::Fleet(entry) = &mut envelope.position else {
                return Err("archive source placement".into());
            };
            entry.position = FleetPosition::try_from(position)?;
            envelope.request = None;
            envelope.time = RecordTime::Router(now.0);
        }
    }
    check(
        board_found && workspace_found,
        "both complete typed Archiving records present",
    )?;
    Ok(records)
}

impl NativeBoardSession {
    async fn report(
        &mut self,
        board: &BoardId,
        project: &ProjectId,
        epoch: u64,
    ) -> ConformanceResult<ArchiveReportOutcome> {
        use board_sync_protocol::{ArchiveSeen, BoardConsumerAnswer, BoardConsumerRequest};
        let BoardConsumerAnswer::ArchiveSeen(payload) = self
            .request(BoardConsumerRequest::ArchiveSeen(ArchiveSeen {
                board: board.clone(),
                project: project.clone(),
                position: fleet_sync_protocol::ReplicationPosition::new(
                    fleet_sync_protocol::Epoch::new(epoch)?,
                    fleet_sync_protocol::CommitSequence::new(9_007_199_254_740_993),
                ),
            }))
            .await?
        else {
            return Err("native archive report did not return its typed outcome".into());
        };
        Ok(payload.decode()?)
    }
}

pub async fn run_archive_report_with_clock<UpdateClock, UpdateFuture>(
    client: &ConformanceClient,
    mut update_clock: UpdateClock,
) -> ConformanceResult<()>
where
    UpdateClock: FnMut(RegistryTime) -> UpdateFuture,
    UpdateFuture: std::future::Future<Output = ConformanceResult<()>>,
{
    let future = chrono::Utc::now() + chrono::TimeDelta::minutes(2);
    let at = RegistryTime(
        chrono::DateTime::from_timestamp_millis(future.timestamp_millis()).ok_or("fixture time")?,
    );
    update_clock(at).await?;
    let workspace = WorkspaceId::try_from(uuid::Uuid::now_v7().to_string())?;
    let board = BoardId::try_from(uuid::Uuid::now_v7().to_string())?;
    let first = ProjectId::try_from(uuid::Uuid::now_v7().to_string())?;
    let second = ProjectId::try_from(uuid::Uuid::now_v7().to_string())?;
    let mut sender = NativeBoardSession(
        crate::enroll_admitted_peer(
            client,
            "550e8400-e29b-41d4-a716-446655440af1",
            "019f0000-0000-7000-8000-000000001000",
            "native-archive-sender",
            &uuid::Uuid::now_v7().to_string(),
        )
        .await?,
    );
    let mut successor = NativeBoardSession(
        crate::enroll_admitted_peer(
            client,
            "550e8400-e29b-41d4-a716-446655440af2",
            "019f0000-0000-7000-8000-000000001001",
            "native-archive-successor",
            &uuid::Uuid::now_v7().to_string(),
        )
        .await?,
    );
    let proof = async {
        for operation in [
            FleetOperation::WorkspaceCreate { name:ResourceName::try_from(format!("Archive {}",workspace.as_str()))?, description:Description::try_from("native archive proof".to_owned())? },
            FleetOperation::BoardCreate { board:board.clone(),name:ResourceName::try_from("Archive board".to_owned())?,description:Description::try_from("native archive proof".to_owned())?,board_kind:BoardKind::Area {} },
            FleetOperation::ProjectDirectoryCreate { project:first.clone(),board:board.clone(),title:ResourceName::try_from("First project".to_owned())? },
            FleetOperation::ProjectDirectoryCreate { project:second.clone(),board:board.clone(),title:ResourceName::try_from("Second project".to_owned())? },
        ] {
            let request = native_archive_request(&workspace,operation)?;
            check(matches!(sender.write(&request).await?,RequestDecision::Accepted { .. }),"native archive fixture metadata accepted")?;
        }
        equal(sender.report(&board,&first,1).await?,ArchiveReportOutcome::Refused(ArchiveReportRefusal::BoardNotArchiving { state: BoardState::Open }))?;
        let request = native_archive_request(&workspace,FleetOperation::WorkspaceArchive {})?;
        let decision = sender.write(&request).await?;
        check(matches!(decision,RequestDecision::Accepted { .. }),"native workspace archive accepted")?;
        let before = sender.snapshot().await?;
        check(before.0.iter().any(|record|matches!(record,Record::Board(value) if value.board_id == board && value.state == BoardState::Archiving)),"unknown projected leases remain archive blockers")?;
        let missing = ProjectId::try_from(uuid::Uuid::now_v7().to_string())?;
        equal(sender.report(&board,&missing,1).await?,ArchiveReportOutcome::Refused(ArchiveReportRefusal::NotFound(missing.into())))?;
        equal(sender.snapshot().await?,before.clone())?;
        equal(sender.report(&board,&first,1).await?,ArchiveReportOutcome::Recorded)?;
        equal(sender.snapshot().await?,before.clone())?;
        equal(successor.report(&board,&first,9).await?,ArchiveReportOutcome::AlreadyRecorded)?;
        equal(sender.snapshot().await?,before.clone())?;
        let final_time = RegistryTime(at.0 + chrono::TimeDelta::seconds(3));
        update_clock(final_time).await?;
        equal(sender.report(&board,&second,1).await?,ArchiveReportOutcome::Recorded)?;
        let expected = (finalized_records(&before.0,&board,&workspace,before.1+1,final_time)?,before.1+2);
        equal(sender.snapshot().await?,expected.clone())?;
        equal(successor.report(&board,&second,9).await?,ArchiveReportOutcome::AlreadyRecorded)?;
        equal(sender.snapshot().await?,expected.clone())?;
        equal(sender.write(&request).await?,decision)?;
        equal(sender.snapshot().await?,expected)?;
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let closed = sender.close().await;
    let other_closed = successor.close().await;
    proof?;
    closed?;
    other_closed?;
    println!(
        "ARCHIVE native sessions: Recorded/AlreadyRecorded/typed refusal, unknown lease blockers, partial/successor replay no-op, exact attributed board/workspace finalization and unchanged ledger: passed"
    );
    Ok(())
}

fn native_archive_request(
    workspace: &WorkspaceId,
    operation: FleetOperation,
) -> ConformanceResult<FleetRequest> {
    Ok(FleetRequest {
        request_id: RequestId::try_from(uuid::Uuid::now_v7().to_string())?,
        workspace: workspace.clone(),
        actor: RecordedActor::Human {
            human_id: HumanId::try_from("native archive requester".to_owned())?,
        },
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation,
    })
}
