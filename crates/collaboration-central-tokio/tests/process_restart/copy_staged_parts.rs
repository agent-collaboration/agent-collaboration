//! Copy startup reconstructs staged-part expiry without an observing call driving it.
use super::*;
use collaboration_central_admin_protocol::{CopyHealth, CopyHealthRead, LiveCopyHealth};
use collaboration_central_conformance::owner_assertion;
use collaboration_central_core::RegistryTime;
use fleet_sync_protocol::{
    CommitSequence, CopyStatus, DatabaseId, Epoch, PeerId, Piece, PieceContents, PieceDigest,
    ReplicatedTransaction, ReplicationAnswer, ReplicationPosition, ReplicationRequest,
    TransactionKey, TransactionPart,
};
use reqwest_websocket::WebSocket;

const PART_RETENTION_MILLIS: i64 = 86_400_001;

struct StagedCopy {
    database: DatabaseId,
    piece: Piece<BoardAnnotation>,
    transaction: TransactionKey,
    staged_at: chrono::DateTime<chrono::Utc>,
    status: CopyStatus,
}

#[tokio::test]
async fn copy_staged_part_expires_automatically_after_process_death_and_restages_freshly()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let created = chrono::Utc::now().timestamp_millis();
    let first = ServerProcess::start_at(root.path(), &jwks.url, created, &[]).await?;
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?;
    let prepared = stage_before_process_death(&http, &first.base, created).await;
    let crashed = first.crash().await;
    crashed?;
    let prepared = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            jwks.shutdown().await?;
            return Err(error);
        }
    };

    let address = ObjectAddress::Copy(prepared.database);
    let second = ServerProcess::start_at(
        root.path(),
        &jwks.url,
        created,
        std::slice::from_ref(&address),
    )
    .await?;
    let retained = assert_original_part(
        &read_health(&http, &second.base, prepared.database).await?,
        &prepared,
        created,
    );
    let stopped = second.stop().await;
    retained?;
    stopped?;
    let expired = created + PART_RETENTION_MILLIS;
    let third = ServerProcess::start_at(root.path(), &jwks.url, expired, &[address]).await?;
    let observed =
        observe_expiry_and_restage(&http, &third.base, root.path(), &prepared, expired).await;
    let stopped = third.stop().await;
    let keys_stopped = jwks.shutdown().await;
    observed?;
    stopped?;
    keys_stopped?;
    println!("COPY_STAGED_RESTART_CASE automatic_expiry_and_fresh_restage passed");
    Ok(())
}

async fn stage_before_process_death(
    http: &reqwest::Client,
    base: &str,
    created: i64,
) -> ConformanceResult<StagedCopy> {
    let mut holder = enroll_admitted_peer(
        &ConformanceClient::new(base)?,
        SERVICE,
        PEER,
        CREDENTIAL,
        &Uuid::now_v7().to_string(),
    )
    .await?;
    let proof = async {
        let database = DatabaseId::new(Uuid::now_v7())?;
        let workspace = WorkspaceId::generate();
        let board = BoardId::generate();
        for operation in [
            FleetOperation::WorkspaceCreate {
                name: ResourceName::try_from("staged Copy restart".to_owned())?,
                description: Description::try_from("ordinary registration fixture".to_owned())?,
            },
            FleetOperation::BoardCreate {
                board: board.clone(),
                name: ResourceName::try_from("staged Copy board".to_owned())?,
                description: Description::try_from("incomplete schema transaction".to_owned())?,
                board_kind: BoardKind::Area {},
            },
            creation_scenarios::project(database, board)?,
        ] {
            creation_scenarios::accepted(
                creation_scenarios::metadata(&mut holder.socket, &workspace, operation).await?,
            )?;
        }
        // Preserve the independent four-hour holder grant while Copy's saved
        // part ages a full day. Only the first process receives this setup pin;
        // neither reopened process receives a clock command.
        control(
            base,
            ObjectAddress::Registry,
            RuntimeControl::SetTime {
                now: RegistryTime(
                    chrono::DateTime::from_timestamp_millis(created + PART_RETENTION_MILLIS)
                        .ok_or("Registry setup time is outside the timestamp range")?,
                ),
            },
        )
        .await?;
        let writer = PeerId::new(holder.peer)?;
        let registered = creation_scenarios::register(
            &mut holder.socket,
            database,
            writer,
            fleet_sync_protocol::RequestId::new(Uuid::now_v7())?,
        )
        .await?;
        creation_scenarios::registered(&registered, writer)?;
        let original = read_health(http, base, database).await?;
        let status = copy_status(&original)?.clone();
        assert_eq!(status.acked, position(0)?);
        assert_eq!(status.schema.version, 0);
        assert!(staged_parts(&original)?.is_empty());

        let staged_at = chrono::DateTime::from_timestamp_millis(created)
            .ok_or("staging time is outside the timestamp range")?;
        let (piece, transaction) = first_part(database)?;
        assert_eq!(
            apply(&mut holder.socket, piece.clone()).await?,
            ReplicationAnswer::PartStaged {
                transaction,
                index: 0,
                acked: position(0)?,
            }
        );
        let saved = StagedCopy {
            database,
            piece,
            transaction,
            staged_at,
            status,
        };
        assert_original_part(&read_health(http, base, database).await?, &saved, created)?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(saved)
    }
    .await;
    let closed = holder
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    let staged = proof?;
    closed?;
    Ok(staged)
}

