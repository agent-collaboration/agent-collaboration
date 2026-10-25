//! Force a real schema refusal only after Active has blocked the accepted archive.
use super::*;
use collaboration_central_conformance::enroll_open_peer_with_label;
use fleet_sync_protocol::{CopyStatusAnswer, ReplicationAnswer, ReplicationRequest, SettleOutcome};
use futures_util::SinkExt;

pub(super) async fn prepare_cleanup(
    base: &str,
    database: DatabaseId,
    loss: CleanupLoss,
) -> ConformanceResult<PreparedCleanup> {
    let at = RegistryTime(
        chrono::DateTime::from_timestamp_millis(chrono::Utc::now().timestamp_millis())
            .ok_or("cleanup fixture time is outside range")?,
    );
    for address in [ObjectAddress::Registry, ObjectAddress::Fleet] {
        control(base, address, RuntimeControl::SetTime { now: at }).await?;
    }
    let client = ConformanceClient::new(base)?;
    let mut claimant = enroll_open_peer_with_label(
        &client,
        SERVICE,
        PEER,
        CREDENTIAL,
        CREDENTIAL,
        &Uuid::now_v7().to_string(),
    )
    .await?;
    let session = Uuid::now_v7();
    claimant
        .socket
        .send(reqwest_websocket::Message::Text(
            json!({"kind":"hello","value":{"sessionId":session,"schemaSupport":{"maxVersion":0}}})
                .to_string(),
        ))
        .await?;
    let welcome = tokio::time::timeout(Duration::from_secs(5), claimant.socket.next())
        .await?
        .ok_or("schema0 claimant closed before Welcome")??;
    let bytes = match welcome {
        reqwest_websocket::Message::Text(text) => text.into_bytes(),
        reqwest_websocket::Message::Binary(bytes) => bytes.to_vec(),
        _ => return Err("schema0 claimant did not receive Welcome".into()),
    };
    let welcome: Value = serde_json::from_slice(&bytes)?;
    if welcome.get("kind") != Some(&json!("welcome"))
        || welcome.pointer("/value/sessionId") != Some(&json!(session))
    {
        return Err("schema0 claimant Welcome lost its real admission identity".into());
    }
    let mut reader = enroll_admitted_peer(
        &client,
        READER,
        READER_PEER,
        READER_CREDENTIAL,
        &Uuid::now_v7().to_string(),
    )
    .await?;
    let status = creation_scenarios::operation_answer(
        &mut reader.socket,
        OperationRequest::Replication(ReplicationRequest::CopyStatus { database }),
    )
    .await?;
    let OperationAnswer::Replication(ReplicationAnswer::CopyStatus(CopyStatusAnswer::Status(
        status,
    ))) = status
    else {
        return Err("schema refusal setup has no real imported Copy status".into());
    };
    if status.schema.version != 1 {
        return Err("schema refusal fixture did not retain its canonical schema1 seed".into());
    }
    let refusal = HolderChangeRefusal::SchemaTooNew {
        store: status.schema.clone(),
        supported: Some(0),
    };
    let attempt = TransferId::new(Uuid::now_v7())?;
    let request = fleet_sync_protocol::RequestId::new(attempt.as_uuid())?;
    let copy = ObjectAddress::Copy(database);
    control(
        base,
        ObjectAddress::Registry,
        RuntimeControl::HoldNextDispatch {
            destination: copy.clone(),
        },
    )
    .await?;
    creation_scenarios::send(
        &mut claimant.socket,
        OperationRequest::Lease(fleet_sync_protocol::LeaseRequest::Claim {
            attempt: request,
            database,
        }),
    )
    .await?;
    // The library's first Copy call reads status after the actual reservation.
    let status_call = await_call(base, &copy).await?;
    if status_call.get("kind")
        != Some(&json!(
            <fleet_sync_lease::CopyRead as durable_steps::CallTarget>::KIND
        ))
    {
        return Err(
            format!("Claim did not hold its actual Copy status path: {status_call}").into(),
        );
    }
    if lease_publication::reservation(base, database, attempt).await?
        != Some(ClaimReservationState::Active)
    {
        return Err("normal Claim did not create its real Active reservation".into());
    }
    control(
        base,
        ObjectAddress::Registry,
        RuntimeControl::ResumeDispatchAndHoldNext {
            destination: copy.clone(),
        },
    )
    .await?;
    let fence = await_call(base, &copy).await?;
    if fence.get("kind")
        != Some(&json!(
            <fleet_sync_lease::CopyFence as durable_steps::CallTarget>::KIND
        ))
    {
        return Err(format!("Claim did not hold its actual schema-refusing fence: {fence}").into());
    }
    let workspace = WorkspaceId::try_from("019f0000-0000-7000-8000-000000000a81".to_owned())?;
    let archive = FleetRequest {
        request_id: message_board::RequestId::generate(),
        workspace,
        actor: RecordedActor::Human {
            human_id: HumanId::try_from("archive recovery owner".to_owned())?,
        },
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation: FleetOperation::WorkspaceArchive {},
    };
    let decision = decide(&mut reader.socket, &archive).await?;
    if !matches!(
        &decision,
        FleetRequestResult::Decided(RequestDecision::Accepted {
            change: ChangeOutcome::Changed,
            ..
        })
    ) {
        return Err("Active reservation fixture refused its normal workspace archive".into());
    }
    let before = read_fleet(&mut reader.socket).await?;
    let _ = finalized(&before, at)?; // Every target must still be Archiving here.
    match loss {
        CleanupLoss::Dispatch => {
            control(
                base,
                ObjectAddress::Registry,
                RuntimeControl::ResumeDispatchAndHoldNext {
                    destination: ObjectAddress::Fleet,
                },
            )
            .await?;
        }
        CleanupLoss::Acknowledgement => {
            control(
                base,
                ObjectAddress::Registry,
                RuntimeControl::DropNextObjectReply {
                    destination: ObjectAddress::Fleet,
                },
            )
            .await?;
            control(
                base,
                ObjectAddress::Registry,
                RuntimeControl::ResumeDispatch,
            )
            .await?;
        }
    }
    let call = match loss {
        CleanupLoss::Dispatch => await_call(base, &ObjectAddress::Fleet).await?,
        CleanupLoss::Acknowledgement => {
            let observed = control(
                base,
                ObjectAddress::Registry,
                RuntimeControl::AwaitDroppedObjectReply,
            )
            .await?;
            let ObjectReplyFaultObservation::Dropped {
                destination,
                request,
            } = observed.object_reply
            else {
                return Err("actual refused-settlement reply was not dropped".into());
            };
            if destination != ObjectAddress::Fleet {
                return Err("dropped another object's reply".into());
            }
            serde_json::from_slice(&request)?
        }
    };
    if call.get("kind")
        != Some(&json!(
            <fleet_sync_lease::ClaimSettlement as durable_steps::CallTarget>::KIND
        ))
    {
        return Err(
            format!("cleanup missed its exact refused-settlement hold or reply: {call}").into(),
        );
    }
    let original: ClaimSettlement = serde_json::from_value(
        call.get("request")
            .cloned()
            .ok_or("saved settlement omitted payload")?,
    )?;
    if original.change
        != (HolderChangeRequest::Claim {
            attempt,
            database,
            claimant: fleet_sync_protocol::PeerId::try_from(PEER.to_owned())?,
        })
        || original.outcome != (SettleOutcome::Refused { reason: refusal })
    {
        return Err(
            "actual refused settlement lost its exact original attempt/schema refusal".into(),
        );
    }
    let operation = call
        .pointer("/identity/operation")
        .cloned()
        .ok_or("settlement has no original sender operation")?;
    if call.pointer("/identity/step") != Some(&json!("settle")) {
        return Err("settlement lost its original library step".into());
    }
    let settled_before = match loss {
        CleanupLoss::Dispatch => {
            if read_fleet(&mut reader.socket).await? != before
                || lease_publication::reservation(base, database, attempt).await?
                    != Some(ClaimReservationState::Active)
            {
                return Err("pre-dispatch loss already applied the refused settlement".into());
            }
            None
        }
        CleanupLoss::Acknowledgement => {
            let expected = finalized(&before, at)?;
            if read_fleet(&mut reader.socket).await? != expected {
                return Err(
                    "lost real settlement ACK did not follow exact committed archive finalization"
                        .into(),
                );
            }
            Some(expected)
        }
    };
    Ok(PreparedCleanup {
        input: CleanupInput {
            database,
            at,
            original,
            call,
            operation,
            archive,
            decision,
            before,
            settled_before,
        },
        claimant,
        reader,
    })
}

async fn await_call(base: &str, destination: &ObjectAddress) -> ConformanceResult<Value> {
    let observed = tokio::time::timeout(
        Duration::from_secs(8),
        control(
            base,
            ObjectAddress::Registry,
            RuntimeControl::AwaitHeldDispatch,
        ),
    )
    .await
    .map_err(|_| "archive cleanup missed its exact refused-settlement or Copy hold")??;
    let DispatchFaultObservation::Held {
        destination: actual,
        request,
    } = observed.dispatch
    else {
        return Err("actual cleanup dispatch did not reach its barrier".into());
    };
    if &actual != destination {
        return Err("cleanup hold intercepted another destination".into());
    }
    Ok(serde_json::from_slice(&request)?)
}
