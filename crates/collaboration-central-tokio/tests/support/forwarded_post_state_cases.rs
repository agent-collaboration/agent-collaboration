//! Shared observer client and exact state challenges on the actual native/Worker owners.
#![deny(clippy::too_many_lines)]
use super::{
    forwarded_post_lifecycle_oracles as wire,
    forwarded_post_route_overlap::late_route,
    forwarded_post_scenarios::control,
    forwarded_post_state_oracles::{live_records, no_retained_state},
};
use collaboration_central_conformance::{ConformanceResult, machine_assertion, owner_assertion};
use collaboration_central_core::{FORWARD_STATE_PATH, ForwardStateRequest};
use fleet_sync_protocol::DatabaseId;
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

pub(super) async fn wake_deadline(
    base: &str,
) -> ConformanceResult<Option<collaboration_central_core::UnixMillis>> {
    let observed: collaboration_central_core::ForwardStateObservation =
        serde_json::from_value(observe(base, false).await?)?;
    Ok(observed.wake_deadline)
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
    let request = wire::post(sender, database).await?;
    let delivered = wire::holder_frame(holder).await?;
    let before_sender = observe(base, false).await?;
    let before_holder = observe(base, true).await?;
    live_records(&before_sender, &before_holder, database, request, delivered)?;
    println!("FORWARD_OWNING_LIVE project={database} request={request} delivered={delivered}");
    let reply = wire::answer(holder, delivered).await?;
    if wire::outcome(sender, request).await?
        != (board_sync_protocol::ForwardOutcome::Answered { reply })
    {
        return Err(
            "state challenge did not complete its actual correlated forwarding reply".into(),
        );
    }
    no_retained_state(&observe(base, false).await?, &observe(base, true).await?)
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
