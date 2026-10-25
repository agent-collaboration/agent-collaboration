//! Both original notice-loss paths recover through a fresh native OS process.
use super::*;
use collaboration_central_conformance::ObjectReplyFaultObservation;
use collaboration_central_core::{ConformanceBoardState, RegistryTime};
use fleet_admission::{DirectoryEntry, DirectoryLease, DirectorySide, LeaseMetadata};
use fleet_sync_protocol::{
    DatabaseId, Epoch, HolderChangeAnswer, NoticePosition, RequestId as LeaseRequestId,
};
use reqwest_websocket::WebSocket;

const WORKSPACE: &str = "019f0000-0000-7000-8000-000000000a81";
const BOARD: &str = "019f0000-0000-7000-8000-000000000a82";

#[derive(Clone, Copy)]
enum NoticeLoss {
    Dispatch,
    Acknowledgement,
}

struct PreparedNoticeRestart {
    request: FleetRequest,
    decision: FleetRequestResult,
    records: Vec<Record>,
    feed: Vec<Record>,
    saved_call: Value,
    operation_id: Value,
    input_digest: Vec<u8>,
    grant_notice: PositionedBoardNotice,
}

#[tokio::test]
async fn archived_workspace_notices_recover_after_process_death_before_dispatch()
-> ConformanceResult<()> {
    run_notice_loss_across_process_restart(NoticeLoss::Dispatch).await
}

#[tokio::test]
async fn archived_workspace_notices_recover_after_real_ack_loss_without_duplicate_positions()
-> ConformanceResult<()> {
    run_notice_loss_across_process_restart(NoticeLoss::Acknowledgement).await
}

async fn run_notice_loss_across_process_restart(loss: NoticeLoss) -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let database = imported_board_fixture::arrange_imported_unheld_board(
        root.path(),
        ConformanceBoardState::Open,
    )
    .await?;
    let jwks = JwksFixtureServer::start().await?;
    let first = ServerProcess::start(root.path(), &jwks.url).await?;
    let prepared = prepare_notice_loss(&first.base, root.path(), database, loss).await;
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
    // Inspect the stopped owning store before startup can drive anything. A
    // successful retry before SIGKILL cannot masquerade as crash recovery.
    assert_eq!(
        operation(&root.path().join("fleet.db"), &prepared.operation_id)
            .await?
            .0,
        "running",
        "the original lost call completed before process death"
    );
    let (input, digest) = read_sender_notice_input(root.path(), &prepared.operation_id).await?;
    assert_eq!(
        serde_json::from_slice::<Value>(
            input
                .as_deref()
                .ok_or("process death lost the original pending input")?
        )?,
        json!({"installation":SERVICE,"notice":prepared.saved_call.get("request").ok_or("saved call omitted its payload")?})
    );
    assert_eq!(digest, prepared.input_digest);
    eprintln!("FLEET_NOTICE_RESTART_STAGE original Fleet debt persisted at process death");

    let second = ServerProcess::start(root.path(), &jwks.url).await?;
    let proof = verify_recovered_notices(&second.base, root.path(), &prepared).await;
    let stopped = second.stop().await;
    let keys_stopped = jwks.shutdown().await;
    proof?;
    stopped?;
    keys_stopped?;
    println!("FLEET_NOTICE_RESTART_CASE exact_saved_call_two_hints_and_no_repeated_debt passed");
    Ok(())
}

