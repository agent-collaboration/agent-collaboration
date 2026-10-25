//! Reuse normal Fleet/session setup; public library grants are arrangement only.
use super::*;
use collaboration_central_core::CoreHost;
use durable_steps::{AtomicStore, StoreError};
use fleet_sync_lease::RegistrySnapshot;
use fleet_sync_tokio_runner::{ObjectKey, ObjectStore};
use std::sync::Arc;

pub(super) struct Fixture {
    root: tempfile::TempDir,
    keys: JwksFixtureServer,
    server: TokioServer,
    pub http: reqwest::Client,
    pub old: WebSocket,
    pub(super) target: reqwest_websocket::WebSocket,
    pub database: DatabaseId,
}
impl Fixture {
    pub async fn open() -> ConformanceResult<Self> {
        let root = tempfile::tempdir()?;
        let keys = JwksFixtureServer::start().await?;
        let config = configuration::registry_config()?;
        let database = DatabaseId::new(durable_steps::Uuid::now_v7())?;
        let server = support::start(root.path(), &keys.url).await?;
        let mut old = support::admit(&server).await?;
        let arranged = support::create_graph(
            &mut old.socket,
            &WorkspaceId::generate(),
            &BoardId::generate(),
            &[database],
        )
        .await;
        let closed = old
            .socket
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await;
        server.shutdown().await?;
        closed?;
        arranged?;
        // Only the initial registration is arranged through an existing public
        // task helper. Its gate is a setup stand-in; all admin actions are real.
        registry_setup::register_grant_history(
            root.path(),
            config,
            database,
            vec![(Epoch::new(1)?, PeerId::try_from(PEER.to_owned())?)],
        )
        .await?;
        let server = support::start(root.path(), &keys.url).await?;
        let mut old = support::connect(&server).await?.socket;
        // Setup's retained clock is old. A genuine current renewal establishes
        // the Move recency predicate instead of changing production policy.
        if !matches!(
            support::answer(
                &mut old,
                OperationRequest::Lease(LeaseRequest::Renew(RenewRequest {
                    attempt: RequestId::new(durable_steps::Uuid::now_v7())?,
                    database,
                    epoch: Epoch::new(1)?,
                }))
            )
            .await?,
            OperationAnswer::Lease(LeaseAnswer::Renew(RenewAnswer::Renewed {
                replayed: false,
                ..
            }))
        ) {
            return Err("owner Registry setup failed its normal current-holder renewal".into());
        }
        let target = enroll_admitted_peer(
            &ConformanceClient::new(server.base_url())?,
            TARGET_SERVICE,
            TARGET_PEER,
            "owner registry target",
            &durable_steps::Uuid::now_v7().to_string(),
        )
        .await?;
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()?;
        Ok(Self {
            root,
            keys,
            server,
            http,
            old,
            target: target.socket,
            database,
        })
    }
    pub async fn target_schema(&mut self, max_version: u64) -> ConformanceResult<()> {
        use reqwest_websocket::RequestBuilderExt;
        let mut replacement = self
            .http
            .get(format!("{}/v1/session", self.server.base_url()))
            .bearer_auth("owner registry target")
            .header(
                "Cf-Access-Jwt-Assertion",
                collaboration_central_conformance::machine_assertion("owner registry target")?,
            )
            .upgrade()
            .send()
            .await?
            .into_websocket()
            .await?;
        let session_id = SessionId::new(durable_steps::Uuid::now_v7())?;
        replacement
            .send(Message::Binary(
                serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Hello {
                    session_id,
                    schema_support: SchemaSupport { max_version },
                })?
                .into(),
            ))
            .await?;
        let received = tokio::time::timeout(Duration::from_secs(8), replacement.next())
            .await?
            .ok_or("target ended before its new schema Hello")??;
        let bytes = match received {
            Message::Binary(bytes) => bytes.to_vec(),
            Message::Text(text) => text.into_bytes(),
            other => return Err(format!("target schema Hello returned {other:?}").into()),
        };
        if !matches!(serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&bytes)?,ServerFrame::Welcome {session_id:actual,..} if actual==session_id)
        {
            return Err("target schema support was not admitted by the real Hello".into());
        }
        let previous = std::mem::replace(&mut self.target, replacement);
        previous
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await?;
        Ok(())
    }
    pub async fn copy_probe(&self, request: &Value) -> ConformanceResult<Value> {
        Ok(self
            .http
            .post(format!(
                "{}/admin/v1/conformance/copy-call",
                self.server.base_url()
            ))
            .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
            .json(request)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
    pub fn route(&self, action: &str) -> String {
        format!(
            "{}/admin/v1/projects/{}/{action}",
            self.server.base_url(),
            self.database
        )
    }
    pub async fn post(&self, action: &str, body: &Value) -> ConformanceResult<Value> {
        post(&self.http, &self.route(action), body).await
    }
    pub async fn preview(&self) -> ConformanceResult<Value> {
        self.post("takeover/preview", &Value::Null)
            .await?
            .get("preview")
            .cloned()
            .ok_or_else(|| "owner preview omitted its unchanged native confirmation value".into())
    }
    pub async fn control(
        &self,
        command: collaboration_central_conformance::RuntimeControl,
    ) -> ConformanceResult<collaboration_central_conformance::RuntimeFaultObservation> {
        Ok(self
            .http
            .post(format!(
                "{}/admin/v1/conformance/runtime",
                self.server.base_url()
            ))
            .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
            .json(&collaboration_central_conformance::RuntimeControlRequest {
                address: collaboration_central_core::ObjectAddress::Registry,
                command,
            })
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
    pub async fn renew_old(&mut self) -> ConformanceResult<()> {
        if !matches!(
            support::answer(
                &mut self.old,
                OperationRequest::Lease(LeaseRequest::Renew(RenewRequest {
                    attempt: RequestId::new(durable_steps::Uuid::now_v7())?,
                    database: self.database,
                    epoch: Epoch::new(1)?,
                }))
            )
            .await?,
            OperationAnswer::Lease(LeaseAnswer::Renew(RenewAnswer::Renewed {
                replayed: false,
                ..
            }))
        ) {
            return Err("presence fixture did not commit its real renewal".into());
        }
        Ok(())
    }
    pub async fn refresh_old_presence(&mut self) -> ConformanceResult<()> {
        let replacement = support::connect(&self.server).await?.socket;
        let previous = std::mem::replace(&mut self.old, replacement);
        previous
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await?;
        Ok(())
    }
    pub async fn complete(
        &self,
        action: &str,
        body: &Value,
        initial: Value,
    ) -> ConformanceResult<Value> {
        let mut reply = initial;
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                if reply.get("kind") != Some(&json!("pending")) {
                    return Ok(reply);
                }
                // An idempotent repeat reads the authoritative operation outcome.
                tokio::time::sleep(Duration::from_millis(20)).await;
                reply = self.post(action, body).await?;
            }
        })
        .await?
    }
    pub async fn close(self) -> ConformanceResult<tempfile::TempDir> {
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

pub(super) async fn post(
    http: &reqwest::Client,
    route: &str,
    body: &Value,
) -> ConformanceResult<Value> {
    let response = http
        .post(route)
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(body)
        .send()
        .await?;
    let status = response.status();
    let reply = response.json::<Value>().await?;
    if status != reqwest::StatusCode::OK {
        return Err(
            format!("normal owner Registry route {route} failed: {status}: {reply}").into(),
        );
    }
    Ok(reply)
}
pub(super) fn confirm(transfer: durable_steps::Uuid, preview: Value) -> Value {
    json!({"transferId":transfer,"toInstallation":TARGET_SERVICE,"reason":"owner takeover real preview","preview":preview})
}
pub(super) fn refused(reply: &Value, reason: &str) -> ConformanceResult<()> {
    if reply != &json!({"kind":"refused","value":{"kind":reason}}) {
        return Err(format!("owner grant did not preserve typed {reason}: {reply}").into());
    }
    Ok(())
}
pub(super) fn granted(
    reply: &Value,
    kind: &str,
    epoch: u64,
    start: ReplicationPosition,
) -> ConformanceResult<()> {
    if reply.get("kind") != Some(&json!(kind))
        || reply.pointer("/value/newEpoch") != Some(&json!(epoch))
        || reply.pointer("/value/start") != Some(&json!(start))
    {
        return Err(format!(
            "owner transfer did not return its committed {kind} epoch/start: {reply}"
        )
        .into());
    }
    Ok(())
}
pub(super) fn position(epoch: u64, sequence: u64) -> ConformanceResult<ReplicationPosition> {
    Ok(ReplicationPosition::new(
        Epoch::new(epoch)?,
        CommitSequence::new(sequence),
    ))
}
pub(super) async fn release(
    socket: &mut WebSocket,
) -> ConformanceResult<(FrameId, ReleaseRequest)> {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let message = socket
                .next()
                .await
                .ok_or("old holder closed before Release")??;
            let bytes = match message {
                Message::Binary(bytes) => bytes.to_vec(),
                Message::Text(text) => text.into_bytes(),
                _ => continue,
            };
            match serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&bytes)? {
                ServerFrame::Request {
                    frame,
                    body: ServerRequest::Crate(ServerCall::Release { request, .. }),
                } => return Ok((frame, request)),
                ServerFrame::Notice { .. } => {}
                _ => return Err("owner Move sent an unexpected old-holder frame".into()),
            }
        }
    })
    .await?
}
pub(super) async fn snapshot(
    root: &Path,
    database: DatabaseId,
) -> ConformanceResult<RegistrySnapshot> {
    let objects = ObjectStore::open(root).await?;
    let receiver = configuration::registry_config()?.authoritative_receiver;
    let registry = objects
        .object(
            ObjectKey::new("registry")?,
            Default::default(),
            Arc::new(CoreHost::for_store(native_fleet::Io::default(), receiver)),
        )
        .await?;
    let observed = registry
        .transaction(move |scope| {
            Box::pin(async move {
                fleet_sync_lease::registry_snapshot(scope, database)
                    .await
                    .map_err(StoreError::backend)
            })
        })
        .await;
    objects.close().await?;
    observed?.ok_or_else(|| "owner action has no reopened public Registry snapshot".into())
}
