//! v3.8: `setTime` positions the clock; `settleClock` is the one portable way to
//! make due work run, on both shells. Observed through the control answers
//! (`alarmEvents`, `readAlarm`, the S8 snapshot), so the same cases run on Tokio
//! and on workerd. What S1 shows (the delivery label, no record for a dropped
//! alarm, the lost-delivery marker at Frame entry) is read from S1 by the
//! shells' own tests and the Verifier's reader.

use crate::{ConformanceClient, ConformanceResult, RuntimeControl, RuntimeControlRequest};
use crate::{RuntimeControlAnswer, RuntimeControlFailure};
use collaboration_central_core::{ObjectAddress, RegistryTime};

/// Send a seam control and read its typed answer (a 409 is its typed failure).
async fn seam(
    client: &ConformanceClient,
    address: ObjectAddress,
    command: RuntimeControl,
) -> ConformanceResult<Result<RuntimeControlAnswer, RuntimeControlFailure>> {
    let response = client
        .with_owner_access(
            client
                .client
                .post(format!("{}/admin/v1/conformance/runtime", client.base_url)),
        )?
        .json(&RuntimeControlRequest { address, command })
        .send()
        .await?;
    if response.status() == reqwest::StatusCode::CONFLICT {
        return Ok(Err(response.json().await?));
    }
    Ok(Ok(response.error_for_status()?.json().await?))
}

async fn answer(
    client: &ConformanceClient,
    address: ObjectAddress,
    command: RuntimeControl,
) -> ConformanceResult<RuntimeControlAnswer> {
    seam(client, address, command)
        .await?
        .map_err(|failure| format!("control refused: {failure:?}").into())
}

async fn stored(
    client: &ConformanceClient,
    address: ObjectAddress,
) -> ConformanceResult<Option<i64>> {
    match answer(client, address, RuntimeControl::ReadAlarm).await? {
        RuntimeControlAnswer::Alarm { stored } => Ok(stored),
        other => Err(format!("readAlarm answered {other:?}").into()),
    }
}

async fn settle(
    client: &ConformanceClient,
    address: ObjectAddress,
    at: i64,
) -> ConformanceResult<u64> {
    let now = RegistryTime(
        chrono::DateTime::from_timestamp_millis(at).ok_or("settle time out of range")?,
    );
    match answer(client, address, RuntimeControl::SettleClock { now }).await? {
        RuntimeControlAnswer::ClockSettled { alarm_events } => Ok(alarm_events),
        other => Err(format!("settleClock answered {other:?}").into()),
    }
}

/// The registration fence a snapshot reports: its sessions and socket inventory.
async fn fence(
    client: &ConformanceClient,
    address: ObjectAddress,
) -> ConformanceResult<serde_json::Value> {
    match answer(client, address, RuntimeControl::Snapshot).await? {
        RuntimeControlAnswer::Snapshot(snapshot) => Ok(serde_json::json!({
            "sessions": snapshot.sessions,
            "inventory": snapshot.inventory,
        })),
        other => Err(format!("snapshot answered {other:?}").into()),
    }
}

