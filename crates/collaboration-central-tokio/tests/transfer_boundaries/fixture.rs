//! Normal creation, owner requests and socket controls shared by transfer cases.
use super::*;
use collaboration_central_conformance::{
    DispatchFaultObservation, RuntimeControl, RuntimeControlRequest, RuntimeFaultObservation,
    owner_assertion,
};
use collaboration_central_core::ObjectAddress;

pub(super) const OLD_SERVICE: &str = "550e8400-e29b-41d4-a716-446655440792";
pub(super) const OLD_PEER: &str = "019f0000-0000-7000-8000-000000000792";
pub(super) const TARGET_SERVICE: &str = "550e8400-e29b-41d4-a716-446655440793";
pub(super) const TARGET_PEER: &str = "019f0000-0000-7000-8000-000000000793";
pub(super) const TARGET_CREDENTIAL: &str = "transfer support target";

pub(super) fn position(epoch: u64, sequence: u64) -> ConformanceResult<ReplicationPosition> {
    Ok(ReplicationPosition::new(
        Epoch::new(epoch)?,
        CommitSequence::new(sequence),
    ))
}
pub(super) struct Fixture {
    root: tempfile::TempDir,
    keys: JwksFixtureServer,
    server: TokioServer,
    pub(super) old: WebSocket,
    pub(super) target: WebSocket,
    pub(super) http: reqwest::Client,
    pub(super) database: DatabaseId,
}
pub(super) struct StoppedFixture {
    pub(super) root: tempfile::TempDir,
    pub(super) keys: JwksFixtureServer,
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
            holder_answer_timeout: Duration::from_secs(2),
            copy_ack_wait: Duration::from_secs(2),
        })
        .await?;
        creation::pin_registry_clock(
            server.base_url(),
            RegistryTime("2030-03-17T17:46:40Z".parse()?),
        )
        .await?;
        let client = ConformanceClient::new(server.base_url())?;
        let mut old = enroll_admitted_peer(
            &client,
            OLD_SERVICE,
            OLD_PEER,
            "transfer support holder",
            &Uuid::now_v7().to_string(),
        )
        .await?;
        let target = enroll_admitted_peer(
            &client,
            TARGET_SERVICE,
            TARGET_PEER,
            TARGET_CREDENTIAL,
            &Uuid::now_v7().to_string(),
        )
        .await?;
        let database = DatabaseId::new(Uuid::now_v7())?;
        let workspace = WorkspaceId::generate();
        let board = BoardId::generate();
        for operation in [
            FleetOperation::WorkspaceCreate {
                name: ResourceName::try_from("transfer boundary workspace".to_owned())?,
                description: Description::try_from("schema, floors and ordering".to_owned())?,
            },
            FleetOperation::BoardCreate {
                board: board.clone(),
                name: ResourceName::try_from("transfer boundary board".to_owned())?,
                description: Description::try_from("normal public owner transfer".to_owned())?,
                board_kind: message_board::BoardKind::Area {},
            },
            creation::project(database, board)?,
        ] {
            creation::accepted(creation::metadata(&mut old.socket, &workspace, operation).await?)?;
        }
        creation::registered(
            &creation::register(
                &mut old.socket,
                database,
                PeerId::try_from(OLD_PEER.to_owned())?,
                RequestId::new(Uuid::now_v7())?,
            )
            .await?,
            PeerId::try_from(OLD_PEER.to_owned())?,
        )?;
        let schema=Piece::<BoardAnnotation>::new(PieceContents {database,epoch:Epoch::new(1)?,continues_from:position(1,0)?,through:position(1,1)?,
            transactions:vec![ReplicatedTransaction::SchemaStep {commit_id:CommitSequence::new(1),migration_version:1,statements:vec!["CREATE TABLE transfer_rows(record_key TEXT NOT NULL PRIMARY KEY,number INTEGER NOT NULL,label TEXT NOT NULL)".into()]}],part:None,annotation:None})?;
        if !matches!(creation::operation_answer(&mut old.socket,OperationRequest::Replication(ReplicationRequest::Piece(schema))).await?,OperationAnswer::Replication(ReplicationAnswer::Applied {through}) if through==position(1,1)?)
        {
            return Err("transfer setup did not apply its real schema1 prefix".into());
        }
        println!("TRANSFER_BOUNDARY_SETUP normal_enrollment_Fleet_Register_schema1 passed");
        Ok(Self {
            root,
            keys,
            server,
            old: old.socket,
            target: target.socket,
            database,
            http: reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(10))
                .build()?,
        })
    }
    pub(super) fn base(&self) -> &str {
        self.server.base_url()
    }
    pub(super) fn route(&self, action: &str) -> String {
        format!(
            "{}/admin/v1/projects/{}/{action}",
            self.base(),
            self.database
        )
    }
    pub(super) async fn post(&self, action: &str, body: &Value) -> ConformanceResult<Value> {
        owner_registry_scenarios::post(&self.http, &self.route(action), body).await
    }
    pub(super) async fn complete(
        &self,
        action: &str,
        body: &Value,
        initial: Value,
    ) -> ConformanceResult<Value> {
        owner_registry_scenarios::complete(&self.http, &self.route(action), body, initial).await
    }
    pub(super) async fn preview(&self) -> ConformanceResult<TakeoverPreview> {
        let reply = self.post("takeover/preview", &Value::Null).await?;
        if reply.get("holderInstallation") != Some(&json!(OLD_SERVICE)) {
            return Err("transfer preview lost original holder installation".into());
        }
        Ok(serde_json::from_value(
            reply
                .get("preview")
                .cloned()
                .ok_or("transfer Preview omitted native confirmation")?,
        )?)
    }
    pub(super) fn takeover(&self, preview: TakeoverPreview, target: &str) -> Value {
        json!({"transferId":Uuid::now_v7(),"toInstallation":target,"reason":"normal transfer boundary","preview":preview})
    }
    pub(super) async fn target_schema(&mut self, maximum: u64) -> ConformanceResult<()> {
        let mut replacement = self
            .http
            .get(format!("{}/v1/session", self.base()))
            .bearer_auth(TARGET_CREDENTIAL)
            .header(
                "Cf-Access-Jwt-Assertion",
                machine_assertion(TARGET_CREDENTIAL)?,
            )
            .upgrade()
            .send()
            .await?
            .into_websocket()
            .await?;
        let session_id = SessionId::new(Uuid::now_v7())?;
        replacement
            .send(Message::Binary(
                serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Hello {
                    session_id,
                    schema_support: SchemaSupport {
                        max_version: maximum,
                    },
                })?
                .into(),
            ))
            .await?;
        let welcome = tokio::time::timeout(Duration::from_secs(5), replacement.next())
            .await?
            .ok_or("target closed before schema-support Welcome")??;
        let bytes = match welcome {
            Message::Binary(bytes) => bytes.to_vec(),
            Message::Text(text) => text.into_bytes(),
            _ => return Err("target schema support did not return Welcome data".into()),
        };
        if !matches!(serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&bytes)?,ServerFrame::Welcome {session_id:actual,databases} if actual==session_id && databases.is_empty())
        {
            return Err("schema-support replacement lost exact empty target Welcome".into());
        }
        std::mem::replace(&mut self.target, replacement)
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await?;
        Ok(())
    }
    pub(super) async fn status(&mut self) -> ConformanceResult<CopyStatus> {
        match creation::operation_answer(
            &mut self.old,
            OperationRequest::Replication(ReplicationRequest::CopyStatus {
                database: self.database,
            }),
        )
        .await?
        {
            OperationAnswer::Replication(ReplicationAnswer::CopyStatus(
                CopyStatusAnswer::Status(status),
            )) => Ok(status),
            _ => Err("transfer case lost its actual Copy status".into()),
        }
    }
    pub(super) async fn copy(
        &mut self,
        fence: u64,
        acked: ReplicationPosition,
        rows: Vec<Vec<SqlValue>>,
    ) -> ConformanceResult<SchemaIdentity> {
        let status = self.status().await?;
        let holder = if fence == 1 || fence == 5 {
            OLD_PEER
        } else {
            TARGET_PEER
        };
        if status.database != self.database
            || status.role != CopyRole::Authoritative
            || status.fence != Epoch::new(fence)?
            || status.acked != acked
            || status.holder != Some(PeerId::try_from(holder.to_owned())?)
            || status.schema.version != 1
        {
            return Err(format!(
                "transfer changed exact Copy holder/fence/schema/prefix: {status:?}"
            )
            .into());
        }
        let page = creation::operation_answer(
            &mut self.old,
            OperationRequest::Replication(ReplicationRequest::SnapshotPage {
                database: self.database,
                cursor: None,
            }),
        )
        .await?;
        if !matches!(page,OperationAnswer::Replication(ReplicationAnswer::SnapshotPage(SnapshotRead::Page(page))) if page.database==self.database && page.position==acked && page.schema==status.schema && page.table=="transfer_rows" && page.rows==rows && page.next.is_none())
        {
            return Err("transfer lost its exact complete row snapshot/schema/prefix".into());
        }
        Ok(status.schema)
    }
    pub(super) async fn unchanged(&mut self) -> ConformanceResult<SchemaIdentity> {
        self.copy(1, position(1, 1)?, vec![]).await
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
    pub(super) async fn held_copy(&self) -> ConformanceResult<Value> {
        let DispatchFaultObservation::Held {
            destination,
            request,
        } = self
            .control(RuntimeControl::AwaitHeldDispatch)
            .await?
            .dispatch
        else {
            return Err("transfer did not reach its actual held Copy call".into());
        };
        if destination != ObjectAddress::Copy(self.database) {
            return Err("transfer held a different Copy object".into());
        }
        Ok(serde_json::from_slice(&request)?)
    }
    pub(super) async fn fence_five(&self) -> ConformanceResult<()> {
        let request = FenceRequest {
            database: self.database,
            transfer_id: TransferId::new(Uuid::now_v7())?,
            new_epoch: Epoch::new(5)?,
            expect: FenceExpectation {
                epoch: Epoch::new(1)?,
                acked: position(1, 1)?,
            },
            target_schema: Some(SchemaSupport { max_version: 10 }),
            holder: PeerId::try_from(OLD_PEER.to_owned())?,
        };
        let reply:Value=self.http.post(format!("{}/admin/v1/conformance/copy-call",self.base())).header("Cf-Access-Jwt-Assertion",owner_assertion()?).json(&json!({"attempt":Uuid::now_v7(),"copy":configuration::registry_config()?.authoritative_receiver,"request":{"kind":"fence","value":request}})).send().await?.error_for_status()?.json().await?;
        if reply.pointer("/value/value")
            != Some(&json!(FenceResult::Fenced {
                old_epoch: Epoch::new(1)?,
                new_epoch: Epoch::new(5)?,
                start: position(1, 1)?
            }))
        {
            return Err(format!(
                "out-of-band setup did not commit exact public Copy Fence5: {reply}"
            )
            .into());
        }
        Ok(())
    }
    pub(super) async fn release(
        &mut self,
        request: &Value,
    ) -> ConformanceResult<(FrameId, ReleaseRequest)> {
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                let message = self
                    .old
                    .next()
                    .await
                    .ok_or("holder closed before correlated Release")??;
                let bytes = match message {
                    Message::Binary(bytes) => bytes.to_vec(),
                    Message::Text(text) => text.into_bytes(),
                    _ => return Err("holder Release was not a data frame".into()),
                };
                match serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&bytes)? {
                    ServerFrame::Request {
                        frame,
                        body:
                            ServerRequest::Crate(ServerCall::Release {
                                request: release, ..
                            }),
                    } if release.database == self.database
                        && release.epoch == Epoch::new(1)?
                        && request.get("transferId") == Some(&json!(release.transfer)) =>
                    {
                        return Ok((frame, release));
                    }
                    ServerFrame::Notice { .. } => {}
                    _ => {
                        return Err(
                            "holder Release lost exact database/epoch/transfer/frame correlation"
                                .into(),
                        );
                    }
                }
            }
        })
        .await?
    }
    pub(super) async fn reply_release(
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
    pub(super) async fn stop_for_copy_loss(self) -> ConformanceResult<StoppedFixture> {
        let old = self
            .old
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await;
        let target = self
            .target
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await;
        let server = self.server.shutdown().await;
        old?;
        target?;
        server?;
        Ok(StoppedFixture {
            root: self.root,
            keys: self.keys,
            http: self.http,
            database: self.database,
        })
    }
    pub(super) async fn close(self) -> ConformanceResult<tempfile::TempDir> {
        let old = self
            .old
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await;
        let target = self
            .target
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await;
        let server = self.server.shutdown().await;
        let keys = self.keys.shutdown().await;
        old?;
        target?;
        server?;
        keys?;
        Ok(self.root)
    }
}
