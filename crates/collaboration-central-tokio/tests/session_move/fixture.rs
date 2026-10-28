//! Normal creation and socket transport for the session Move obligations.
use super::*;
use collaboration_central_conformance::{
    ConformanceClient, JwksFixtureServer, RuntimeControlRequest, RuntimeFaultObservation,
    enroll_admitted_peer, fixture_policy, owner_assertion,
};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use fleet_admission::FleetOperation;
use futures_util::StreamExt;
use message_board::{BoardId, Description, ResourceName, WorkspaceId};
use reqwest_websocket::WebSocket;

pub(super) const OLD_PEER: &str = "019f0000-0000-7000-8000-000000000751";
pub(super) const TARGET_PEER: &str = "019f0000-0000-7000-8000-000000000752";
pub(super) const TARGET_SERVICE: &str = "550e8400-e29b-41d4-a716-446655440752";
const ROW_KEY: &str = "019f0000-0000-7000-8000-000000000753";

pub(super) fn position(epoch: u64, sequence: u64) -> ConformanceResult<ReplicationPosition> {
    Ok(ReplicationPosition::new(
        Epoch::new(epoch)?,
        CommitSequence::new(sequence),
    ))
}
pub(super) fn time(offset_ms: i64) -> ConformanceResult<SyncTime> {
    let start: chrono::DateTime<chrono::Utc> = "2030-03-17T17:46:40Z".parse()?;
    Ok(SyncTime::from_unix_millis(
        start.timestamp_millis() + offset_ms,
    ))
}
pub(super) fn lease(
    database: DatabaseId,
    epoch: u64,
    holder: &str,
) -> ConformanceResult<LeaseStatus> {
    Ok(LeaseStatus {
        database,
        epoch: Epoch::new(epoch)?,
        holder: PeerId::try_from(holder.to_owned())?,
        state: LeaseState::Held,
        expires_at: time(14_400_000)?,
        remaining_ms: 14_400_000,
    })
}
pub(super) struct Fixture {
    _root: tempfile::TempDir,
    keys: JwksFixtureServer,
    server: TokioServer,
    pub(super) old: WebSocket,
    pub(super) target: WebSocket,
    pub(super) http: reqwest::Client,
    pub(super) database: DatabaseId,
}
impl Fixture {
    pub(super) async fn open() -> ConformanceResult<Self> {
        let root = tempfile::tempdir()?;
        let keys = JwksFixtureServer::start().await?;
        let server = TokioServer::start(TokioSettings {
            root: root.path().into(),
            access_policy: fixture_policy(),
            jwks_url: keys.url.clone(),
            registry: configuration::registry_config()?,
            delivery: collaboration_central_tokio::DeliveryConfig {
                relay_peer: PeerId::try_from("019f0000-0000-7000-8000-000000005240".to_owned())?,
            },
            holder_answer_timeout: Duration::from_secs(10),
            copy_ack_wait: Duration::from_secs(10),
        })
        .await?;
        creation_scenarios::pin_registry_clock(
            server.base_url(),
            collaboration_central_core::RegistryTime("2030-03-17T17:46:40Z".parse()?),
        )
        .await?;
        let client = ConformanceClient::new(server.base_url())?;
        let mut old = enroll_admitted_peer(
            &client,
            "550e8400-e29b-41d4-a716-446655440751",
            OLD_PEER,
            "normal Move old holder",
            &Uuid::now_v7().to_string(),
        )
        .await?;
        let target = enroll_admitted_peer(
            &client,
            TARGET_SERVICE,
            TARGET_PEER,
            "normal Move target",
            &Uuid::now_v7().to_string(),
        )
        .await?;
        let database = DatabaseId::new(Uuid::now_v7())?;
        let workspace = WorkspaceId::generate();
        let board = BoardId::generate();
        for operation in [
            FleetOperation::WorkspaceCreate {
                name: ResourceName::try_from("normal Move workspace".to_owned())?,
                description: Description::try_from("real holder release".to_owned())?,
            },
            FleetOperation::BoardCreate {
                board: board.clone(),
                name: ResourceName::try_from("normal Move board".to_owned())?,
                description: Description::try_from("copy catches drained prefix".to_owned())?,
                board_kind: message_board::BoardKind::Area {},
            },
            creation_scenarios::project(database, board)?,
        ] {
            creation_scenarios::accepted(
                creation_scenarios::metadata(&mut old.socket, &workspace, operation).await?,
            )?;
        }
        creation_scenarios::registered(
            &creation_scenarios::register(
                &mut old.socket,
                database,
                PeerId::try_from(OLD_PEER.to_owned())?,
                RequestId::new(Uuid::now_v7())?,
            )
            .await?,
            PeerId::try_from(OLD_PEER.to_owned())?,
        )?;
        let schema=Piece::<BoardAnnotation>::new(PieceContents {database,epoch:Epoch::new(1)?,continues_from:position(1,0)?,through:position(1,1)?,
            transactions:vec![ReplicatedTransaction::SchemaStep {commit_id:CommitSequence::new(1),migration_version:1,statements:vec!["CREATE TABLE move_records(record_key TEXT NOT NULL PRIMARY KEY,id INTEGER NOT NULL,label TEXT NOT NULL)".into()]}],part:None,annotation:None})?;
        if !matches!(creation_scenarios::operation_answer(&mut old.socket,OperationRequest::Replication(ReplicationRequest::Piece(schema))).await?,OperationAnswer::Replication(ReplicationAnswer::Applied {through}) if through==position(1,1)?)
        {
            return Err("Move fixture did not apply its normal schema1 prefix".into());
        }
        Ok(Self {
            _root: root,
            keys,
            server,
            old: old.socket,
            target: target.socket,
            http: reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(15))
                .build()?,
            database,
        })
    }
    pub(super) fn base(&self) -> &str {
        self.server.base_url()
    }
    pub(super) fn route(&self) -> String {
        format!("{}/admin/v1/projects/{}/move", self.base(), self.database)
    }
    pub(super) async fn post(&self, body: &Value) -> ConformanceResult<Value> {
        owner_registry_scenarios::post(&self.http, &self.route(), body).await
    }
    pub(super) fn start_move(
        &self,
        callers: &mut tokio::task::JoinSet<ConformanceResult<Value>>,
        body: Value,
    ) {
        let http = self.http.clone();
        let route = self.route();
        callers.spawn(async move { owner_registry_scenarios::post(&http, &route, &body).await });
    }
    pub(super) async fn control(
        &self,
        command: RuntimeControl,
    ) -> ConformanceResult<RuntimeFaultObservation> {
        Ok(self
            .http
            .post(format!("{}/admin/v1/conformance/runtime", self.base()))
            .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
            .json(&RuntimeControlRequest {
                address: ObjectAddress::Registry,
                command,
            })
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
    pub(super) async fn release(
        &mut self,
        body: &Value,
    ) -> ConformanceResult<(FrameId, ReleaseRequest)> {
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                let message = self
                    .old
                    .next()
                    .await
                    .ok_or("Move holder closed before Release")??;
                let bytes = match message {
                    Message::Binary(bytes) => bytes.to_vec(),
                    Message::Text(text) => text.into_bytes(),
                    _ => return Err("Move holder did not receive a Release data frame".into()),
                };
                match serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&bytes)? {
                    ServerFrame::Request {
                        frame,
                        body: ServerRequest::Crate(ServerCall::Release { request, .. }),
                    } => {
                        if request.database != self.database
                            || request.epoch != Epoch::new(1)?
                            || Some(&json!(request.transfer)) != body.get("transferId")
                        {
                            return Err(
                                "Move Release lost exact database/epoch/transfer correlation"
                                    .into(),
                            );
                        }
                        return Ok((frame, request));
                    }
                    ServerFrame::Notice { .. } => {}
                    _ => return Err("Move holder received another response before Release".into()),
                }
            }
        })
        .await?
    }
    pub(super) async fn reply(
        &mut self,
        frame: FrameId,
        answer: ReleaseAnswer,
    ) -> ConformanceResult<()> {
        self.old
            .send(Message::Binary(
                serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Answer {
                    frame,
                    body: ClientAnswer::Crate(ServerCallAnswer::Release(answer)),
                })?
                .into(),
            ))
            .await?;
        Ok(())
    }
    pub(super) async fn renew_old(&mut self) -> ConformanceResult<RenewAnswer> {
        renew(&mut self.old, self.database, 1).await
    }
    pub(super) async fn target_renewed(&mut self) -> ConformanceResult<()> {
        let answer = renew(&mut self.target, self.database, 2).await?;
        if answer
            != (RenewAnswer::Renewed {
                recorded: RenewResult {
                    epoch: Epoch::new(2)?,
                    expires_at: time(14_400_000)?,
                },
                current: lease(self.database, 2, TARGET_PEER)?,
                transfer_in_progress: None,
                replayed: false,
            })
        {
            return Err("Move did not leave exactly epoch2 and a usable target lease".into());
        }
        Ok(())
    }
    pub(super) async fn apply_drained_row(&mut self) -> ConformanceResult<()> {
        let piece = Piece::<BoardAnnotation>::new(PieceContents {
            database: self.database,
            epoch: Epoch::new(1)?,
            continues_from: position(1, 1)?,
            through: position(1, 2)?,
            transactions: vec![ReplicatedTransaction::Rows {
                commit_id: CommitSequence::new(2),
                rows: vec![RowChange::upsert(
                    "move_records",
                    vec![SqlValue::Text(ROW_KEY.into())],
                    vec![
                        SqlValue::Text(ROW_KEY.into()),
                        SqlValue::Integer(2),
                        SqlValue::Text("drained".into()),
                    ],
                )?],
            }],
            part: None,
            annotation: None,
        })?;
        if !matches!(creation_scenarios::operation_answer(&mut self.old,OperationRequest::Replication(ReplicationRequest::Piece(piece))).await?,OperationAnswer::Replication(ReplicationAnswer::Applied {through}) if through==position(1,2)?)
        {
            return Err(
                "Move did not apply the exact drained row through the original holder".into(),
            );
        }
        Ok(())
    }
    pub(super) async fn copy_unchanged(
        &mut self,
        fence: u64,
        acked: ReplicationPosition,
        has_row: bool,
    ) -> ConformanceResult<()> {
        check_copy(&mut self.old, self.database, fence, acked, has_row).await
    }
    pub(super) async fn copy_from_target_unchanged(&mut self) -> ConformanceResult<()> {
        check_copy(&mut self.target, self.database, 1, position(1, 1)?, false).await
    }
    pub(super) async fn disconnect_old(&mut self) -> ConformanceResult<()> {
        let service =
            message_board::ServiceId::try_from("550e8400-e29b-41d4-a716-446655440751".to_owned())?;
        if self
            .server
            .conformance_platform()
            .conformance_socket_origins(&service)
            .await?
            .len()
            != 1
        {
            return Err("disconnect fixture did not own exactly one real attached socket".into());
        }
        SinkExt::close(&mut self.old).await?;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if self
                    .server
                    .conformance_platform()
                    .conformance_socket_origins(&service)
                    .await?
                    .is_empty()
                {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                }
                // Only the owning platform's read-only attachment observation establishes close readiness.
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await??;
        Ok(())
    }
    pub(super) async fn close(self) -> ConformanceResult<()> {
        let _ = self
            .old
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await;
        let target = self
            .target
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await;
        let server = self.server.shutdown().await;
        let keys = self.keys.shutdown().await;
        target?;
        server?;
        keys?;
        Ok(())
    }
}
async fn renew(
    socket: &mut WebSocket,
    database: DatabaseId,
    epoch: u64,
) -> ConformanceResult<RenewAnswer> {
    match creation_scenarios::operation_answer(
        socket,
        OperationRequest::Lease(LeaseRequest::Renew(RenewRequest {
            database,
            epoch: Epoch::new(epoch)?,
            attempt: RequestId::new(Uuid::now_v7())?,
        })),
    )
    .await?
    {
        OperationAnswer::Lease(LeaseAnswer::Renew(answer)) => Ok(answer),
        _ => Err("Move renewal returned another answer family".into()),
    }
}
async fn check_copy(
    socket: &mut WebSocket,
    database: DatabaseId,
    fence: u64,
    acked: ReplicationPosition,
    has_row: bool,
) -> ConformanceResult<()> {
    if !matches!(creation_scenarios::operation_answer(socket,OperationRequest::Replication(ReplicationRequest::CopyStatus {database})).await?,OperationAnswer::Replication(ReplicationAnswer::CopyStatus(CopyStatusAnswer::Status(status))) if status.fence==Epoch::new(fence)? && status.acked==acked && status.schema.version==1)
    {
        return Err("Move changed the exact Copy fence/prefix/schema".into());
    }
    let expected = if has_row {
        vec![vec![
            SqlValue::Text(ROW_KEY.into()),
            SqlValue::Integer(2),
            SqlValue::Text("drained".into()),
        ]]
    } else {
        Vec::new()
    };
    if !matches!(creation_scenarios::operation_answer(socket,OperationRequest::Replication(ReplicationRequest::SnapshotPage {database,cursor:None})).await?,OperationAnswer::Replication(ReplicationAnswer::SnapshotPage(SnapshotRead::Page(page))) if page.database==database && page.position==acked && page.schema.version==1 && page.table=="move_records" && page.rows==expected && page.next.is_none())
    {
        return Err("Move changed the exact single-row/empty Copy snapshot".into());
    }
    Ok(())
}
