//! An archive appends both its notices in its own transaction (slice 4a), so
//! they survive the death of the native OS process that committed them, and a
//! reconnecting holder reads them at their original positions exactly once.
use super::*;
use collaboration_central_core::{ConformanceBoardState, RegistryTime};
use fleet_admission::{DirectoryEntry, DirectoryLease, DirectorySide, LeaseMetadata};
use fleet_protocol::{
    DatabaseId, Epoch, HolderChangeAnswer, NoticePosition, RequestId as LeaseRequestId,
};
use reqwest_websocket::WebSocket;

const WORKSPACE: &str = "019f0000-0000-7000-8000-000000000a81";
const BOARD: &str = "019f0000-0000-7000-8000-000000000a82";
const ARCHIVE_NOTICES: &str = "SELECT COUNT(*) AS count FROM machine_control_project_notices WHERE kind='project_list_changed' AND position IN (2,3)";
const NOTICE_JOBS: &str =
    "SELECT COUNT(*) AS count FROM _fleet_sync_operations WHERE kind='central.fleet-notice'";

struct PreparedArchive {
    request: FleetRequest,
    decision: FleetRequestResult,
    records: Vec<Record>,
    feed: Vec<Record>,
    grant_notice: PositionedBoardNotice,
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare the stopped store and the reconnected reads while process errors propagate."
)]
#[tokio::test]
async fn archived_workspace_notices_survive_process_death_without_duplicate_positions()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let database = imported_board_fixture::arrange_imported_unheld_board(
        root.path(),
        ConformanceBoardState::Open,
    )
    .await?;
    let jwks = JwksFixtureServer::start().await?;
    let first = ServerProcess::start(root.path(), &jwks.url).await?;
    let prepared = prepare_archive(&first.base, database).await;
    if prepared.is_err() {
        let _ = observe_failed_preparation(&first.base, root.path()).await;
    }
    // A failed readiness assertion must still reap its actual owned process.
    let crashed = first.crash().await;
    crashed?;
    let prepared = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            jwks.shutdown().await?;
            return Err(error);
        }
    };
    // Inspect the stopped owning store before startup can drive anything: both
    // notices committed with the archive and no notice job carries them.
    let fleet_path = root.path().join("fleet.db");
    assert_eq!(read_count(&fleet_path, ARCHIVE_NOTICES).await?, 2);
    assert_eq!(read_count(&fleet_path, NOTICE_JOBS).await?, 0);
    eprintln!("FLEET_NOTICE_RESTART_STAGE archive notices persisted at process death");

    let second = ServerProcess::start(root.path(), &jwks.url).await?;
    let proof = verify_recovered_notices(&second.base, root.path(), &prepared).await;
    let stopped = second.stop().await;
    let keys_stopped = jwks.shutdown().await;
    proof?;
    stopped?;
    keys_stopped?;
    println!(
        "FLEET_NOTICE_RESTART_CASE committed_archive_notices_and_no_repeated_positions passed"
    );
    Ok(())
}

async fn prepare_archive(base: &str, database: DatabaseId) -> ConformanceResult<PreparedArchive> {
    let at = RegistryTime(chrono::Utc::now() - chrono::TimeDelta::seconds(1));
    control(
        base,
        ObjectAddress::Fleet,
        RuntimeControl::SetTime { now: at },
    )
    .await?;
    let mut holder = enroll_admitted_peer(
        &ConformanceClient::new(base)?,
        SERVICE,
        PEER,
        CREDENTIAL,
        &Uuid::now_v7().to_string(),
    )
    .await?;
    let proof = async {
        let granted = imported_claim_scenarios::claim(
            &mut holder.socket,
            database,
            LeaseRequestId::new(Uuid::now_v7())?,
        )
        .await?;
        assert_eq!(granted, HolderChangeAnswer::Granted {
            epoch: Epoch::new(2)?,
            expires_at: runtime_primitives::SyncTime::from_unix_millis(
                at.0.timestamp_millis() + 14_400_000,
            ),
            start: fleet_protocol::ReplicationPosition::new(
                Epoch::new(1)?, fleet_protocol::CommitSequence::new(1),
            ),
        });
        eprintln!("FLEET_NOTICE_RESTART_STAGE normal imported Claim granted");
        await_holder_projection(&mut holder.socket, database).await?;
        eprintln!("FLEET_NOTICE_RESTART_STAGE normal Directory holder published");
        let grant = PositionedBoardNotice {
            position: NoticePosition::new(1),
            kind: BoardNotice::LeaseGranted {
                project: ProjectId::try_from(database.to_string())?,
                epoch: Epoch::new(2)?,
            },
        };
        tokio::time::timeout(RECOVERY_BOUND, async {
            loop {
                let notices = read_machine_notices(&mut holder.socket, None).await?;
                if notices == vec![grant.clone()] {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                }
                if !notices.is_empty() {
                    return Err(format!("real Claim produced another notice history: {notices:?}").into());
                }
                tokio::task::yield_now().await;
            }
        }).await.map_err(|_| "normal imported Claim grant notice did not arrive during setup")??;
        eprintln!("FLEET_NOTICE_RESTART_STAGE original grant notice retained");
        let (initial_feed, head) = read_complete_fleet_feed(&mut holder.socket).await?;
        assert_eq!(head, 3);
        assert_eq!(initial_feed.len(), 3);
        let request = FleetRequest {
            request_id: message_board::RequestId::generate(),
            workspace: WorkspaceId::try_from(WORKSPACE.to_owned())?,
            actor: RecordedActor::Human { human_id: HumanId::try_from("notice restart owner".to_owned())? },
            acting_for: None,
            protocol_version: ProtocolVersion::CURRENT,
            operation: FleetOperation::WorkspaceArchive {},
        };
        let decision = decide(&mut holder.socket, &request).await?;
        assert!(matches!(&decision, FleetRequestResult::Decided(RequestDecision::Accepted {
            change: ChangeOutcome::Changed, ..
        })));
        let (records, snapshot_at) = read_small_fleet_snapshot(&mut holder.socket).await?;
        assert_eq!(snapshot_at, 6);
        assert!(records.iter().any(|record| matches!(record, Record::Board(board)
            if board.board_id.as_str()==BOARD && board.state==BoardState::Archiving)));
        assert!(records.iter().any(|record| matches!(record, Record::Workspace(workspace)
            if workspace.workspace_id.as_str()==WORKSPACE && workspace.state==WorkspaceState::Archiving)));
        let (feed, head) = read_complete_fleet_feed(&mut holder.socket).await?;
        assert_eq!(head, 6);
        assert_eq!(feed.len(), 6);
        assert_eq!(feed.get(..3).ok_or("archive feed lost its imported prefix")?, initial_feed.as_slice());
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(PreparedArchive {
            request, decision, records, feed, grant_notice: grant,
        })
    }.await;
    let closed = holder
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    let prepared = proof?;
    closed?;
    Ok(prepared)
}