async fn observe_expiry_and_restage(
    http: &reqwest::Client,
    base: &str,
    root: &std::path::Path,
    saved: &StagedCopy,
    expired: i64,
) -> ConformanceResult<()> {
    let deadline = saved.staged_at + chrono::TimeDelta::milliseconds(PART_RETENTION_MILLIS);
    assert!(
        expired >= deadline.timestamp_millis(),
        "Copy pruned before its saved deadline"
    );
    // The child's real startup alarm completed before this direct owning-store read.
    // No Health, session, time control or Repair has touched this process yet.
    assert_eq!(
        read_count(
            &root.join(format!("copy-{}.db", saved.database)),
            "SELECT COUNT(*) AS count FROM _fleet_sync_staged_parts"
        )
        .await?,
        0,
        "startup did not automatically expire the original staged Copy part"
    );
    let health = read_health(http, base, saved.database).await?;
    assert_eq!(copy_status(&health)?, &saved.status);
    assert!(staged_parts(&health)?.is_empty());

    let mut holder = registration::reconnect(base).await?;
    let proof = async {
        let before =
            chrono::DateTime::from_timestamp_millis(expired).ok_or("invalid restart time")?;
        assert_eq!(
            apply(&mut holder, saved.piece.clone()).await?,
            ReplicationAnswer::PartStaged {
                transaction: saved.transaction,
                index: 0,
                acked: position(0)?,
            }
        );
        let after = before;
        let health = read_health(http, base, saved.database).await?;
        assert_eq!(copy_status(&health)?, &saved.status);
        let [restaged] = staged_parts(&health)? else {
            return Err("exact restage did not retain one fresh part".into());
        };
        assert_eq!(restaged.transaction, saved.transaction);
        assert_eq!(restaged.index, 0);
        assert!(restaged.staged_at > saved.staged_at);
        // Millisecond storage may round down the instant just before send.
        assert!(restaged.staged_at.timestamp_millis() >= before.timestamp_millis());
        assert!(restaged.staged_at.timestamp_millis() <= after.timestamp_millis());
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let closed = holder
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    proof?;
    closed?;
    Ok(())
}

fn position(sequence: u64) -> ConformanceResult<ReplicationPosition> {
    Ok(ReplicationPosition::new(
        Epoch::new(1)?,
        CommitSequence::new(sequence),
    ))
}

fn first_part(database: DatabaseId) -> ConformanceResult<(Piece<BoardAnnotation>, TransactionKey)> {
    let transaction = TransactionKey {
        epoch: Epoch::new(1)?,
        commit_id: CommitSequence::new(1),
    };
    let first = "CREATE TABLE alarm_records(id TEXT NOT NULL PRIMARY KEY)";
    let second = "CREATE INDEX alarm_records_index ON alarm_records(id)";
    let whole = ReplicatedTransaction::SchemaStep {
        commit_id: transaction.commit_id,
        migration_version: 1,
        statements: vec![first.to_owned(), second.to_owned()],
    };
    Ok((
        Piece::new(PieceContents {
            database,
            epoch: transaction.epoch,
            continues_from: position(0)?,
            through: position(1)?,
            transactions: vec![ReplicatedTransaction::SchemaStep {
                commit_id: transaction.commit_id,
                migration_version: 1,
                statements: vec![first.to_owned()],
            }],
            part: Some(TransactionPart::new(
                transaction,
                0,
                2,
                PieceDigest::of_transaction(&whole)?,
            )?),
            annotation: None,
        })?,
        transaction,
    ))
}

async fn apply(
    socket: &mut WebSocket,
    piece: Piece<BoardAnnotation>,
) -> ConformanceResult<ReplicationAnswer> {
    match creation_scenarios::operation_answer(
        socket,
        OperationRequest::Replication(ReplicationRequest::Piece(piece)),
    )
    .await?
    {
        OperationAnswer::Replication(answer) => Ok(answer),
        other => Err(format!(
            "staged Copy request returned another family: {}",
            serde_json::to_string(&other)?
        )
        .into()),
    }
}

async fn read_health(
    http: &reqwest::Client,
    base: &str,
    database: DatabaseId,
) -> ConformanceResult<LiveCopyHealth> {
    match http
        .get(format!("{base}/admin/v1/copies/{database}/health"))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .send()
        .await?
        .error_for_status()?
        .json::<CopyHealth>()
        .await?
    {
        CopyHealth::Live(health) => Ok(*health),
        other @ CopyHealth::Retired { .. } => {
            Err(format!("staged Copy lost its live Health projection: {other:?}").into())
        }
    }
}

fn staged_parts(
    health: &LiveCopyHealth,
) -> ConformanceResult<&[collaboration_central_admin_protocol::StagedPartHealth]> {
    match &health.staged_parts {
        CopyHealthRead::Known(parts) => Ok(parts),
        CopyHealthRead::Unreadable { detail } => {
            Err(format!("staged Copy Health unreadable: {detail}").into())
        }
    }
}

fn copy_status(health: &LiveCopyHealth) -> ConformanceResult<&CopyStatus> {
    match &health.copy {
        CopyHealthRead::Known(Some(status)) => Ok(status),
        _ => Err("staged Copy Health omitted its committed status".into()),
    }
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
fn assert_original_part(
    health: &LiveCopyHealth,
    saved: &StagedCopy,
    now: i64,
) -> ConformanceResult<()> {
    assert!(
        now < saved.staged_at.timestamp_millis() + PART_RETENTION_MILLIS,
        "restart missed the pre-expiry barrier"
    );
    assert_eq!(copy_status(health)?, &saved.status);
    let [part] = staged_parts(health)? else {
        return Err("original staged part did not survive process restart".into());
    };
    assert_eq!(part.transaction, saved.transaction);
    assert_eq!(part.index, 0);
    assert_eq!(part.staged_at, saved.staged_at);
    Ok(())
}
