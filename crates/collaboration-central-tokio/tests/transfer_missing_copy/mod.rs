//! A real file loss after Preview must fail closed, then recover the same request.
use super::*;
use collaboration_central_admin_protocol::{
    OwnerBlockedSubject, OwnerTakeover, OwnerTransferAnswer,
};
use collaboration_central_conformance::owner_assertion;

mod storage_fault;
use storage_fault::{CopyBackup, read_registry};

struct RunningTargetSession {
    server: TokioServer,
    target: WebSocket,
}
impl RunningTargetSession {
    async fn open(stopped: &fixture::StoppedFixture) -> ConformanceResult<Self> {
        let server = TokioServer::start(TokioSettings {
            root: stopped.root.path().into(),
            access_policy: fixture_policy(),
            jwks_url: stopped.keys.url.clone(),
            registry: configuration::registry_config()?,
            delivery: collaboration_central_tokio::DeliveryConfig {
                relay_peer: PeerId::try_from("019f0000-0000-7000-8000-000000005240".to_owned())?,
            },
            holder_answer_timeout: Duration::from_secs(2),
            copy_ack_wait: Duration::from_secs(2),
        })
        .await?;
        let connected = async {
            creation::pin_registry_clock(
                server.base_url(),
                RegistryTime("2030-03-17T17:46:40Z".parse()?),
            )
            .await?;
            Self::connect_target(stopped, server.base_url()).await
        }
        .await;
        match connected {
            Ok(target) => Ok(Self { server, target }),
            Err(error) => {
                server.shutdown().await?;
                Err(error)
            }
        }
    }
    async fn connect_target(
        stopped: &fixture::StoppedFixture,
        base: &str,
    ) -> ConformanceResult<WebSocket> {
        let mut socket = stopped
            .http
            .get(format!("{base}/v1/session"))
            .bearer_auth(fixture::TARGET_CREDENTIAL)
            .header(
                "Cf-Access-Jwt-Assertion",
                machine_assertion(fixture::TARGET_CREDENTIAL)?,
            )
            .upgrade()
            .send()
            .await?
            .into_websocket()
            .await?;
        let session_id = SessionId::new(Uuid::now_v7())?;
        socket
            .send(Message::Binary(
                serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Hello {
                    session_id,
                    schema_support: SchemaSupport { max_version: 10 },
                })?
                .into(),
            ))
            .await?;
        let frame = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await?
            .ok_or("target closed before missing-Copy Welcome")??;
        let bytes = match frame {
            Message::Binary(bytes) => bytes.to_vec(),
            Message::Text(text) => text.into_bytes(),
            _ => return Err("target missing-Copy Welcome was not data".into()),
        };
        if !matches!(serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&bytes)?,
                ServerFrame::Welcome {session_id:actual,databases} if actual==session_id && databases.is_empty())
        {
            return Err("target obtained a lease before missing-Copy recovery granted it".into());
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(socket)
    }
    async fn reconnect_target(
        &mut self,
        stopped: &fixture::StoppedFixture,
    ) -> ConformanceResult<()> {
        let replacement = Self::connect_target(stopped, self.server.base_url()).await?;
        std::mem::replace(&mut self.target, replacement)
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await?;
        Ok(())
    }
    async fn copy_answer(&mut self, database: DatabaseId) -> ConformanceResult<ReplicationAnswer> {
        match creation::operation_answer(
            &mut self.target,
            OperationRequest::Replication(ReplicationRequest::CopyStatus { database }),
        )
        .await?
        {
            OperationAnswer::Replication(answer) => Ok(answer),
            _ => Err("missing-Copy read did not return the public typed status".into()),
        }
    }
    async fn takeover(
        &self,
        stopped: &fixture::StoppedFixture,
        request: &OwnerTakeover,
    ) -> ConformanceResult<(reqwest::StatusCode, OwnerTransferAnswer)> {
        let response = stopped
            .http
            .post(format!(
                "{}/admin/v1/projects/{}/takeover",
                self.server.base_url(),
                stopped.database
            ))
            .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
            .json(request)
            .send()
            .await?;
        let status = response.status();
        let answer = response.json().await?;
        Ok((status, answer))
    }
    async fn close(self) -> ConformanceResult<()> {
        let socket = self
            .target
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await;
        let server = self.server.shutdown().await;
        socket?;
        server?;
        Ok(())
    }
}