async fn verify_recovered_notices(
    base: &str,
    root: &std::path::Path,
    prepared: &PreparedArchive,
) -> ConformanceResult<()> {
    let mut socket = registration::reconnect(base).await?;
    let proof = async {
        let hint = BoardNotice::ProjectListChanged {};
        let recovered = read_machine_notices(&mut socket, Some(1)).await?;
        let positions: Vec<_> = recovered
            .iter()
            .map(|notice| (notice.position.value(), notice.kind.clone()))
            .collect();
        assert_eq!(
            positions,
            vec![(2, hint.clone()), (3, hint.clone())],
            "the archive notices changed across process death"
        );
        assert_eq!(
            read_small_fleet_snapshot(&mut socket).await?,
            (prepared.records.clone(), 6)
        );
        assert_eq!(
            read_complete_fleet_feed(&mut socket).await?,
            (prepared.feed.clone(), 6)
        );
        let mut expected_history = vec![prepared.grant_notice.clone()];
        expected_history.extend(recovered.clone());
        assert_eq!(
            read_machine_notices(&mut socket, None).await?,
            expected_history
        );
        // A replayed archive answers from its recorded decision and appends nothing.
        assert_eq!(
            decide(&mut socket, &prepared.request).await?,
            prepared.decision
        );
        assert_eq!(
            read_machine_notices(&mut socket, None).await?,
            expected_history
        );
        // Later time runs the due sources; none of them repeats a notice.
        // SettleClock answers a seam result, not a fault observation.
        post(
            base,
            "/admin/v1/conformance/runtime",
            &serde_json::to_value(collaboration_central_conformance::RuntimeControlRequest {
                address: ObjectAddress::Fleet,
                command: RuntimeControl::SettleClock {
                    now: RegistryTime(chrono::Utc::now() + chrono::TimeDelta::seconds(60)),
                },
            })?,
        )
        .await?;
        assert_eq!(
            read_machine_notices(&mut socket, None).await?,
            expected_history
        );
        assert_eq!(
            read_small_fleet_snapshot(&mut socket).await?,
            (prepared.records.clone(), 6)
        );
        assert_eq!(
            read_complete_fleet_feed(&mut socket).await?,
            (prepared.feed.clone(), 6)
        );
        let fleet_path = root.join("fleet.db");
        assert_eq!(read_count(&fleet_path, ARCHIVE_NOTICES).await?, 2);
        assert_eq!(read_count(&fleet_path, NOTICE_JOBS).await?, 0);
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let closed = socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    proof?;
    closed?;
    Ok(())
}

async fn read_machine_notices(
    socket: &mut WebSocket,
    after: Option<u64>,
) -> ConformanceResult<Vec<PositionedBoardNotice>> {
    match creation_scenarios::operation_answer(
        socket,
        OperationRequest::Consumer(BoardConsumerRequest::Read {
            scope: BoardReadScope::Notices,
            cursor: after.map(|position| BoardReadCursor::Notices(NoticePosition::new(position))),
        }),
    )
    .await?
    {
        OperationAnswer::Consumer(BoardConsumerAnswer::Read(BoardReadPage::Notices(
            NoticeRead::Page { notices, .. },
        ))) => Ok(notices),
        other => Err(format!(
            "notice catch-up returned another read: {}",
            serde_json::to_string(&other)?
        )
        .into()),
    }
}

async fn read_small_fleet_snapshot(
    socket: &mut WebSocket,
) -> ConformanceResult<(Vec<Record>, u64)> {
    match creation_scenarios::operation_answer(
        socket,
        OperationRequest::Consumer(BoardConsumerRequest::Read {
            scope: BoardReadScope::FleetSnapshot,
            cursor: None,
        }),
    )
    .await?
    {
        OperationAnswer::Consumer(BoardConsumerAnswer::Read(BoardReadPage::FleetSnapshot {
            records,
            next: None,
            snapshot_at,
            ..
        })) => Ok((
            records
                .into_iter()
                .map(|record| record.decode::<Record>())
                .collect::<Result<Vec<_>, _>>()?,
            snapshot_at.get(),
        )),
        other => Err(format!(
            "small fixture Fleet snapshot returned another page: {}",
            serde_json::to_string(&other)?
        )
        .into()),
    }
}

async fn read_complete_fleet_feed(socket: &mut WebSocket) -> ConformanceResult<(Vec<Record>, u64)> {
    match creation_scenarios::operation_answer(
        socket,
        OperationRequest::Consumer(BoardConsumerRequest::Read {
            scope: BoardReadScope::FleetRecords,
            cursor: Some(BoardReadCursor::Fleet(FleetPosition::try_from(0)?)),
        }),
    )
    .await?
    {
        OperationAnswer::Consumer(BoardConsumerAnswer::Read(BoardReadPage::FleetFeed(
            FleetFeedRead::Page {
                records,
                through,
                head,
                ..
            },
        ))) if through == head => Ok((
            records
                .into_iter()
                .map(|record| record.decode::<Record>())
                .collect::<Result<Vec<_>, _>>()?,
            head.get(),
        )),
        other => Err(format!(
            "small fixture Fleet feed returned another page: {}",
            serde_json::to_string(&other)?
        )
        .into()),
    }
}

async fn await_holder_projection(
    socket: &mut WebSocket,
    database: DatabaseId,
) -> ConformanceResult<()> {
    tokio::time::timeout(RECOVERY_BOUND, async {
        loop {
            let OperationAnswer::Consumer(BoardConsumerAnswer::Read(BoardReadPage::Directory {entries,..})) = creation_scenarios::operation_answer(socket, OperationRequest::Consumer(BoardConsumerRequest::Read {
                scope: BoardReadScope::Directory {query: BoardPayload::encode(&fleet_admission::DirectoryQuery::default())?}, cursor: None,
            })).await? else { return Err("Claim fixture did not expose its Directory publication".into()); };
            let entries=entries.into_iter().map(|entry|entry.decode::<DirectoryEntry>()).collect::<Result<Vec<_>,_>>()?;
            let project=entries.iter().find(|entry|entry.project.as_str()==database.to_string()).ok_or("Claim fixture Directory omitted its project")?;
            if matches!(&project.lease_side,DirectorySide::Known(LeaseMetadata {lease:DirectoryLease::Held {holder,..},..}) if holder.as_str()==SERVICE) {
                return Ok::<_,Box<dyn std::error::Error+Send+Sync>>(());
            }
            tokio::task::yield_now().await;
        }
    }).await.map_err(|_| "normal imported Claim Directory holder was not published during setup")?
}

async fn observe_failed_preparation(base: &str, root: &std::path::Path) -> ConformanceResult<()> {
    use sqlx_core::{
        connection::{ConnectOptions, Connection},
        query::query,
        row::Row,
    };
    for (address, file) in [(ObjectAddress::Fleet, "fleet.db")] {
        let observed = control(base, address, RuntimeControl::Observe).await?;
        eprintln!(
            "FLEET_NOTICE_RESTART_DIAGNOSTIC file={file} clock={:?} wall_ms={}",
            observed.clock,
            chrono::Utc::now().timestamp_millis()
        );
        let mut connection = sqlx_turso::TursoConnectOptions::new()
            .filename(root.join(file))
            .read_only(true)
            .connect()
            .await?;
        let rows=query::<sqlx_turso::Turso>("SELECT kind,state,next_attempt_at,drive_until,attempt FROM _fleet_sync_operations ORDER BY kind").fetch_all(&mut connection).await?;
        for row in rows {
            eprintln!(
                "FLEET_NOTICE_RESTART_DIAGNOSTIC file={file} state={} kind={} next_attempt_at={:?} drive_until={:?} attempt={}",
                row.try_get::<String, _>("state")?,
                row.try_get::<String, _>("kind")?,
                row.try_get::<Option<i64>, _>("next_attempt_at")?,
                row.try_get::<Option<i64>, _>("drive_until")?,
                row.try_get::<i64, _>("attempt")?
            );
        }
        connection.close().await?;
    }
    Ok(())
}
