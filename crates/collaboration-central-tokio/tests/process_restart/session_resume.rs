//! Lost Piece and renewal answers replay from owning stores after OS process death.
use super::*;
use collaboration_central_conformance::{AnswerFaultObservation, RuntimeControlRequest};
use collaboration_central_core::RegistryTime;
use fleet_sync_protocol::{
    ClientFrame, CommitSequence, DatabaseId, DatabaseStatus, Epoch, FrameId, LeaseAnswer,
    LeaseRequest, LeaseState, LeaseStatus, PeerId, Piece, PieceContents, RenewAnswer, RenewRequest,
    RenewResult, ReplicatedTransaction, ReplicationAnswer, ReplicationPosition, ReplicationRequest,
    SnapshotRead,
};
use futures_util::SinkExt;
use reqwest_websocket::{Message, WebSocket};
use runtime_primitives::SyncTime;

struct SavedSession {
    database: DatabaseId,
    piece: Piece<BoardAnnotation>,
    renewal: RenewRequest,
    at: RegistryTime,
    expires_at: SyncTime,
}

#[tokio::test]
async fn lost_piece_and_renewal_answers_replay_after_the_server_process_dies()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let first = ServerProcess::start(root.path(), &jwks.url).await?;
    let saved = prepare_lost_answers(&first.base).await;
    let crashed = first.crash().await;
    crashed?;
    let saved = match saved {
        Ok(saved) => saved,
        Err(error) => {
            jwks.shutdown().await?;
            return Err(error);
        }
    };
    let initial = [RuntimeControlRequest {
        address: ObjectAddress::Registry,
        command: RuntimeControl::SetTime {
            now: RegistryTime(saved.at.0 + chrono::TimeDelta::seconds(2)),
        },
    }];
    let second = ServerProcess::start_with_controls(root.path(), &jwks.url, &initial).await?;
    let proof = verify_recovered_answers(&second.base, &saved).await;
    let stopped = second.stop().await;
    let keys_stopped = jwks.shutdown().await;
    proof?;
    stopped?;
    keys_stopped?;
    println!("SESSION_RESUME_RESTART_CASE exact_ack_schema_and_original_renewal_receipt passed");
    Ok(())
}

async fn prepare_lost_answers(base: &str) -> ConformanceResult<SavedSession> {
    let mut holder = enroll_admitted_peer(
        &ConformanceClient::new(base)?,
        SERVICE,
        PEER,
        CREDENTIAL,
        &Uuid::now_v7().to_string(),
    )
    .await?;
    let proof = async {
        let at = RegistryTime(
            chrono::DateTime::from_timestamp_millis(chrono::Utc::now().timestamp_millis())
                .ok_or("session fixture time outside timestamp range")?,
        );
        pin_registry(base, at).await?;
        let workspace = WorkspaceId::generate();
        let board = BoardId::generate();
        let database = DatabaseId::new(Uuid::now_v7())?;
        for operation in [
            FleetOperation::WorkspaceCreate {
                name: ResourceName::try_from("lost session answers".to_owned())?,
                description: Description::try_from("ordinary Fleet creation".to_owned())?,
            },
            FleetOperation::BoardCreate {
                board: board.clone(),
                name: ResourceName::try_from("resume board".to_owned())?,
                description: Description::try_from("real Register and Copy".to_owned())?,
                board_kind: BoardKind::Area {},
            },
            creation_scenarios::project(database, board)?,
        ] {
            creation_scenarios::accepted(
                creation_scenarios::metadata(&mut holder.socket, &workspace, operation).await?,
            )?;
        }
        let registered = creation_scenarios::register(
            &mut holder.socket,
            database,
            PeerId::new(holder.peer)?,
            fleet_sync_protocol::RequestId::new(Uuid::now_v7())?,
        )
        .await?;
        creation_scenarios::registered(&registered, PeerId::new(holder.peer)?)?;
        let piece = Piece::new(PieceContents {
            database,
            epoch: Epoch::new(1)?,
            continues_from: position(0)?,
            through: position(1)?,
            transactions: vec![ReplicatedTransaction::SchemaStep {
                commit_id: CommitSequence::new(1),
                migration_version: 1,
                statements: vec![
                    "CREATE TABLE resume_records(id TEXT NOT NULL PRIMARY KEY)".to_owned(),
                ],
            }],
            part: None,
            annotation: None,
        })?;
        drop_committed_answer(
            base,
            &mut holder.socket,
            OperationRequest::Replication(ReplicationRequest::Piece(piece.clone())),
        )
        .await?;
        pin_registry(base, RegistryTime(at.0 + chrono::TimeDelta::seconds(1))).await?;
        let renewal = RenewRequest {
            attempt: fleet_sync_protocol::RequestId::new(Uuid::now_v7())?,
            database,
            epoch: Epoch::new(1)?,
        };
        drop_committed_answer(
            base,
            &mut holder.socket,
            OperationRequest::Lease(LeaseRequest::Renew(renewal.clone())),
        )
        .await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(SavedSession {
            database,
            piece,
            renewal,
            at,
            expires_at: SyncTime::from_unix_millis(at.0.timestamp_millis() + 1_000 + 14_400_000),
        })
    }
    .await;
    let closed = holder
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    let saved = proof?;
    closed?;
    // Read the committed expiry without consuming either original answer or
    // replaying the renewal attempt, exactly as the retained row requires.
    let (observer, databases) =
        registration::reconnect_peer_with_welcome(base, CREDENTIAL, 1).await?;
    let observed:ConformanceResult<()>=match databases.as_slice() {
        [DatabaseStatus::Ready {database,lease,..}] if *database==saved.database && *lease==expected_lease(&saved,14_400_000)? => Ok(()),
        _=>Err("prepared lost renewal was not visible with its literal expiry and authority in Welcome".into()),
    };
    let closed = observer
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    observed?;
    closed?;
    println!(
        "SESSION_RESUME_RESTART_STAGE both_answers_dropped_original_expiry_observed_without_replay"
    );
    Ok(saved)
}

