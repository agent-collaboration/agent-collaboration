use super::*;
use collaboration_central_core::{
    CopyDirectorySummary, CoreHost, ObjectAddress, ObjectRuntime, PlatformIo,
};
use collaboration_central_tokio::TokioStorageHost;
use durable_steps::{AtomicStore, CallFailure, StoreError};
use fleet_sync_lease::{RegistryConfig, RegistrySnapshot};
use fleet_sync_tokio_runner::{ObjectKey, ObjectOptions, ObjectStore};
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{Message, RequestBuilderExt, WebSocket};
use runtime_primitives::{Clock, IdSource, WakeFailure, WakeScheduler};
use std::sync::Arc;

pub(super) async fn start(root: &Path, jwks: &str) -> ConformanceResult<TokioServer> {
    Ok(TokioServer::start_conformance(TokioSettings {
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
    .await?)
}
pub(super) async fn admit(
    server: &TokioServer,
) -> ConformanceResult<collaboration_central_conformance::AdmittedPeerSocket> {
    enroll_admitted_peer(
        &ConformanceClient::new(server.base_url())?,
        SERVICE,
        PEER,
        "directory writer",
        &durable_steps::Uuid::now_v7().to_string(),
    )
    .await
}
pub(super) struct ConnectedMachine {
    pub socket: WebSocket,
}
pub(super) async fn connect(server: &TokioServer) -> ConformanceResult<ConnectedMachine> {
    let mut socket = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(8))
        .build()?
        .get(format!("{}/v1/session", server.base_url()))
        .bearer_auth("directory writer")
        .header(
            "Cf-Access-Jwt-Assertion",
            collaboration_central_conformance::machine_assertion("directory writer")?,
        )
        .upgrade()
        .send()
        .await?
        .into_websocket()
        .await?;
    let session_id = SessionId::new(durable_steps::Uuid::now_v7())?;
    socket
        .send(Message::Binary(
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Hello {
                session_id,
                schema_support: SchemaSupport { max_version: 1 },
            })?
            .into(),
        ))
        .await?;
    let received = tokio::time::timeout(Duration::from_secs(8), socket.next())
        .await?
        .ok_or("reopened Directory session ended before Welcome")??;
    let bytes = match received {
        Message::Binary(bytes) => bytes.to_vec(),
        Message::Text(text) => text.into_bytes(),
        other => return Err(format!("reopened Directory session received {other:?}").into()),
    };
    assert!(
        matches!(serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&bytes)?,
        ServerFrame::Welcome {session_id:actual,..} if actual==session_id)
    );
    Ok(ConnectedMachine { socket })
}
pub(super) async fn answer(
    socket: &mut WebSocket,
    body: OperationRequest<BoardConsumerProtocol>,
) -> ConformanceResult<OperationAnswer<BoardConsumerProtocol>> {
    let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
    socket
        .send(Message::Binary(
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request { frame, body })?
                .into(),
        ))
        .await?;
    loop {
        let received = tokio::time::timeout(Duration::from_secs(8), socket.next())
            .await?
            .ok_or("Directory session ended before its answer")??;
        let bytes = match received {
            Message::Binary(bytes) => bytes.to_vec(),
            Message::Text(text) => text.into_bytes(),
            other => return Err(format!("Directory session received {other:?}").into()),
        };
        match serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&bytes)? {
            ServerFrame::Answer {
                frame: actual,
                body,
            } if actual == frame => return Ok(body),
            ServerFrame::Notice { .. } => {}
            _ => return Err("Directory answer lost exact frame correlation".into()),
        }
    }
}
pub(super) async fn directory(
    socket: &mut WebSocket,
    query: DirectoryQuery,
) -> ConformanceResult<Vec<DirectoryEntry>> {
    let OperationAnswer::Consumer(BoardConsumerAnswer::Read(BoardReadPage::Directory {
        entries,
        ..
    })) = answer(
        socket,
        OperationRequest::Consumer(BoardConsumerRequest::Read {
            scope: BoardReadScope::Directory {
                query: BoardPayload::encode(&query)?,
            },
            cursor: None,
        }),
    )
    .await?
    else {
        return Err("Directory read answered another family".into());
    };
    Ok(entries
        .into_iter()
        .map(BoardPayload::decode)
        .collect::<Result<Vec<_>, _>>()?)
}
pub(super) async fn create_graph(
    socket: &mut WebSocket,
    workspace: &WorkspaceId,
    board: &BoardId,
    databases: &[DatabaseId],
) -> ConformanceResult<()> {
    let mut operations = vec![
        FleetOperation::WorkspaceCreate {
            name: "directory workspace".to_owned().try_into()?,
            description: "Directory real-session proof".to_owned().try_into()?,
        },
        FleetOperation::BoardCreate {
            board: board.clone(),
            name: "directory board".to_owned().try_into()?,
            description: "Directory real-session proof".to_owned().try_into()?,
            board_kind: message_board::BoardKind::Area {},
        },
    ];
    for (index, database) in databases.iter().enumerate() {
        operations.push(FleetOperation::ProjectDirectoryCreate {
            project: ProjectId::try_from(database.to_string())?,
            board: board.clone(),
            title: format!("directory project {index}").try_into()?,
        });
    }
    for operation in operations {
        let request = FleetRequest {
            request_id: message_board::RequestId::generate(),
            workspace: workspace.clone(),
            actor: message_board::RecordedActor::Human {
                human_id: "owner".to_owned().try_into()?,
            },
            acting_for: None,
            protocol_version: message_board::ProtocolVersion::CURRENT,
            operation,
        };
        let OperationAnswer::Consumer(BoardConsumerAnswer::Fleet {
            request_id,
            decision,
        }) = answer(
            socket,
            OperationRequest::Consumer(BoardConsumerRequest::Fleet(FleetRequestEnvelope {
                request_id: request.request_id.clone(),
                operation: BoardPayload::encode(&request)?,
            })),
        )
        .await?
        else {
            return Err("Directory setup did not get its Fleet decision".into());
        };
        assert_eq!(request_id, request.request_id);
        assert!(matches!(
            decision.decode::<FleetRequestResult>()?,
            FleetRequestResult::Decided(message_board::RequestDecision::Accepted { .. })
        ));
    }
    Ok(())
}

