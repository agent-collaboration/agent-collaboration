pub(crate) use crate::conformance_object_control::{
    SignalSlots, before_dispatch, consume, edge_clock, object_control, remember_outgoing, signal,
};
use crate::{
    conformance_object_control::{TestRequest, test, test_address},
    object_rpc::{self, RpcIdentity},
};
use collaboration_central_conformance::{AccessVerificationOutcome, ControlRequest, ControlResult};
use collaboration_central_core::*;
use worker::*;
pub(crate) async fn edge_control(
    mut request: Request,
    env: Env,
    labels: crate::access_verification::InvocationLabels,
) -> Result<Response> {
    let command: ControlRequest = serde_json::from_str(&request.text().await?)?;
    let result = match command {
        ControlRequest::ProbeForbiddenEdge => {
            // This adversarial envelope is sent by the edge to Registry. It
            // claims Copy as its source to exercise the receiver's wire guard;
            // no Copy object issues an upward RPC.
            let address = ObjectAddress::Registry;
            let claimed = ObjectAddress::Copy(
                collaboration_replication_protocol::ProjectId::new(
                    uuid::Uuid::now_v7().to_string(),
                )
                .map_err(|error| Error::RustError(error.to_string()))?,
            );
            let body = crate::object_rpc::ObjectRequest {
                address: address.clone(),
                caller: RpcIdentity::Conformance,
                caller_kind: CallerKind::Copy,
                test_clock: None,
                now: RegistryTime(chrono::Utc::now()),
                input: WireInput::Call {
                    from: claimed,
                    call_id: ObjectCallId::new(uuid::Uuid::now_v7().to_string())
                        .map_err(|error| Error::RustError(error.to_string()))?,
                    call: Box::new(WireCall::Registry(RegistryCall::AuthenticateMachine {
                        credential_hash: "edge-probe-unknown".into(),
                        access_client_id: AccessClientId::new("edge-probe-unknown")
                            .map_err(|error| Error::RustError(error.to_string()))?,
                    })),
                },
            };
            let mut response = object_rpc::request(&env, &address, "/invoke", &body).await?;
            let outcome = match response.status_code() {
                403 => match serde_json::from_str::<crate::object_auth::RpcRefusal>(
                    &response.text().await?,
                )? {
                    crate::object_auth::RpcRefusal::CallEdgeRefused(refusal) => {
                        collaboration_central_conformance::CallEdgeProbe::Refused(refusal)
                    }
                    _ => {
                        return Err(Error::RustError(
                            "forbidden edge reached caller-class refusal".into(),
                        ));
                    }
                },
                200 => collaboration_central_conformance::CallEdgeProbe::Accepted,
                status => return Err(Error::RustError(format!("edge probe status {status}"))),
            };
            ControlResult::CallEdgeProbe(outcome)
        }

        ControlRequest::HoldNextFence => {
            test::<RegistryKind>(&env, (), TestRequest::Arm("hold_fence".into())).await?;
            ControlResult::Complete
        }
        ControlRequest::AwaitHeldFence => {
            test::<RegistryKind>(&env, (), TestRequest::Wait("fence_held".into())).await?;
            ControlResult::Complete
        }
        ControlRequest::ResumeFence => {
            test::<RegistryKind>(&env, (), TestRequest::Signal("resume_fence".into())).await?;
            ControlResult::Complete
        }
        ControlRequest::InterruptAfterFence => {
            test::<RegistryKind>(&env, (), TestRequest::Arm("stop_after_fence".into())).await?;
            ControlResult::Complete
        }
        ControlRequest::DropNextAnswer { installation } => {
            test::<SessionKind>(&env, installation, TestRequest::Arm("drop_answer".into())).await?;
            ControlResult::Complete
        }
        ControlRequest::AwaitDroppedAnswer { installation } => {
            test::<SessionKind>(
                &env,
                installation,
                TestRequest::Wait("answer_dropped".into()),
            )
            .await?;
            ControlResult::Complete
        }
        ControlRequest::HoldNextDispatch { installation } => {
            test::<SessionKind>(&env, installation, TestRequest::Arm("hold_dispatch".into()))
                .await?;
            ControlResult::Complete
        }
        ControlRequest::ResumeDispatch { installation } => {
            test::<SessionKind>(
                &env,
                installation,
                TestRequest::Signal("resume_dispatch".into()),
            )
            .await?;
            ControlResult::Complete
        }
        ControlRequest::HoldNextForwardDelivery { installation } => {
            test::<SessionKind>(
                &env,
                installation,
                TestRequest::Arm("hold_forward_delivery".into()),
            )
            .await?;
            ControlResult::Complete
        }
        ControlRequest::AwaitHeldForwardDelivery { installation } => {
            test::<SessionKind>(
                &env,
                installation,
                TestRequest::Wait("forward_delivery_held".into()),
            )
            .await?;
            ControlResult::Complete
        }
        ControlRequest::ResumeForwardDelivery { installation } => {
            test::<SessionKind>(
                &env,
                installation,
                TestRequest::Signal("resume_forward_delivery".into()),
            )
            .await?;
            ControlResult::Complete
        }

        ControlRequest::AwaitHeldDispatch { installation } => {
            test::<SessionKind>(
                &env,
                installation.clone(),
                TestRequest::Wait("dispatch_held".into()),
            )
            .await?;
            let held: Option<WireCall> = object_rpc::decode(
                test::<SessionKind>(&env, installation, TestRequest::TakeHeldDispatch).await?,
            )
            .await?;
            ControlResult::HeldDispatch(Box::new(
                held.ok_or_else(|| Error::RustError("dispatch held without its call".into()))?,
            ))
        }
        ControlRequest::DropNextDirectoryUpdate => {
            test::<FleetKind>(&env, (), TestRequest::Arm("drop_directory".into())).await?;
            ControlResult::Complete
        }
        ControlRequest::DirectoryUpdates => ControlResult::DirectoryUpdates(
            object_rpc::decode(test::<FleetKind>(&env, (), TestRequest::DirectoryUpdates).await?)
                .await?,
        ),
        ControlRequest::Call { address, call } => ControlResult::Call(Box::new(
            crate::worker_edge::invoke(&env, address, RpcIdentity::Conformance, *call).await?,
        )),
        ControlRequest::Step {
            address,
            input,
            now,
        } => return test_address(&env, &address, TestRequest::Step { input: *input, now }).await,
        ControlRequest::Evict { address } => {
            test_address(&env, &address, TestRequest::Evict).await?;
            ControlResult::Complete
        }
        ControlRequest::SetTime { now } => {
            let known: Vec<ObjectAddress> =
                object_rpc::decode(test::<RegistryKind>(&env, (), TestRequest::Known).await?)
                    .await?;
            test::<RegistryKind>(&env, (), TestRequest::Clock(now)).await?;
            for address in known {
                if address != ObjectAddress::Registry {
                    test_address(&env, &address, TestRequest::Clock(now)).await?;
                }
            }
            ControlResult::Complete
        }
        ControlRequest::CopyFull {
            address,
            after_writes,
        } => {
            test_address(&env, &address, TestRequest::Full(after_writes)).await?;
            ControlResult::Complete
        }
        ControlRequest::PanicAfterWrites {
            address,
            after_writes,
        } => {
            object_rpc::decode::<()>(
                test_address(&env, &address, TestRequest::Panic(after_writes)).await?,
            )
            .await?;
            ControlResult::Complete
        }
        ControlRequest::Corrupt { address, mutation } => {
            test_address(&env, &address, TestRequest::Corrupt(mutation)).await?;
            ControlResult::Complete
        }
        ControlRequest::PendingAlarms => {
            let known: Vec<ObjectAddress> =
                object_rpc::decode(test::<RegistryKind>(&env, (), TestRequest::Known).await?)
                    .await?;
            let mut alarms = Vec::new();
            for address in known {
                let found: Vec<(ObjectAddress, RegistryTime)> =
                    object_rpc::decode(test_address(&env, &address, TestRequest::Alarms).await?)
                        .await?;
                alarms.extend(found);
            }
            ControlResult::Alarms(alarms)
        }
        ControlRequest::VerifyAccessAssertion { assertion, path } => {
            ControlResult::AccessVerification(AccessVerificationOutcome::of(
                crate::access_verification::verify(
                    &env,
                    Some(&assertion),
                    &path,
                    &labels,
                    crate::access_verification::VerificationLayer::ControlInner,
                )
                .await,
            ))
        }
    };
    Response::from_json(&result)
}