/// The v3.8 clock cases observable through control answers. `quiet` is an
/// object with no stored alarm (checked, not assumed); `now` is the pinned
/// business time (Unix ms).
pub(crate) async fn clock_settle_cases(
    client: &ConformanceClient,
    quiet: ObjectAddress,
    now: i64,
) -> ConformanceResult<()> {
    let fleet = ObjectAddress::Fleet;

    // No stored alarm: a jump delivers nothing and stores nothing.
    if stored(client, quiet.clone()).await?.is_some() {
        return Err("the quiet object already had a stored alarm".into());
    }
    let day = 24 * 60 * 60 * 1_000;
    if settle(client, quiet.clone(), now + day).await? != 0 {
        return Err("settleClock delivered an Alarm with no stored alarm".into());
    }
    if stored(client, quiet).await?.is_some() {
        return Err("settleClock stored an alarm on a quiet object".into());
    }

    // A move short of the Fleet's armed alarm: no Alarm, the deadline unchanged.
    // A 1 ms move, so no other work comes due while the clock moves.
    let armed = stored(client, fleet.clone())
        .await?
        .ok_or("the Fleet has no stored alarm")?;
    if armed <= now + 1 {
        return Err("the Fleet's alarm is not in the future of the pinned clock".into());
    }
    let before = fence(client, fleet.clone()).await?;
    if settle(client, fleet.clone(), now + 1).await? != 0 {
        return Err("a move short of the alarm delivered an Alarm".into());
    }
    if stored(client, fleet.clone()).await? != Some(armed) {
        return Err("a move short of the alarm changed the stored deadline".into());
    }

    // Across the deadline: exactly one Alarm, and the stored alarm moves on.
    let delivered = settle(client, fleet.clone(), armed).await?;
    if delivered != 1 {
        return Err(
            format!("settleClock across the deadline delivered {delivered} Alarms, not 1").into(),
        );
    }
    if stored(client, fleet.clone())
        .await?
        .is_some_and(|next| next <= armed)
    {
        return Err("the delivered Alarm left its deadline due".into());
    }

    // The passive snapshot after the settled event (its answer comes after the
    // event completed): the registration fence is unchanged.
    if fence(client, fleet.clone()).await? != before {
        return Err("the registration fence changed across a settled clock event".into());
    }

    // Delivery suppressed: a real Fleet write from a fresh peer arms a new alarm;
    // with delivery off, settleClock across it counts one dropped delivery and
    // consumes the alarm. Re-enabled, a second settle delivers nothing: no alarm
    // is stored until a real event writes one.
    let mut fresh = crate::enroll_admitted_peer(
        client,
        &durable_steps::Uuid::now_v7().to_string(),
        &durable_steps::Uuid::now_v7().to_string(),
        "settle-suppressed",
        &durable_steps::Uuid::now_v7().to_string(),
    )
    .await?;
    let written = crate::runtime_control_scenarios::create_workspace(&mut fresh.socket).await;
    let rearmed = match written {
        Ok(_) => stored(client, fleet.clone()).await,
        Err(failure) => Err(failure),
    };
    let suppressed = async {
        let rearmed = rearmed?.ok_or("the fresh Fleet write armed no alarm")?;
        answer(
            client,
            fleet.clone(),
            RuntimeControl::AlarmDelivery { enabled: false },
        )
        .await?;
        let dropped = settle(client, fleet.clone(), rearmed).await;
        answer(
            client,
            fleet.clone(),
            RuntimeControl::AlarmDelivery { enabled: true },
        )
        .await?;
        if dropped? != 1 {
            return Err::<(), Box<dyn std::error::Error + Send + Sync>>(
                "a suppressed due alarm was not counted once".into(),
            );
        }
        if stored(client, fleet.clone()).await?.is_some() {
            return Err("a suppressed alarm was not consumed".into());
        }
        if settle(client, fleet.clone(), rearmed).await? != 0 {
            return Err("re-enabled delivery delivered a consumed alarm".into());
        }
        Ok(())
    }
    .await;
    let closed = fresh
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    suppressed?;
    closed?;

    // A never-opened object: its snapshot answers notOpen and opens nothing, so a
    // second snapshot is notOpen too, and it has no stored alarm.
    let unopened = ObjectAddress::Copy(fleet_protocol::DatabaseId::try_from(
        durable_steps::Uuid::now_v7().to_string(),
    )?);
    for _ in 0..2 {
        match seam(client, unopened.clone(), RuntimeControl::Snapshot).await? {
            Err(RuntimeControlFailure::NotOpen) => {}
            other => {
                return Err(format!("a never-opened object's snapshot answered {other:?}").into());
            }
        }
    }
    if stored(client, unopened).await?.is_some() {
        return Err("a snapshot armed an alarm on a never-opened object".into());
    }
    Ok(())
}
