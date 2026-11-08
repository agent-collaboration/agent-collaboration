//! Canonical metadata, genuine task admission and normal native owners arrange Directory inputs.
use super::*;
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerRequest, BoardPayload, FleetRequestEnvelope,
};
use collaboration_central_conformance::{
    ConformanceClient, RuntimeControl, RuntimeControlRequest, fixture_policy,
};
use collaboration_central_core::{CoreHost, ObjectAddress, RegistryTime};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use durable_steps::{AtomicStore, JournalScope, StoreError};
use fleet_admission::{FleetOperation, FleetRequest, FleetRequestResult};
use fleet_tokio_objects::{ObjectKey, ObjectStore, TokioObject};
use std::sync::Arc;

pub(super) const NOW: i64 = 1_792_281_600_000;

pub(super) async fn start(root: &Path, jwks: &str, now: i64) -> ConformanceResult<TokioServer> {
    Ok(TokioServer::start_with_controls(
        TokioSettings {
            delivery: collaboration_central_tokio::DeliveryConfig {
                relay_peer: "019f0000-0000-7000-8000-000000005240"
                    .to_owned()
                    .try_into()?,
            },
            root: root.into(),
            access_policy: fixture_policy(),
            jwks_url: jwks.into(),
            registry: configuration::registry_config()?,
            holder_answer_timeout: Duration::from_secs(2),
            copy_ack_wait: Duration::from_secs(2),
        },
        vec![RuntimeControlRequest {
            address: ObjectAddress::Fleet,
            command: RuntimeControl::SetTime {
                now: RegistryTime(
                    chrono::DateTime::from_timestamp_millis(now)
                        .ok_or("Directory fixture time out of range")?,
                ),
            },
        }],
    )
    .await?)
}

pub(super) async fn import_graph(
    server: &TokioServer,
    workspace: &WorkspaceId,
    board: &BoardId,
    databases: &[DatabaseId; 4],
) -> ConformanceResult<()> {
    let retired_board = BoardId::generate();
    let bundle = serde_json::from_value(serde_json::json!({
        "declaredOwner":"owner","provenance":{"installation":SERVICE,"machine":"Directory annotation setup"},
        "records":[
            {"kind":"workspace","id":workspace,"name":"directory workspace","description":"Directory real session proof","repositories":[],"time":{"kind":"legacyUnknown"}},
            {"kind":"board","id":board,"workspace":workspace,"name":"directory board","description":"Directory real session proof","state":"active","time":{"kind":"legacyUnknown"}},
            {"kind":"board","id":retired_board,"workspace":workspace,"name":"retirement board","description":"Archived parent is an explicit setup input","state":"archived","time":{"kind":"legacyUnknown"}},
            {"kind":"directory","project":databases[2],"board":board,"title":"directory project 2","time":{"kind":"legacyUnknown"}},
            {"kind":"directory","project":databases[3],"board":retired_board,"title":"directory project 3","time":{"kind":"legacyUnknown"}}
        ]
    }))?;
    if !matches!(
        ConformanceClient::new(server.base_url())?
            .import_fleet(&bundle)
            .await?,
        fleet_admission::FleetImportResult::Imported { .. }
    ) {
        return Err("canonical mixed Directory graph was not imported".into());
    }
    Ok(())
}

pub(super) async fn create_projects(
    socket: &mut reqwest_websocket::WebSocket,
    workspace: &WorkspaceId,
    board: &BoardId,
    databases: &[DatabaseId],
) -> ConformanceResult<()> {
    for database in databases {
        let request = FleetRequest {
            request_id: message_board::RequestId::generate(),
            workspace: workspace.clone(),
            actor: message_board::RecordedActor::Human {
                human_id: "owner".to_owned().try_into()?,
            },
            acting_for: None,
            protocol_version: message_board::ProtocolVersion::CURRENT,
            operation: FleetOperation::ProjectDirectoryCreate {
                project: database.to_string().try_into()?,
                board: board.clone(),
                title: format!("normal Directory project {database}").try_into()?,
            },
        };
        let answer = support::answer(
            socket,
            OperationRequest::Consumer(BoardConsumerRequest::Fleet(FleetRequestEnvelope {
                request_id: request.request_id.clone(),
                operation: BoardPayload::encode(&request)?,
            })),
        )
        .await?;
        let OperationAnswer::Consumer(BoardConsumerAnswer::Fleet {
            request_id,
            decision,
        }) = answer
        else {
            return Err("Directory project creation returned another answer family".into());
        };
        if request_id != request.request_id
            || !matches!(
                decision.decode::<FleetRequestResult>()?,
                FleetRequestResult::Decided(message_board::RequestDecision::Accepted { .. })
            )
        {
            return Err("Directory project creation did not commit its accepted decision".into());
        }
    }
    Ok(())
}