async fn drop_committed_answer(
    base: &str,
    socket: &mut WebSocket,
    body: OperationRequest<BoardConsumerProtocol>,
) -> ConformanceResult<()> {
    let frame = FrameId::new(Uuid::now_v7())?;
    let address = ObjectAddress::Connection(SERVICE.to_owned().try_into()?);
    control(
        base,
        address.clone(),
        RuntimeControl::DropNextAnswer { frame },
    )
    .await?;
    socket
        .send(Message::Binary(
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request { frame, body })?
                .into(),
        ))
        .await?;
    let observed = control(base, address, RuntimeControl::AwaitDroppedAnswer { frame }).await?;
    if !matches!(observed.answer,AnswerFaultObservation::Dropped {frame:actual} if actual==frame) {
        return Err("session resume did not lose the exact original outgoing frame answer".into());
    }
    Ok(())
}

async fn verify_recovered_answers(base: &str, saved: &SavedSession) -> ConformanceResult<()> {
    let (mut socket, databases) =
        registration::reconnect_peer_with_welcome(base, CREDENTIAL, 1).await?;
    let proof=async {
        if databases != vec![DatabaseStatus::Ready {
            database:saved.database,acked:position(1)?,lease:expected_lease(saved,14_399_000)?,
        }] {
            return Err(format!("restart Welcome did not preserve the exact committed ack and lease: {databases:?}").into());
        }
        println!("SESSION_RESUME_RESTART_STAGE exact_recovered_welcome_before_any_replay");
        let replay=creation_scenarios::operation_answer(&mut socket,OperationRequest::Replication(ReplicationRequest::Piece(saved.piece.clone()))).await?;
        if !matches!(replay,OperationAnswer::Replication(ReplicationAnswer::AlreadyApplied {through}) if through==position(1)?) {
            return Err("lost schema piece did not replay AlreadyApplied after physical restart".into());
        }
        verify_snapshot(&mut socket,saved.database).await?;
        let mut previous_remaining=14_399_000;
        for elapsed_seconds in [2,3] {
            pin_registry(base,RegistryTime(saved.at.0+chrono::TimeDelta::seconds(elapsed_seconds))).await?;
            let response=creation_scenarios::operation_answer(&mut socket,OperationRequest::Lease(LeaseRequest::Renew(saved.renewal.clone()))).await?;
            let OperationAnswer::Lease(LeaseAnswer::Renew(RenewAnswer::Renewed {
                recorded,current,transfer_in_progress:None,replayed:true,
            }))=response else {
                return Err("dropped renewal did not replay its original terminal receipt without a transfer".into());
            };
            let remaining=14_401_000-u64::try_from(elapsed_seconds)?*1_000;
            if recorded != (RenewResult {epoch:Epoch::new(1)?,expires_at:saved.expires_at})
                || current != expected_lease(saved,remaining)?
                || current.remaining_ms>previous_remaining {
                return Err("lost renewal receipt changed its recorded expiry or authority after restart".into());
            }
            previous_remaining=current.remaining_ms;
        }
        verify_snapshot(&mut socket,saved.database).await?;
        println!("SESSION_RESUME_RESTART_STAGE exact_piece_and_twice_replayed_renewal_no_extension");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let closed = socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    proof?;
    closed?;
    Ok(())
}

async fn verify_snapshot(socket: &mut WebSocket, database: DatabaseId) -> ConformanceResult<()> {
    let page = creation_scenarios::operation_answer(
        socket,
        OperationRequest::Replication(ReplicationRequest::SnapshotPage {
            database,
            cursor: None,
        }),
    )
    .await?;
    if !matches!(page,OperationAnswer::Replication(ReplicationAnswer::SnapshotPage(SnapshotRead::Page(page)))
        if page.database==database && page.schema.version==1 && page.position==position(1)?
            && page.table=="resume_records" && page.rows.is_empty() && page.next.is_none())
    {
        return Err("replayed schema did not preserve its single original version and empty snapshot after restart".into());
    }
    Ok(())
}

fn position(sequence: u64) -> ConformanceResult<ReplicationPosition> {
    Ok(ReplicationPosition::new(
        Epoch::new(1)?,
        CommitSequence::new(sequence),
    ))
}

fn expected_lease(saved: &SavedSession, remaining_ms: u64) -> ConformanceResult<LeaseStatus> {
    Ok(LeaseStatus {
        database: saved.database,
        epoch: Epoch::new(1)?,
        holder: PeerId::try_from(PEER.to_owned())?,
        state: LeaseState::Held,
        expires_at: saved.expires_at,
        remaining_ms,
    })
}

async fn pin_registry(base: &str, now: RegistryTime) -> ConformanceResult<()> {
    control(
        base,
        ObjectAddress::Registry,
        RuntimeControl::SetTime { now },
    )
    .await?;
    Ok(())
}
