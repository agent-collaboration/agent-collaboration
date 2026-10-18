use crate::{LocalResult, http_server::HttpState};
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use collaboration_central_conformance::{AccessVerificationOutcome, ControlRequest, ControlResult};
pub(crate) async fn control(
    State(state): State<HttpState>,
    Json(request): Json<ControlRequest>,
) -> Response {
    let result: LocalResult<ControlResult> = async {
        let runtime = &state.runtime;
        Ok(match request {
            ControlRequest::ProbeForbiddenEdge => {
                use collaboration_central_core::*;
                let result = runtime
                    .invoke_identified(
                        CallerKind::Copy,
                        ObjectAddress::Copy(collaboration_replication_protocol::ProjectId::new(
                            uuid::Uuid::now_v7().to_string(),
                        )?),
                        ObjectAddress::Registry,
                        ObjectCallId::new(uuid::Uuid::now_v7().to_string())?,
                        WireCall::Registry(RegistryCall::AuthenticateMachine {
                            credential_hash: "edge-probe-unknown".into(),
                            access_client_id: AccessClientId::new("edge-probe-unknown").map_err(
                                |_| crate::LocalError::from("invalid fixed probe client"),
                            )?,
                        }),
                    )
                    .await;
                let outcome = match result {
                    Err(crate::LocalError::CallEdge(refusal)) => {
                        collaboration_central_conformance::CallEdgeProbe::Refused(refusal)
                    }
                    Ok(_) => collaboration_central_conformance::CallEdgeProbe::Accepted,
                    Err(error) => return Err(error),
                };
                ControlResult::CallEdgeProbe(outcome)
            }

            ControlRequest::HoldNextFence => {
                runtime
                    .faults
                    .hold_next_fence
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                ControlResult::Complete
            }
            ControlRequest::AwaitHeldFence => {
                runtime.faults.fence_held.notified().await;
                ControlResult::Complete
            }
            ControlRequest::ResumeFence => {
                runtime.faults.resume_fence.notify_one();
                ControlResult::Complete
            }
            ControlRequest::DropNextDirectoryUpdate => {
                runtime
                    .faults
                    .drop_directory
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                ControlResult::Complete
            }
            ControlRequest::DirectoryUpdates => ControlResult::DirectoryUpdates(
                runtime.faults.directory_updates.lock().await.clone(),
            ),
            ControlRequest::Call { address, call } => {
                ControlResult::Call(Box::new(runtime.invoke(address, *call).await?))
            }
            ControlRequest::Step {
                address,
                input,
                now,
            } => {
                let effects = runtime
                    .actors
                    .step(address.clone(), *input, now)
                    .await?
                    .into_trace();
                // Keep RPC effects held for controlled delivery, but persist the
                // real alarm plan so a physical restart can exercise its repair.
                runtime.refresh_alarm(address).await?;
                ControlResult::Effects(effects)
            }
            ControlRequest::Evict { address } => {
                runtime.actors.evict(address).await?;
                ControlResult::Complete
            }
            ControlRequest::SetTime { now } => {
                runtime.set_time(now).await?;
                ControlResult::Complete
            }
            ControlRequest::DropNextAnswer { installation } => {
                runtime
                    .faults
                    .session(&installation)
                    .await
                    .drop_next_answer
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                ControlResult::Complete
            }
            ControlRequest::AwaitDroppedAnswer { installation } => {
                runtime
                    .faults
                    .session(&installation)
                    .await
                    .answer_dropped
                    .notified()
                    .await;
                ControlResult::Complete
            }
            ControlRequest::HoldNextDispatch { installation } => {
                runtime
                    .faults
                    .session(&installation)
                    .await
                    .hold_next_dispatch
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                ControlResult::Complete
            }
            ControlRequest::AwaitHeldDispatch { installation } => {
                runtime
                    .faults
                    .session(&installation)
                    .await
                    .dispatch_held
                    .notified()
                    .await;
                let held = runtime
                    .faults
                    .session(&installation)
                    .await
                    .held_dispatch
                    .lock()
                    .await
                    .take();
                ControlResult::HeldDispatch(Box::new(held.ok_or("dispatch held without its call")?))
            }
            ControlRequest::ResumeDispatch { installation } => {
                runtime
                    .faults
                    .session(&installation)
                    .await
                    .resume_dispatch
                    .notify_one();
                ControlResult::Complete
            }
            ControlRequest::HoldNextForwardDelivery { installation } => {
                runtime
                    .faults
                    .session(&installation)
                    .await
                    .hold_forward_delivery
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                ControlResult::Complete
            }
            ControlRequest::AwaitHeldForwardDelivery { installation } => {
                runtime
                    .faults
                    .session(&installation)
                    .await
                    .forward_delivery_held
                    .notified()
                    .await;
                ControlResult::Complete
            }
            ControlRequest::ResumeForwardDelivery { installation } => {
                runtime
                    .faults
                    .session(&installation)
                    .await
                    .resume_forward_delivery
                    .notify_one();
                ControlResult::Complete
            }
            ControlRequest::InterruptAfterFence => {
                runtime.interrupt_after_fence();
                ControlResult::Complete
            }
            ControlRequest::CopyFull {
                address,
                after_writes,
            } => {
                runtime.actors.inject_full(address, after_writes).await?;
                ControlResult::Complete
            }
            ControlRequest::Corrupt { address, mutation } => {
                runtime.actors.corrupt(address, mutation).await?;
                ControlResult::Complete
            }
            ControlRequest::PendingAlarms => ControlResult::Alarms(runtime.pending_alarms().await),
            ControlRequest::VerifyAccessAssertion { assertion, path } => {
                ControlResult::AccessVerification(AccessVerificationOutcome::of(
                    state
                        .access
                        .verify(Some(&assertion), &path, runtime.now())
                        .await,
                ))
            }
            ControlRequest::PanicAfterWrites { .. } => {
                return Err(
                    "panic observation is only supported by the explicit Worker fault build".into(),
                );
            }
        })
    }
    .await;
    match result {
        Ok(result) => Json(result).into_response(),
        Err(error) => {
            eprintln!("local conformance control: {error}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
