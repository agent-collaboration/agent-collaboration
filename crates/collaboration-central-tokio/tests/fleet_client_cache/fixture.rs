use super::*;
use board_sync_protocol::{BoardConsumerAnswer, BoardPayload, FleetRequestEnvelope};
use collaboration_central_conformance::AdmittedPeerSocket;
use fleet_admission::FleetRequestResult;
use fleet_sync_protocol::{FrameId, OperationAnswer, ServerFrame};
use sqlx_core::{
    connection::{ConnectOptions, Connection},
    query::query,
    row::Row,
};

pub struct Live {
    pub server: TokioServer,
    writer: AdmittedPeerSocket,
}
impl Live {
    pub async fn start(root: &Path, jwks: &str) -> TestResult<Self> {
        let server = TokioServer::start_conformance(TokioSettings {
            delivery: collaboration_central_tokio::DeliveryConfig {
                relay_peer: fleet_sync_protocol::PeerId::try_from(
                    "019f0000-0000-7000-8000-000000005240".to_owned(),
                )?,
            },
            root: root.into(),
            access_policy: fixture_policy(),
            jwks_url: jwks.into(),
            registry: configuration::registry_config()?,
            holder_answer_timeout: Duration::from_secs(2),
            copy_ack_wait: Duration::from_secs(2),
        })
        .await?;
        let writer = enroll_admitted_peer(
            &ConformanceClient::new(server.base_url())?,
            WRITER_SERVICE,
            "019f0000-0000-7000-8000-00000000100c",
            "cache-writer",
            &durable_steps::Uuid::now_v7().to_string(),
        )
        .await?;
        Ok(Self { server, writer })
    }
    pub async fn commit(&mut self, request: FleetRequest) -> TestResult {
        let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
        self.writer
            .socket
            .send(Message::Binary(
                serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request {
                    frame,
                    body: OperationRequest::Consumer(BoardConsumerRequest::Fleet(
                        FleetRequestEnvelope {
                            request_id: request.request_id.clone(),
                            operation: BoardPayload::encode(&request)?,
                        },
                    )),
                })?
                .into(),
            ))
            .await?;
        loop {
            let next = tokio::time::timeout(BOUND, self.writer.socket.next())
                .await?
                .ok_or("writer closed")??;
            let bytes = match next {
                Message::Binary(bytes) => bytes,
                Message::Text(text) => text.into_bytes().into(),
                _ => continue,
            };
            match serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&bytes)? {
                ServerFrame::Answer {
                    frame: actual,
                    body:
                        OperationAnswer::Consumer(BoardConsumerAnswer::Fleet {
                            request_id,
                            decision,
                        }),
                } if actual == frame && request_id == request.request_id => {
                    assert!(matches!(
                        decision.decode::<FleetRequestResult>()?,
                        FleetRequestResult::Decided(RequestDecision::Accepted {
                            change: ChangeOutcome::Changed,
                            ..
                        })
                    ));
                    return Ok(());
                }
                ServerFrame::Notice { .. } => {}
                _ => return Err("writer received a different correlated answer".into()),
            }
        }
    }
    pub async fn shutdown(self) -> TestResult {
        self.writer
            .socket
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await?;
        self.server.shutdown().await?;
        Ok(())
    }
}
pub struct Reader {
    pub fleet: Arc<FleetClient>,
    pub ingress: ingress::Ingress,
    stop: watch::Sender<bool>,
    task: tokio::task::JoinHandle<TestResult>,
}
impl Reader {
    pub async fn start(root: &Path, upstream: &str) -> TestResult<Self> {
        // Enrollment alone must not create a competing logical session. A
        // helper that also sends Hello would end the saved session on restart.
        let credential_hash = ring::digest::digest(&ring::digest::SHA256, b"cache-reader")
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let enrollment: serde_json::Value = reqwest::Client::new()
            .post(format!("{upstream}/admin/v1/machines/enroll"))
            .header("Cf-Access-Jwt-Assertion", collaboration_central_conformance::owner_assertion()?)
            .json(&serde_json::json!({"kind":"enrollMachine","value":{
                "label":"cache-reader", "installation":READER_SERVICE,
                "peer":"019f0000-0000-7000-8000-000000001f14",
                "access_client_id":collaboration_central_conformance::access_client_id("cache-reader")?,
                "credential_hash":credential_hash, "expires_at":"2033-05-18T03:33:20Z"
            }})).send().await?.error_for_status()?.json().await?;
        assert_eq!(
            enrollment.get("kind"),
            Some(&serde_json::json!("machineEnrolled"))
        );
        let ingress = ingress::Ingress::start(upstream).await?;
        let transport = ReqwestSessionTransport::new(
            AccessServiceToken::new("cache-fixture-client", "cache-fixture-secret")?,
            BOUND,
            BOUND,
        )?;
        let client = SessionClient::<_, _, BoardConsumerProtocol>::load(
            transport,
            MachineSessionStore::open(root).await?,
            &UuidV7Ids,
            SchemaSupport { max_version: 1 },
            Backoff {
                initial: Duration::from_millis(10),
                max: Duration::from_millis(100),
                factor: 2,
            },
        )
        .await?;
        let (requests, mut events, pump) = SessionPump::new(
            client,
            Endpoint::new(&ingress.endpoint)?,
            PeerCredential::new("Bearer cache-reader")?,
            BOUND,
        );
        let fleet =
            Arc::new(FleetClient::open(root, Arc::new(NoRegistration), Some(requests)).await?);
        let (stop, stopped) = watch::channel(false);
        let (ready, waiting) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let running = tokio::spawn(async move { pump.run(stopped).await });
            let mut ready = Some(ready);
            while let Some(event) = events.recv().await {
                if matches!(event, ClientEvent::Welcome { .. })
                    && let Some(ready) = ready.take()
                {
                    let _ = ready.send(());
                }
            }
            let result = running.await?;
            if let Err(error) = &result {
                eprintln!("cache reader pump failed before completion: {error:?}");
            }
            result?;
            Ok(())
        });
        tokio::time::timeout(BOUND, waiting).await??;
        Ok(Self {
            fleet,
            ingress,
            stop,
            task,
        })
    }
    pub async fn shutdown(self) -> TestResult {
        self.stop.send_replace(true);
        tokio::time::timeout(BOUND, self.task).await???;
        self.ingress.shutdown().await?;
        Arc::try_unwrap(self.fleet)
            .map_err(|_| "reader still in use")?
            .close()
            .await;
        Ok(())
    }
}
pub fn request(operation: FleetOperation) -> TestResult<FleetRequest> {
    Ok(FleetRequest {
        request_id: RequestId::generate(),
        workspace: WorkspaceId::try_from(WORKSPACE.to_owned())?,
        actor: RecordedActor::Human {
            human_id: HumanId::try_from("cache owner".to_owned())?,
        },
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation,
    })
}
pub fn create() -> TestResult<FleetRequest> {
    request(FleetOperation::WorkspaceCreate {
        name: ResourceName::try_from("old visible workspace".to_owned())?,
        description: Description::try_from("cache rebuild proof".to_owned())?,
    })
}
pub fn update(name: &str) -> TestResult<FleetRequest> {
    request(FleetOperation::WorkspaceUpdate {
        name: ResourceName::try_from(name.to_owned())?,
        description: Description::try_from("cache rebuild proof".to_owned())?,
    })
}
pub async fn age_feed(root: &Path) -> TestResult {
    // Called only after the owning server stops; next admission runs retention.
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join("fleet.db"))
        .connect()
        .await?;
    query::<sqlx_turso::Turso>("UPDATE fleet_feed SET as_of='2020-01-01T00:00:00.000000000Z'")
        .execute(&mut connection)
        .await?;
    connection.close().await?;
    Ok(())
}
pub async fn assert_visible(
    fleet: &FleetClient,
    root: &Path,
    name: &str,
    through: u64,
) -> TestResult {
    let reference = WorkspaceId::try_from(WORKSPACE.to_owned())?.into();
    assert!(
        matches!(fleet.record(&reference).await?, Some(Record::Workspace(workspace)) if workspace.name.as_str() == name)
    );
    let mut reader = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join("fleet-cache.sqlite"))
        .read_only(true)
        .connect()
        .await?;
    let row =
        query::<sqlx_turso::Turso>("SELECT position FROM fleet_cache_checkpoint WHERE singleton=1")
            .fetch_one(&mut reader)
            .await?;
    assert_eq!(u64::try_from(row.try_get::<i64, _>("position")?)?, through);
    reader.close().await?;
    Ok(())
}

pub async fn assert_floor(root: &Path, expected: i64) -> TestResult {
    let mut reader = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join("fleet.db"))
        .read_only(true)
        .connect()
        .await?;
    let row = query::<sqlx_turso::Turso>(
        "SELECT retained_through FROM fleet_feed_state WHERE name='fleet'",
    )
    .fetch_one(&mut reader)
    .await?;
    assert_eq!(row.try_get::<i64, _>("retained_through")?, expected);
    reader.close().await?;
    Ok(())
}
pub async fn assert_staging_rows(root: &Path, expected: i64) -> TestResult {
    let mut reader = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join("fleet-cache.sqlite"))
        .read_only(true)
        .connect()
        .await?;
    let row =
        query::<sqlx_turso::Turso>("SELECT COUNT(*) AS records FROM fleet_cache_rebuild_records")
            .fetch_one(&mut reader)
            .await?;
    assert_eq!(row.try_get::<i64, _>("records")?, expected);
    reader.close().await?;
    Ok(())
}