#[derive(Clone)]
struct PublicationIo(Arc<native_fleet::Fixture>);
impl Clock for PublicationIo {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(1_792_281_600_000)
    }
}
impl IdSource for PublicationIo {
    fn new_id(&self) -> durable_steps::Uuid {
        durable_steps::Uuid::now_v7()
    }
}
impl WakeScheduler for PublicationIo {
    async fn wake_at(&self, _: SyncTime) -> Result<(), WakeFailure> {
        Ok(())
    }
}
impl PlatformIo for PublicationIo {
    async fn object_call(
        &self,
        destination: ObjectAddress,
        bytes: Vec<u8>,
    ) -> Result<Vec<u8>, CallFailure> {
        if destination != ObjectAddress::Fleet {
            return Err(CallFailure::AnswerLost);
        }
        self.0.runtime.on_call(bytes).await
    }
}
pub(super) async fn publish(
    root: &Path,
    config: RegistryConfig,
    databases: &[DatabaseId],
) -> ConformanceResult<Vec<(Option<CopyDirectorySummary>, RegistrySnapshot)>> {
    let fleet = Arc::new(native_fleet::Fixture::open(root).await?);
    let io = PublicationIo(fleet.clone());
    let objects = ObjectStore::open(root).await?;
    let registry = objects
        .object(
            ObjectKey::new("registry")?,
            Default::default(),
            Arc::new(CoreHost::for_store(
                io.clone(),
                config.authoritative_receiver,
            )),
        )
        .await?;
    let runtime = ObjectRuntime::registry(
        Arc::new(TokioStorageHost::new(registry.clone())),
        io.clone(),
        config,
        collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
    )?;
    runtime.initialize().await?;
    runtime.on_alarm().await?;
    let mut observed = Vec::new();
    for database in databases {
        let database = *database;
        let snapshot = registry
            .transaction(move |scope| {
                Box::pin(async move {
                    fleet_sync_lease::registry_snapshot(scope, database)
                        .await
                        .map_err(StoreError::backend)
                })
            })
            .await?
            .ok_or("public Registry snapshot missing")?;
        let copy_store = objects
            .object(
                ObjectKey::new(format!("copy-{database}"))?,
                ObjectOptions {
                    foreign_keys: false,
                },
                Arc::new(CoreHost::for_store(
                    io.clone(),
                    config.authoritative_receiver,
                )),
            )
            .await?;
        let copy = ObjectRuntime::copy(
            Arc::new(TokioStorageHost::new(copy_store)),
            io.clone(),
            config.authoritative_receiver,
            database,
            Duration::from_secs(2),
        );
        copy.initialize().await?;
        copy.on_alarm().await?;
        observed.push((copy.copy_summary().await?, snapshot));
    }
    objects.close().await?;
    fleet.objects.close().await?;
    Ok(observed)
}