async fn prepare_notice_loss(
    base: &str,
    root: &std::path::Path,
    database: DatabaseId,
    loss: NoticeLoss,
) -> ConformanceResult<PreparedNoticeRestart> {
    let at = RegistryTime(chrono::Utc::now() - chrono::TimeDelta::seconds(1));
    control(
        base,
        ObjectAddress::Registry,
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
            start: fleet_sync_protocol::ReplicationPosition::new(
                Epoch::new(1)?, fleet_sync_protocol::CommitSequence::new(1),
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
        let destination = ObjectAddress::Connection(SERVICE.to_owned().try_into()?);
        // Each released call arms the next hold atomically. A first receiver
        // call may only admit its async append; keep advancing until a real
        // successful reply is lost, then retain the next hold until SIGKILL.
        control(base, ObjectAddress::Fleet, RuntimeControl::HoldNextDispatch {
            destination: destination.clone(),
        }).await?;
        if matches!(loss,NoticeLoss::Acknowledgement) {
            control(base,ObjectAddress::Fleet,RuntimeControl::DropNextObjectReply {
                destination: destination.clone(),
            }).await?;
        }
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
        let observed = tokio::time::timeout(Duration::from_secs(8), async {
            let recorded: ConformanceResult<Vec<u8>> = match loss {
                NoticeLoss::Dispatch => {
                    let observed = control(base, ObjectAddress::Fleet, RuntimeControl::AwaitHeldDispatch).await?;
                    match observed.dispatch {
                        DispatchFaultObservation::Held { destination: actual, request } if actual == destination => Ok(request),
                        other => Err(format!("actual notice dispatch did not reach its barrier: {other:?}").into()),
                    }
                }
                NoticeLoss::Acknowledgement => {
                    loop {
                        let observed=control(base,ObjectAddress::Fleet,RuntimeControl::AwaitHeldDispatch).await?;
                        if !matches!(observed.dispatch,DispatchFaultObservation::Held {destination:ref actual,..} if *actual==destination) {
                            return Err("notice ACK-loss handoff lost its real next-dispatch hold".into());
                        }
                        if let ObjectReplyFaultObservation::Dropped {destination:actual,request}=observed.object_reply {
                            if actual!=destination {return Err("notice ACK loss recorded another destination".into());}
                            eprintln!("FLEET_NOTICE_RESTART_STAGE successful_reply_lost_next_dispatch_held");
                            break Ok(request);
                        }
                        control(base,ObjectAddress::Fleet,RuntimeControl::ResumeDispatchAndHoldNext {destination:destination.clone()}).await?;
                    }
                }
            };
            recorded
        }).await.map_err(|_| "actual notice dispatch/reply-loss readiness timed out")??;
        let saved_call: Value = serde_json::from_slice(&observed)?;
        assert_eq!(saved_call.get("kind"), Some(&json!("central.machine-notice")));
        assert_eq!(saved_call.pointer("/identity/step"), Some(&json!("deliver_notice")));
        assert_eq!(saved_call.pointer("/target/installation"), Some(&json!(SERVICE)));
        let operation_id = saved_call.pointer("/identity/operation")
            .ok_or("lost notice call omitted its saved operation identity")?.clone();
        let (stored_input, input_digest) = read_sender_notice_input(root, &operation_id).await?;
        if stored_input.is_none() {
            eprintln!("FLEET_NOTICE_RESTART_DIAGNOSTIC original sender already terminal before SIGKILL: {}",
                operation(&root.join("fleet.db"), &operation_id).await?.0);
        }
        let stored_input: Value = serde_json::from_slice(
            stored_input.as_deref().ok_or("interrupted notice compacted its input before completion")?,
        )?;
        let saved_notice = saved_call.get("request").cloned().ok_or("saved call omitted its notice payload")?;
        assert_eq!(stored_input, json!({"installation":SERVICE,"notice":saved_notice}));
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
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(PreparedNoticeRestart {
            request, decision, records, feed, saved_call, operation_id, input_digest,
            grant_notice: grant,
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
    prepared: &PreparedNoticeRestart,
) -> ConformanceResult<()> {
    let mut socket = registration::reconnect(base).await?;
    let proof = async {
        let board_notice = BoardNotice::BoardArchiving {
            board: BoardId::try_from(BOARD.to_owned())?,
        };
        let feed_notice = BoardNotice::FleetFeedAdvanced {
            through: FleetPosition::try_from(6)?,
        };
        let recovered = tokio::time::timeout(RECOVERY_BOUND, async {
            loop {
                let notices = read_machine_notices(&mut socket, Some(1)).await?;
                reject_duplicate_hints(&notices, &board_notice, &feed_notice)?;
                if let [first_notice, second_notice] = notices.as_slice() {
                    assert_eq!(first_notice.position.value(), 2);
                    assert_eq!(second_notice.position.value(), 3);
                    assert_eq!(notices.iter().filter(|notice| notice.kind==board_notice).count(), 1);
                    assert_eq!(notices.iter().filter(|notice| notice.kind==feed_notice).count(), 1);
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(notices);
                }
                tokio::task::yield_now().await;
            }
        }).await.map_err(|_| "startup did not catch up both exact archive notices")??;
        assert_eq!(read_small_fleet_snapshot(&mut socket).await?, (prepared.records.clone(), 6));
        assert_eq!(read_complete_fleet_feed(&mut socket).await?, (prepared.feed.clone(), 6));
        let fleet_path = root.join("fleet.db");
        tokio::time::timeout(RECOVERY_BOUND, async {
            loop {
                let notices = read_machine_notices(&mut socket, Some(1)).await?;
                reject_duplicate_hints(&notices, &board_notice, &feed_notice)?;
                if operation(&fleet_path, &prepared.operation_id).await?.0 == "completed"
                    && read_count(&fleet_path,
                        "SELECT COUNT(*) AS count FROM _fleet_sync_operations WHERE kind='central.fleet-notice' AND state!='completed'"
                    ).await? == 0
                {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                }
                tokio::task::yield_now().await;
            }
        }).await.map_err(|_| "recovered notice was delivered but its original Fleet debt never completed")??;
        assert_eq!(read_sender_notice_input(root, &prepared.operation_id).await?.1, prepared.input_digest);
        assert_original_receiver_receipt(root, &prepared.saved_call).await?;
        let mut expected_history = vec![prepared.grant_notice.clone()];
        expected_history.extend(recovered.clone());
        assert_eq!(read_machine_notices(&mut socket, None).await?, expected_history);
        let original_notice: BoardNotice = serde_json::from_value(prepared.saved_call.get("request").cloned().ok_or("saved call omitted its notice payload")?)?;
        assert_eq!(recovered.iter().filter(|notice| notice.kind == original_notice).count(), 1);
        assert_eq!(decide(&mut socket, &prepared.request).await?, prepared.decision);
        assert_eq!(read_machine_notices(&mut socket, Some(1)).await?, recovered);
        assert_eq!(read_machine_notices(&mut socket, None).await?, expected_history);
        assert_eq!(read_small_fleet_snapshot(&mut socket).await?, (prepared.records.clone(), 6));
        assert_eq!(read_complete_fleet_feed(&mut socket).await?, (prepared.feed.clone(), 6));
        // Preserve the old later-time no-repeat checkpoint through the current
        // native clock/wake capability, after the journal debt is terminal.
        control(base, ObjectAddress::Fleet, RuntimeControl::SetTime {
            now: RegistryTime(chrono::Utc::now() + chrono::TimeDelta::seconds(60)),
        }).await?;
        assert_eq!(read_machine_notices(&mut socket, None).await?, expected_history);
        assert_eq!(read_small_fleet_snapshot(&mut socket).await?, (prepared.records.clone(), 6));
        assert_eq!(read_complete_fleet_feed(&mut socket).await?, (prepared.feed.clone(), 6));
        assert_eq!(read_count(&fleet_path,
            "SELECT COUNT(*) AS count FROM _fleet_sync_operations WHERE kind='central.fleet-notice'"
        ).await?, 2);
        assert_eq!(read_count(&fleet_path,
            "SELECT COUNT(*) AS count FROM _fleet_sync_operations WHERE kind='central.fleet-notice' AND state!='completed'"
        ).await?, 0);
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let closed = socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    proof?;
    closed?;
    Ok(())
}

fn reject_duplicate_hints(
    notices: &[PositionedBoardNotice],
    board: &BoardNotice,
    feed: &BoardNotice,
) -> ConformanceResult<()> {
    let repeated_kind = [board, feed]
        .into_iter()
        .any(|kind| notices.iter().filter(|notice| &notice.kind == kind).count() > 1);
    if notices.len() > 2 || repeated_kind {
        return Err(format!(
            "original notice calls appended duplicate machine positions: {notices:?}"
        )
        .into());
    }
    Ok(())
}

async fn read_sender_notice_input(
    root: &std::path::Path,
    identity: &Value,
) -> ConformanceResult<(Option<Vec<u8>>, Vec<u8>)> {
    use sqlx_core::{
        connection::{ConnectOptions, Connection},
        query::query,
        row::Row,
    };
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join("fleet.db"))
        .read_only(true)
        .connect()
        .await?;
    let result=async {
        let row=query::<sqlx_turso::Turso>("SELECT input,input_digest FROM _fleet_sync_operations WHERE operation_id=? AND kind='central.fleet-notice'")
            .bind(identity.as_str().ok_or("saved operation identity is not text")?).fetch_one(&mut connection).await?;
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>((row.try_get("input")?,row.try_get("input_digest")?))
    }.await;
    let closed = connection.close().await;
    let value = result?;
    closed?;
    Ok(value)
}

async fn assert_original_receiver_receipt(
    root: &std::path::Path,
    call: &Value,
) -> ConformanceResult<()> {
    use sqlx_core::{
        connection::{ConnectOptions, Connection},
        query::query,
        row::Row,
    };
    let operation = call
        .pointer("/identity/operation")
        .and_then(Value::as_str)
        .ok_or("saved operation identity is not text")?;
    let step = call
        .pointer("/identity/step")
        .and_then(Value::as_str)
        .ok_or("saved call step is not text")?;
    let key = format!("central-machine-notice:{operation}:{step}");
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join(format!("connection-{SERVICE}.db")))
        .read_only(true)
        .connect()
        .await?;
    let result=async {
        let row=query::<sqlx_turso::Turso>("SELECT COUNT(*) AS count FROM _fleet_sync_operations WHERE kind='central.machine-notice-append' AND request_key=? AND state='completed'")
            .bind(key).fetch_one(&mut connection).await?;
        assert_eq!(row.try_get::<i64,_>("count")?,1,"original sender operation/step lost its one completed receiver receipt");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let closed = connection.close().await;
    result?;
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
    for (address, file) in [
        (ObjectAddress::Registry, "registry.db"),
        (ObjectAddress::Fleet, "fleet.db"),
    ] {
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
