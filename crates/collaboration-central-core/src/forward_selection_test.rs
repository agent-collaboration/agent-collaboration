//! The real delivery callback selects no new correlation for a revoked holder.
use super::{
    Arc, AtomicStore, AtomicUsize, ConnectionId, Executor, FixtureIo, FleetStorageFixture, NOW,
    PEER, SERVICE, SelectionHost, SessionId, TestError, TransactionScope,
};
use crate::{
    CallerKind, DeliveryKind, ObjectAddress, ObjectRuntime, caller_context::CallAuthority,
    object_call_envelope::ObjectCallEnvelope, object_runtime::ObjectWork,
};
use durable_steps::{
    BoxFuture, CallTarget, OperationId, OperationStatus, RequestKey, StartRefusal, StoreError,
};
use fleet_session::SessionKey;
use std::marker::PhantomData;
struct UnusedEngine;
impl crate::fleet_task_engine::FleetTaskEngine for UnusedEngine {
    fn start<'a>(
        &'a self,
        _: &'a RequestKey,
        _: crate::fleet_task_engine::FleetTaskInput,
    ) -> BoxFuture<'a, Result<OperationId, StartRefusal>> {
        Box::pin(async {
            Err(StoreError::backend("forward selection must not start a task").into())
        })
    }
    fn drive_due(&self) -> BoxFuture<'_, Result<durable_steps::DriveReport, StoreError>> {
        Box::pin(async { Err(StoreError::backend("unexpected engine drive")) })
    }
    fn status<'a>(
        &'a self,
        _: &'a OperationId,
    ) -> BoxFuture<'a, Result<OperationStatus, StoreError>> {
        Box::pin(async { Err(StoreError::backend("unexpected engine read")) })
    }
}
async fn run(revoked: bool, replay: bool) -> Result<(), TestError> {
    use crate::forward_calls::{DeliverForward, ForwardDeliveryRequest};
    let fixture = FleetStorageFixture::open().await?;
    let peer = PEER.to_owned().try_into()?;
    fixture.enroll(peer, SERVICE).await?;
    let session = SessionId::new(durable_steps::Uuid::now_v7())?;
    let host = Arc::new(SelectionHost {
        store: fixture.object.clone(),
        connection: ConnectionId::new(durable_steps::Uuid::now_v7())?,
        session,
        peer,
        sends: AtomicUsize::new(0),
        io: Arc::new(FixtureIo),
    });
    let shared_host = Arc::new(crate::CoreHost::for_store(
        FixtureIo,
        fleet_protocol::ReceiverId::new(durable_steps::Uuid::now_v7())?,
    ));
    let config = fleet_project_lease::RegistryConfig {
        lease: fleet_project_lease::LeasePolicy {
            lease_duration: std::time::Duration::from_secs(3600),
            renew_interval: std::time::Duration::from_secs(900),
        },
        authoritative_receiver: fleet_protocol::ReceiverId::new(durable_steps::Uuid::now_v7())?,
        release_unsent_for: std::time::Duration::ZERO,
        move_needs_renewal_within: None,
    };
    let runtime = ObjectRuntime::<DeliveryKind, _> {
        work: ObjectWork::Fleet {
            honored_capabilities: fleet_protocol::FrameCapabilities::default(),
            host: host.clone(),
            io: Arc::new(FixtureIo),
            engine: Arc::new(UnusedEngine),
            shared_host,
            replies: Arc::new(crate::holder_reply_hub::HolderReplyHub::default()),
            config,
            delivery: crate::DeliveryConfig { relay_peer: peer },
            holder_answer_timeout: std::time::Duration::from_secs(2),
        },
        face: PhantomData,
    };
    let request = ForwardDeliveryRequest {
        forward_frame: fleet_protocol::FrameId::new(durable_steps::Uuid::now_v7())?,
        holder: peer,
        deadline: NOW.after(std::time::Duration::from_secs(20)),
        origin_session: SessionKey {
            peer,
            session_id: session,
        },
        request_frame: fleet_protocol::FrameId::new(durable_steps::Uuid::now_v7())?,
        connection: host.connection,
        created_at: NOW,
        post: serde_json::from_value(
            serde_json::json!({"project":"019f0000-0000-7000-8000-00000000a205","origin":{"installation":SERVICE,"machine":"selection fixture"},"post":{"kind":"threadPost","body":{}}}),
        )?,
    };
    let invoke = || -> Result<ObjectCallEnvelope, TestError> {
        Ok(ObjectCallEnvelope {
            source: ObjectAddress::Fleet,
            source_face: CallerKind::Session,
            authority: CallAuthority::Machine {
                installation: SERVICE.to_owned().try_into()?,
            },
            destination: ObjectAddress::Fleet,
            destination_face: CallerKind::Delivery,
            identity: durable_steps::CallId {
                operation: OperationId::new(durable_steps::Uuid::now_v7())?,
                step: durable_steps::StepName::new("forward")?,
            },
            kind: DeliverForward::KIND.into(),
            target: serde_json::to_value(DeliverForward {
                installation: SERVICE.to_owned().try_into()?,
            })?,
            request: serde_json::to_value(&request)?,
        })
    };
    let selected_request = crate::project_request_store::ProjectRequest {
        forward_frame: request.forward_frame,
        request_frame: request.request_frame,
        connection: request.connection,
        session: request.origin_session,
        project: request.post.project.clone(),
        phase: crate::project_request_store::ProjectRequestPhase::AwaitingHolder { holder: peer },
        deadline: request.deadline,
        created_at: NOW,
        updated_at: NOW,
    };
    fixture
        .object
        .transaction(move |scope| {
            Box::pin(
                async move { crate::project_request_store::save(scope, &selected_request).await },
            )
        })
        .await?;
    if replay {
        runtime.receive_forward_call(invoke()?).await?;
    }
    if revoked {
        fixture
            .object
            .transaction(|scope| {
                Box::pin(async move {
                    scope
                        .executor()
                        .execute("UPDATE machine_enrollments SET state='revoked'")
                        .await
                        .map_err(StoreError::backend)?;
                    Ok(())
                })
            })
            .await?;
    }
    let answer: serde_json::Value =
        serde_json::from_slice(&runtime.receive_forward_call(invoke()?).await?)?;
    let expected = if revoked && !replay {
        "noConnection"
    } else {
        "delivered"
    };
    if answer != serde_json::json!(expected) {
        return Err(format!("wrong forward target selection result: {answer}").into());
    }
    let selected = fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                Ok(
                    crate::project_request_store::by_frame(scope, request.forward_frame)
                        .await?
                        .is_some_and(|row| {
                            matches!(
                                row.phase,
                                crate::project_request_store::ProjectRequestPhase::AtHolder { .. }
                            )
                        }),
                )
            })
        })
        .await?;
    if selected != (expected == "delivered") {
        return Err("forward correlation selection disagreed with admission".into());
    }
    fixture.close().await
}
#[tokio::test]
async fn revoked_holder_gets_no_new_forward_correlation() -> Result<(), TestError> {
    run(true, false).await
}
#[tokio::test]
async fn enrolled_holder_selects_a_forward_correlation() -> Result<(), TestError> {
    run(false, false).await
}
#[tokio::test]
async fn an_exact_forward_selected_before_revoke_can_replay() -> Result<(), TestError> {
    run(true, true).await
}