#[tokio::test]
async fn missing_copy_after_preview_is_unavailable_without_grant_then_same_takeover_recovers()
-> ConformanceResult<()> {
    let mut fixture = fixture::Fixture::open().await?;
    fixture.unchanged().await?;
    let copy_before = fixture.status().await?;
    let request: OwnerTakeover = serde_json::from_value(
        fixture.takeover(fixture.preview().await?, fixture::TARGET_SERVICE),
    )?;
    let stopped = fixture.stop_for_copy_loss().await?;
    let native = HolderChangeRequest::Takeover {
        attempt: request.transfer_id,
        database: stopped.database,
        preview: request.preview.clone(),
        to: PeerId::try_from(fixture::TARGET_PEER.to_owned())?,
        reason: request.reason.clone(),
    };
    let before = read_registry(stopped.root.path(), &native).await?;
    if !matches!(&before.lease,Some(lease) if lease.epoch==Epoch::new(1)? && lease.holder==PeerId::try_from(fixture::OLD_PEER.to_owned())?)
        || before.outcome.is_some()
    {
        return Err("missing-Copy fixture lacks its real original lease/no transfer".into());
    }
    grant_readback::original_only(stopped.root.path(), stopped.database).await?;
    let backup = CopyBackup::remove_from(stopped.root.path(), stopped.database)?;
    let mut missing = RunningTargetSession::open(&stopped).await?;
    let proof=async {
        if !matches!(missing.copy_answer(stopped.database).await?,ReplicationAnswer::CopyUnavailable {reason:CopyUnavailableReason::Uninitialized}) {return Err("removed Copy was still initialized before Takeover".into());}
        for _ in 0..2 {
            tokio::time::timeout(Duration::from_secs(8),async {
                loop {
                    match missing.takeover(&stopped,&request).await? {
                        (reqwest::StatusCode::OK,OwnerTransferAnswer::Pending {transfer_id}) if transfer_id==request.transfer_id=>{},
                        (reqwest::StatusCode::SERVICE_UNAVAILABLE,OwnerTransferAnswer::Unavailable {
                            subject:OwnerBlockedSubject::Transfer {transfer},reason:CopyUnavailableReason::Uninitialized
                        }) if transfer==request.transfer_id=>return Ok::<_,Box<dyn std::error::Error+Send+Sync>>(()),
                        other=>return Err(format!("missing Copy lost typed HTTP503 Unavailable/Uninitialized: {other:?}").into()),
                    }
                }
            }).await??;
        }
        if !matches!(missing.copy_answer(stopped.database).await?,ReplicationAnswer::CopyUnavailable {reason:CopyUnavailableReason::Uninitialized}) {return Err("failed Takeover initialized a missing Copy".into());}
        missing.reconnect_target(&stopped).await?;
        println!("TRANSFER_MISSING_COPY_STAGE absent_CopyUnavailable_Uninitialized_typed_Unavailable_same_attempt_repeat_target_reconnect_empty_Welcome passed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let closed = missing.close().await;
    proof?;
    closed?;
    if storage_fault::read_absent_copy(stopped.root.path(), stopped.database).await?
        != CopyStatusAnswer::NotInitialized
    {
        return Err("stopped public receiver read did not retain underlying NotInitialized".into());
    }
    let after = read_registry(stopped.root.path(), &native).await?;
    if after.lease != before.lease
        || after.grants != before.grants
        || after.outcome
            != Some(HolderChangeAnswer::Pending {
                transfer: request.transfer_id,
            })
    {
        return Err(format!(
            "missing Copy changed lease/grants or failed to retain retryable Pending: {after:?}"
        )
        .into());
    }
    grant_readback::original_only(stopped.root.path(), stopped.database).await?;
    println!(
        "TRANSFER_MISSING_COPY_STAGE stopped_public_lease_epoch_holder_unchanged_underlying_NotInitialized_native_Pending passed"
    );
    backup.restore_to(stopped.root.path())?;
    let mut recovered = RunningTargetSession::open(&stopped).await?;
    let proof=async {
        if recovered.copy_answer(stopped.database).await?!=ReplicationAnswer::CopyStatus(CopyStatusAnswer::Status(copy_before)) {return Err("restored Copy changed its original public fence/holder/schema/prefix before retry".into());}
        // The same operation has a real retry deadline. Advance the existing
        // fixture clock, rather than sleeping or replacing its retry state.
        creation::pin_registry_clock(recovered.server.base_url(),RegistryTime("2030-03-17T17:46:45Z".parse()?)).await?;
        tokio::time::timeout(Duration::from_secs(8),async {
            loop {
                match recovered.takeover(&stopped,&request).await? {
                    (reqwest::StatusCode::OK,OwnerTransferAnswer::Granted {new_epoch,start})
                        if new_epoch==Epoch::new(2)? && start==fixture::position(1,1)?=>return Ok::<_,Box<dyn std::error::Error+Send+Sync>>(()),
                    (reqwest::StatusCode::OK,OwnerTransferAnswer::Pending {transfer_id}) if transfer_id==request.transfer_id=>{},
                    other=>return Err(format!("same Takeover failed to recover after actual Copy restore: {other:?}").into()),
                }
            }
        }).await??;
        if !matches!(recovered.copy_answer(stopped.database).await?,ReplicationAnswer::CopyStatus(CopyStatusAnswer::Status(status))
            if status.fence==Epoch::new(2)? && status.holder==Some(PeerId::try_from(fixture::TARGET_PEER.to_owned())?) && status.acked==fixture::position(1,1)?) {
            return Err("recovered Takeover did not commit its actual Copy fence2/target/prefix".into());
        }
        println!("TRANSFER_MISSING_COPY_CASE original_fence_unchanged_after_restore_same_Takeover_Granted2_start1_1 passed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let closed = recovered.close().await;
    let keys = stopped.keys.shutdown().await;
    proof?;
    closed?;
    keys?;
    let finished = read_registry(stopped.root.path(), &native).await?;
    if !matches!(&finished.lease,Some(lease) if lease.epoch==Epoch::new(2)? && lease.holder==PeerId::try_from(fixture::TARGET_PEER.to_owned())?)
        || !matches!(&finished.outcome,Some(HolderChangeAnswer::Granted {epoch,start,..}) if *epoch==Epoch::new(2)? && *start==fixture::position(1,1)?)
        || !matches!(finished.grants.get(1).and_then(Option::as_ref),Some(grant) if grant.epoch==Epoch::new(2)? && grant.holder==PeerId::try_from(fixture::TARGET_PEER.to_owned())? && grant.ended_at.is_none())
        || finished.grants.iter().skip(2).any(Option::is_some)
    {
        return Err(format!(
            "recovered public grant lacks exact durable lease/epoch2 history: {finished:?}"
        )
        .into());
    }
    println!(
        "TRANSFER_MISSING_COPY_STAGE recovered_stopped_public_lease_history_actual_target_grant2_no3_6 passed"
    );
    Ok(())
}
