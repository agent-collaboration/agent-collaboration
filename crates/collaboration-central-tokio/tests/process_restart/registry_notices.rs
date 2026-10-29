//! A real Claim produces the retained hint; a fresh OS process reconstructs expiry.
use super::*;
use collaboration_central_core::ConformanceBoardState;
use fleet_sync_protocol::{DatabaseId, Epoch, HolderChangeAnswer, NoticePosition};
use sqlx_core::{
    connection::{ConnectOptions, Connection},
    query::query,
    row::Row,
};

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
    let created = chrono::Utc::now().timestamp_millis();
    let first = ServerProcess::start_at(root.path(), &jwks.url, created, &[]).await?;
    let prepared = prepare(&first.base, root.path(), database, created).await;
    let crashed = first.crash().await;
    crashed?;
    let timestamps = match prepared {
        Ok(timestamps) => timestamps,
        Err(error) => {
            jwks.shutdown().await?;
            return Err(error);
        }
    };
    let address = ObjectAddress::Connection(SERVICE.to_owned().try_into()?);
    let second = ServerProcess::start_at(
        root.path(),
        &jwks.url,
        created,
        std::slice::from_ref(&address),
    )
    .await?;
    let retained = verify_retained(&second.base, root.path(), database, timestamps, created).await;
    let stopped = second.stop().await;
    retained?;
    stopped?;
    let expired = timestamps.1 + 1;
    let third = ServerProcess::start_at(root.path(), &jwks.url, expired, &[address]).await?;
    // The receipt observes the actual owner; no reconnect, read or clock command
    // runs before this direct owning-store assertion.
    let proof = verify_expired(root.path(), timestamps, expired).await;
    let stopped = third.stop().await;
    let keys_stopped = jwks.shutdown().await;
    proof?;
    stopped?;
    keys_stopped?;
    println!("REGISTRY_NOTICE_RESTART_CASE claim_retained_seven_days_and_automatic_expiry passed");
    Ok(())
}

async fn verify_expired(
    root: &std::path::Path,
    saved: (i64, i64),
    now: i64,
) -> ConformanceResult<()> {
    assert!(
        now >= saved.1,
        "notice disappeared before its durable expiry"
    );
    assert_eq!(
        timestamps(root).await?,
        None,
        "startup did not automatically prune the expired notice before a read"
    );
    Ok(())
}

async fn prepare(
    base: &str,
    root: &std::path::Path,
    database: DatabaseId,
    created: i64,
) -> ConformanceResult<(i64, i64)> {
    let mut holder = enroll_admitted_peer(
        &ConformanceClient::new(base)?,
        SERVICE,
        PEER,
        CREDENTIAL,
        &Uuid::now_v7().to_string(),
    )
    .await?;
    let registry_at = created;
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
            expires_at: fleet_sync_protocol::SyncTime::from_unix_millis(registry_at + 14_400_000,),
            start: fleet_sync_protocol::ReplicationPosition::new(
                Epoch::new(1)?,
                fleet_sync_protocol::CommitSequence::new(1),
            ),
        }
    );
    let expected = expected(database)?;
    let first = read(&mut holder.socket).await?;
    let notices = if matches!(&first, NoticeRead::Page {notices,..} if notices.is_empty()) {
        // The Claim helper may already have consumed the live hint. If its row
        // is not committed yet, its real hint is still ahead of this read.
        tokio::time::timeout(RECOVERY_BOUND, async {
            loop {
                let message = holder
                    .socket
                    .next()
                    .await
                    .ok_or("Claim session ended before its grant notice")??;
                let frame: ServerFrame<BoardConsumerProtocol> = match message {
                    reqwest_websocket::Message::Binary(bytes) => serde_json::from_slice(&bytes)?,
                    reqwest_websocket::Message::Text(text) => serde_json::from_str(&text)?,
                    other => return Err(format!("Claim notice received {other:?}").into()),
                };
                match frame {
                    ServerFrame::Notice {
                        body: Notice::Consumer(actual),
                        ..
                    } if actual == expected => {
                        return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                    }
                    ServerFrame::Notice {
                        body: Notice::LeaseChanged { .. },
                        ..
                    } => {}
                    _ => return Err("Claim produced another live notice".into()),
                }
            }
        })
        .await
        .map_err(|_| "real Claim did not produce its Registry grant notice")??;
        read(&mut holder.socket).await?
    } else {
        first
    };
    assert_eq!(
        notices,
        NoticeRead::Page {
            notices: vec![expected],
            as_of: chrono::DateTime::from_timestamp_millis(created)
                .ok_or("invalid created clock")?,
        }
    );
    let saved = timestamps(root)
        .await?
        .ok_or("Claim notice missing from owning store")?;
    assert_eq!(saved, (created, created + RETENTION_MILLIS));
    Ok(saved)
}

async fn verify_retained(
    base: &str,
    root: &std::path::Path,
    database: DatabaseId,
    saved: (i64, i64),
    now: i64,
) -> ConformanceResult<()> {
    // Direct read precedes reconnect, any time command, and any notice read.
    assert_eq!(timestamps(root).await?, Some(saved));
    assert!(now < saved.1, "restart missed its pre-expiry barrier");
    let mut socket = registration::reconnect(base).await?;
    let page = read(&mut socket).await?;
    assert!(matches!(page, NoticeRead::Page {notices,..} if notices==vec![expected(database)?]));
    socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    Ok(())
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
        .filename(root.join(format!("connection-{SERVICE}.db")))
        .read_only(true)
        .connect()
        .await?;
    let rows = query::<sqlx_turso::Turso>(
        "SELECT created_at,expires_at FROM machine_notices ORDER BY position",
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
