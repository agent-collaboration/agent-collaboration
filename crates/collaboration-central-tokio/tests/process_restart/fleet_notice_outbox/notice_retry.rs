//! Observe a released sender attempt through short-lived read-only connections before advancing time.
use super::*;

pub(super) async fn resume_at_committed_retry(
    base: &str,
    root: &std::path::Path,
    held: &[u8],
) -> ConformanceResult<()> {
    use sqlx_core::{
        connection::{ConnectOptions, Connection},
        query::query,
        row::Row,
    };
    let call: Value = serde_json::from_slice(held)?;
    let mut operation = call
        .pointer("/identity/operation")
        .and_then(Value::as_str)
        .ok_or("held notice omitted its operation")?
        .to_owned();
    control(
        base,
        ObjectAddress::Fleet,
        RuntimeControl::ResumeDispatchAndHoldNext {
            key: notice_key(),
            next: notice_key(),
        },
    )
    .await?;
    // The outer readiness bound limits this causal wait. Do not move time
    // until the released attempt actually commits its ordinary retry schedule.
    let next = loop {
        let mut connection = sqlx_turso::TursoConnectOptions::new()
            .filename(root.join("fleet.db"))
            .read_only(true)
            .connect()
            .await?;
        let row=query::<sqlx_turso::Turso>("SELECT state,next_attempt_at,drive_until FROM _fleet_sync_operations WHERE operation_id=? AND kind='central.fleet-notice'")
            .bind(&operation).fetch_one(&mut connection).await?;
        let state = row.try_get::<String, _>("state")?;
        let drive = row.try_get::<Option<i64>, _>("drive_until")?;
        let deadline = row.try_get::<Option<i64>, _>("next_attempt_at")?;
        connection.close().await?;
        if state == "completed" {
            let observed = control(base, ObjectAddress::Fleet, RuntimeControl::Observe).await?;
            if let ObjectReplyFaultObservation::Dropped { request, .. } = observed.object_reply {
                let dropped: Value = serde_json::from_slice(&request)?;
                let lost_operation = dropped
                    .pointer("/identity/operation")
                    .and_then(Value::as_str)
                    .ok_or("dropped notice omitted its operation")?;
                // A sibling notice can consume the kind-wide reply fault while
                // this call is held. Its real retry, not this successful call,
                // must commit before advancing the injected clock.
                if lost_operation != operation {
                    eprintln!(
                        "NOTICE_ACK_LOSS_CASE completed_sibling={operation} following_actual_lost_call={lost_operation}"
                    );
                    operation = lost_operation.to_owned();
                    continue;
                }
            }
        }
        if state != "running" {
            return Err("notice sender completed before its intended reply loss".into());
        }
        if drive.is_none() {
            break deadline.ok_or("released notice lost its retry deadline")?;
        }
        tokio::task::yield_now().await;
    };
    let collaboration_central_conformance::ClockObservation::Pinned { now } =
        control(base, ObjectAddress::Fleet, RuntimeControl::Observe)
            .await?
            .clock
    else {
        return Err("notice recovery fixture lost its injected clock".into());
    };
    control(
        base,
        ObjectAddress::Fleet,
        RuntimeControl::SetTime {
            now: RegistryTime(
                chrono::DateTime::from_timestamp_millis(next.max(now.0.timestamp_millis()))
                    .ok_or("notice retry time outside range")?,
            ),
        },
    )
    .await?;
    Ok(())
}
