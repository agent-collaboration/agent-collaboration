//! Shared observer client and exact state challenges on the actual native/Worker owners.
#![deny(clippy::too_many_lines)]
use super::{
    forwarded_post_lifecycle_oracles as wire,
    forwarded_post_route_overlap::late_route,
    forwarded_post_scenarios::control,
    forwarded_post_state_oracles::{live_records, no_retained_state, pending_record},
};
use collaboration_central_conformance::{ConformanceResult, machine_assertion, owner_assertion};
use collaboration_central_core::{FORWARD_STATE_PATH, ForwardStateRequest};
use fleet_protocol::{DatabaseId, FrameId};
use reqwest_websocket::WebSocket;
use serde_json::Value;
use std::time::Duration;

const SENDER: &str = "550e8400-e29b-41d4-a716-446655440476";
const HOLDER: &str = "550e8400-e29b-41d4-a716-446655440477";
async fn observe(base: &str, holder: bool) -> ConformanceResult<Value> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(8))
        .build()?
        .post(format!("{base}{FORWARD_STATE_PATH}"))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(&ForwardStateRequest {
            service: (if holder { HOLDER } else { SENDER })
                .to_owned()
                .try_into()?,
        })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

pub(super) async fn overlap(
    base: &str,
    sender: &mut WebSocket,
    holder: &mut WebSocket,
    database: DatabaseId,
) -> ConformanceResult<()> {
    late_route(
        sender,
        holder,
        database,
        |command| control(base, command),
        |holder| observe(base, holder),
    )
    .await
}

pub(super) async fn own_state(
    base: &str,
    sender: &mut WebSocket,
    holder: &mut WebSocket,
    database: DatabaseId,
) -> ConformanceResult<()> {
    refuse_unprivileged_read(base).await?;
    let (request, routing) = routing_request(base, sender, database).await?;
    let delivered = wire::holder_frame(holder).await?;
    let before_sender = observe(base, false).await?;
    let before_holder = observe(base, true).await?;
    live_records(&before_sender, &before_holder, database, request, delivered)?;
    if routing.get("id") != Some(&serde_json::json!(delivered)) {
        return Err("routing and holder delivery split one request into different rows".into());
    }
    // The requester is admitted, but is not the recorded holder. A read on the
    // same ordered socket establishes that its forged answer was processed.
    wire::answer(sender, delivered).await?;
    read_barrier(sender).await?;
    if observe(base, false).await?.get("requests") != before_sender.get("requests") {
        return Err("wrong-holder answer changed the one live request row".into());
    }
    println!("FORWARD_OWNING_LIVE project={database} request={request} delivered={delivered}");
    let reply = wire::answer(holder, delivered).await?;
    if wire::outcome(sender, request).await?
        != (board_sync_protocol::ForwardOutcome::Answered { reply })
    {
        return Err(
            "state challenge did not complete its actual correlated forwarding reply".into(),
        );
    }
    no_retained_state(&observe(base, false).await?, &observe(base, true).await?)?;
    // A duplicate cannot produce another requester answer or resurrect storage.
    wire::answer(holder, delivered).await?;
    read_barrier(holder).await?;
    read_barrier(sender).await?;
    no_retained_state(&observe(base, false).await?, &observe(base, true).await?)
}

async fn routing_request(
    base: &str,
    sender: &mut WebSocket,
    database: DatabaseId,
) -> ConformanceResult<(FrameId, Value)> {
    use collaboration_central_conformance::{
        DispatchCallSelector, DispatchFaultObservation, DispatchKey, RuntimeControl,
    };
    use collaboration_central_core::ObjectAddress;
    let key = DispatchKey::new(ObjectAddress::Fleet, DispatchCallSelector::ForwardRoute);
    control(base, RuntimeControl::HoldNextDispatch { key: key.clone() }).await?;
    let proof: ConformanceResult<(FrameId, Value)> = async {
        let request = wire::post(sender, database).await?;
        let observed =
            control(base, RuntimeControl::AwaitHeldDispatch { key: key.clone() }).await?;
        if !matches!(
            observed.dispatch(&key),
            DispatchFaultObservation::Held {
                destination: ObjectAddress::Fleet,
                ..
            }
        ) {
            return Err("request did not reach its original route barrier".into());
        }
        let row = pending_record(
            &observe(base, false).await?,
            database,
            request,
            serde_json::json!({"kind":"routing"}),
        )?;
        if row.get("holderMachineId") != Some(&Value::Null) {
            return Err("routing request already recorded a holder".into());
        }
        Ok((request, row))
    }
    .await;
    let released = control(base, RuntimeControl::ResumeDispatch { key }).await;
    let value = proof?;
    released?;
    Ok(value)
}

async fn read_barrier(socket: &mut WebSocket) -> ConformanceResult<()> {
    use board_sync_protocol::{
        BoardConsumerAnswer, BoardConsumerRequest, BoardReadPage, BoardReadScope,
    };
    use fleet_protocol::{OperationAnswer, OperationRequest};
    let answer = super::creation_scenarios::operation_answer(
        socket,
        OperationRequest::Consumer(BoardConsumerRequest::Read {
            scope: BoardReadScope::Notices,
            cursor: None,
        }),
    )
    .await?;
    if !matches!(
        answer,
        OperationAnswer::Consumer(BoardConsumerAnswer::Read(BoardReadPage::Notices(_)))
    ) {
        return Err("forward answer barrier returned another request's answer".into());
    }
    Ok(())
}

async fn refuse_unprivileged_read(base: &str) -> ConformanceResult<()> {
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(8))
        .build()?;
    for assertion in [None, Some(machine_assertion("forward sender")?)] {
        let mut request =
            http.post(format!("{base}{FORWARD_STATE_PATH}"))
                .json(&ForwardStateRequest {
                    service: SENDER.to_owned().try_into()?,
                });
        if let Some(assertion) = assertion {
            request = request.header("Cf-Access-Jwt-Assertion", assertion);
        }
        if request.send().await?.status() != reqwest::StatusCode::UNAUTHORIZED {
            return Err("raw owning state was exposed without the owner Access authority".into());
        }
    }
    Ok(())
}