pub(super) async fn register(
    socket: &mut reqwest_websocket::WebSocket,
    database: DatabaseId,
) -> ConformanceResult<()> {
    let answer = support::answer(
        socket,
        OperationRequest::Lease(LeaseRequest::Register(Registration {
            attempt: RequestId::new(durable_steps::Uuid::now_v7())?,
            database,
            writer: PEER.to_owned().try_into()?,
            origin: Origin::Empty,
        })),
    )
    .await
    .map_err(|error| format!("Directory normal Register for {database}: {error}"))?;
    if !matches!(
        answer,
        OperationAnswer::Lease(LeaseAnswer::Register(RegistrationAnswer::Registered {
            replayed: false,
            ..
        }))
    ) {
        return Err("Directory fixture normal Register did not grant".into());
    }
    Ok(())
}

pub(super) async fn admit_imports(root: &Path, databases: &[DatabaseId]) -> ConformanceResult<()> {
    type Store = TokioObject<CoreHost<native_fleet::Io>>;
    let objects = ObjectStore::open(root).await?;
    let store = objects
        .object(
            ObjectKey::new("fleet")?,
            Default::default(),
            Arc::new(CoreHost::for_store(
                native_fleet::Io::default(),
                configuration::registry_config()?.authoritative_receiver,
            )),
        )
        .await?;
    for database in databases {
        let input = Import {
            attempt: RequestId::new(durable_steps::Uuid::now_v7())?,
            database: *database,
            digest: ImportDigest::new("d".repeat(64))?,
        };
        let key = fleet_project_lease::lifecycle_request_key(input.attempt)?;
        store
            .transaction(move |scope| {
                Box::pin(async move {
                    // The type supplies the public task kind/version only. No NoHook
                    // engine executes: the reopened native Fleet registers its real hook.
                    scope
                        .admit::<fleet_project_lease::ImportTask<
                            Store,
                            sqlx_turso::Turso,
                            fleet_project_lease::NoLeaseChangeHook,
                        >>(&key, &input)
                        .await
                        .map(|_| ())
                        .map_err(StoreError::backend)
                })
            })
            .await?;
    }
    objects.close().await?;
    Ok(())
}

pub(super) async fn await_imports(
    socket: &mut reqwest_websocket::WebSocket,
    databases: &[DatabaseId],
) -> ConformanceResult<()> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let entries = support::directory(socket, DirectoryQuery::default()).await?;
            if databases.iter().all(|database| {
                entries.iter().any(|entry| {
                    entry.project.as_str() == database.to_string()
                        && matches!(
                            &entry.lease_side,
                            DirectorySide::Known(LeaseMetadata {
                                lease: DirectoryLease::Unheld,
                                ..
                            })
                        )
                })
            }) {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await?
}

pub(super) async fn retire(
    server: &TokioServer,
    database: DatabaseId,
) -> ConformanceResult<SyncTime> {
    use collaboration_central_admin_protocol::{OwnerRetirement, OwnerRetirementAnswer};
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(8))
        .build()?;
    let input = OwnerRetirement {
        request_id: RequestId::new(durable_steps::Uuid::now_v7())?,
        expect_fence: Epoch::new(1)?,
        expect_acked: ReplicationPosition::new(Epoch::new(1)?, CommitSequence::new(0)),
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let answer: OwnerRetirementAnswer = http
                .post(format!(
                    "{}/admin/v1/projects/{database}/retire",
                    server.base_url()
                ))
                .header(
                    "Cf-Access-Jwt-Assertion",
                    collaboration_central_conformance::owner_assertion()?,
                )
                .json(&input)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            match answer {
                OwnerRetirementAnswer::Retired { at } => return Ok(at),
                OwnerRetirementAnswer::Pending { .. } => tokio::task::yield_now().await,
                other => return Err(format!("real imported retirement refused: {other:?}").into()),
            }
        }
    })
    .await?
}
