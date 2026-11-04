//! A real Claim produces the retained hint; a fresh OS process reconstructs expiry.
use super::*;
use collaboration_central_core::{ConformanceBoardState, RegistryTime};
use fleet_sync_protocol::{DatabaseId, Epoch, HolderChangeAnswer, NoticePosition};
use sqlx_core::{
    connection::{ConnectOptions, Connection},
    query::query,
    row::Row,
};

const CREATED: i64 = 1_792_281_600_000;
const RETENTION_MILLIS: i64 = 7 * 24 * 60 * 60 * 1_000;

#[tokio::test]
async fn registry_claim_notice_survives_process_death_and_expires_without_a_read()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let database = imported_board_fixture::arrange_imported_unheld_board(
        root.path(),
        ConformanceBoardState::Open,
    )
    .await?;
    let jwks = JwksFixtureServer::start().await?;
    let first =
        ServerProcess::start_with_controls(root.path(), &jwks.url, &[clock(CREATED)?]).await?;
    let prepared = prepare(&first.base, root.path(), database).await;
    let crashed = first.crash().await;
    crashed?;
    let timestamps = match prepared {
        Ok(timestamps) => timestamps,
        Err(error) => {
            jwks.shutdown().await?;
            return Err(error);
        }
    };
    let proof = async {
        let second =
            ServerProcess::start_with_controls(root.path(), &jwks.url, &[clock(CREATED + 1)?])
                .await?;
        let retained = verify_retained(&second.base, root.path(), database, timestamps).await;
        let stopped = second.stop().await;
        retained?;
        stopped?;
        // A third actual process starts with the persisted expiry due. Before
        // any reconnect, notice read or post-start time command, startup must
        // reconstruct and run the owning Fleet's normal expiry alarm.
        let third =
            ServerProcess::start_with_controls(root.path(), &jwks.url, &[clock(timestamps.1)?])
                .await?;
        let expired = verify_automatic_expiry(root.path()).await;
        let stopped = third.stop().await;
        expired?;
        stopped?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let keys_stopped = jwks.shutdown().await;
    proof?;
    keys_stopped?;
    println!("REGISTRY_NOTICE_RESTART_CASE claim_retained_seven_days_and_automatic_expiry passed");
    Ok(())
}

async fn prepare(
    base: &str,
    root: &std::path::Path,
    database: DatabaseId,
) -> ConformanceResult<(i64, i64)> {
    let mut holder = enroll_admitted_peer(
        &ConformanceClient::new(base)?,
        SERVICE,
        PEER,
        CREDENTIAL,
        &Uuid::now_v7().to_string(),
    )
    .await?;
    // One injected Fleet clock owns both lease commitment and notice append.
    // Its value was installed before discovery; no former-owner clock can
    // overwrite it and no real-time window is part of the oracle.
    let grant = imported_claim_scenarios::claim(
        &mut holder.socket,
        database,
        fleet_sync_protocol::RequestId::new(Uuid::now_v7())?,
    )
    .await?;
    assert_eq!(
        grant,
        HolderChangeAnswer::Granted {
            epoch: Epoch::new(2)?,
            expires_at: fleet_sync_protocol::SyncTime::from_unix_millis(CREATED + 14_400_000,),
            start: fleet_sync_protocol::ReplicationPosition::new(
                Epoch::new(1)?,
                fleet_sync_protocol::CommitSequence::new(1),
            ),
        }
    );
    let expected = expected(database)?;
    tokio::time::timeout(RECOVERY_BOUND, async {
        loop {
            let notices = read(&mut holder.socket).await?;
            if notices
                == (NoticeRead::Page {
                    notices: vec![expected.clone()],
                    as_of: chrono::DateTime::from_timestamp_millis(CREATED)
                        .ok_or("invalid created clock")?,
                })
            {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            if !matches!(&notices, NoticeRead::Page {notices,..} if notices.is_empty()) {
                return Err(format!("Claim produced another notice history: {notices:?}").into());
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| "real Claim did not produce its Registry grant notice")??;
    let saved = timestamps(root)
        .await?
        .ok_or("Claim notice missing from owning store")?;
    assert_eq!(saved, (CREATED, CREATED + RETENTION_MILLIS));
    Ok(saved)
}

async fn verify_retained(
    base: &str,
    root: &std::path::Path,
    database: DatabaseId,
    saved: (i64, i64),
) -> ConformanceResult<()> {
    // Fresh process, before reconnect or notice read; original times survived.
    assert_eq!(timestamps(root).await?, Some(saved));
    assert!(CREATED + 1 < saved.1);
    let mut socket = registration::reconnect(base).await?;
    let page = read(&mut socket).await?;
    assert!(matches!(page, NoticeRead::Page {notices,..} if notices==vec![expected(database)?]));
    socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    Ok(())
}

async fn verify_automatic_expiry(root: &std::path::Path) -> ConformanceResult<()> {
    tokio::time::timeout(RECOVERY_BOUND, async {
        loop {
            if timestamps(root).await?.is_none() {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| "startup did not automatically prune the expired notice before a read")??;
    Ok(())
}

fn clock(
    millis: i64,
) -> ConformanceResult<collaboration_central_conformance::RuntimeControlRequest> {
    Ok(collaboration_central_conformance::RuntimeControlRequest {
        address: ObjectAddress::Fleet,
        command: RuntimeControl::SetTime {
            now: RegistryTime(
                chrono::DateTime::from_timestamp_millis(millis)
                    .ok_or("notice test clock outside UTC range")?,
            ),
        },
    })
}

fn expected(database: DatabaseId) -> ConformanceResult<PositionedBoardNotice> {
    Ok(PositionedBoardNotice {
        position: NoticePosition::new(1),
        kind: BoardNotice::LeaseGranted {
            project: ProjectId::try_from(database.to_string())?,
            epoch: Epoch::new(2)?,
        },
    })
}
async fn read(socket: &mut reqwest_websocket::WebSocket) -> ConformanceResult<NoticeRead> {
    let answer = creation_scenarios::operation_answer(
        socket,
        OperationRequest::Consumer(BoardConsumerRequest::Read {
            scope: BoardReadScope::Notices,
            cursor: None,
        }),
    )
    .await?;
    match answer {
        OperationAnswer::Consumer(BoardConsumerAnswer::Read(BoardReadPage::Notices(page))) => {
            Ok(page)
        }
        other => Err(format!("notice read returned {}", serde_json::to_string(&other)?).into()),
    }
}
async fn timestamps(root: &std::path::Path) -> ConformanceResult<Option<(i64, i64)>> {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join("fleet.db"))
        .read_only(true)
        .connect()
        .await?;
    let rows = query::<sqlx_turso::Turso>(
        "SELECT created_at,expires_at FROM machine_control_notices ORDER BY position",
    )
    .fetch_all(&mut connection)
    .await?;
    let result = match rows.as_slice() {
        [] => None,
        [row] => Some((row.try_get("created_at")?, row.try_get("expires_at")?)),
        _ => return Err("Claim produced duplicate retained notice rows".into()),
    };
    connection.close().await?;
    Ok(result)
}
