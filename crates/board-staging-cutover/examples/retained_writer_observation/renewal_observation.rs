//! Lead01a1c1b3 scratch-A-only renewal record read, with public RenewAnswer decoding.
use super::{ObservationError, check_sealed, seed_snapshot::check_scratch, source_hash};
use fleet_sync_protocol::{DatabaseId, Epoch, PeerId, RenewAnswer, SyncTime};
use serde::Deserialize;
use sqlx::{ConnectOptions, Connection, Row};
use sqlx_turso::{Turso, TursoConnectOptions};
use std::{ffi::OsString, path::Path};

const AUTHORITY: &str =
    "SELECT record -> '$.authority' AS authority FROM _fleet_sync_writer WHERE singleton=1";
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum RenewalKind {
    Renewal,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RenewalRecord {
    kind: RenewalKind,
    answer: RenewAnswer,
    request_started: SyncTime,
}
struct Expected {
    database: DatabaseId,
    holder: PeerId,
    epoch: Epoch,
    window_start: i64,
    window_end: i64,
}
fn failure(error: impl std::fmt::Display) -> ObservationError {
    ObservationError::Read(error.to_string())
}
fn identifier<T: serde::de::DeserializeOwned>(input: &OsString) -> Result<T, ObservationError> {
    serde_json::from_value(serde_json::Value::String(
        input
            .to_str()
            .ok_or(ObservationError::InvalidInput)?
            .to_owned(),
    ))
    .map_err(failure)
}
fn verify(raw: &str, expected: &Expected) -> Result<serde_json::Value, ObservationError> {
    let record: RenewalRecord = serde_json::from_str(raw).map_err(failure)?;
    let RenewalKind::Renewal = record.kind;
    let started = record.request_started.unix_millis();
    let RenewAnswer::Renewed {
        recorded, current, ..
    } = &record.answer
    else {
        return Err(failure("latest authority is a renewal refusal"));
    };
    if expected.window_start > expected.window_end
        || started < expected.window_start
        || started > expected.window_end
        || current.database != expected.database
        || current.holder != expected.holder
        || current.epoch != expected.epoch
        || recorded.epoch != expected.epoch
        || current.expires_at.unix_millis() <= started
    {
        return Err(failure(
            "renewal identity, epoch or observation window mismatch",
        ));
    }
    Ok(
        serde_json::json!({"database":expected.database,"holder":expected.holder,"lease":expected.epoch,"requestStarted":record.request_started,"answer":record.answer,"currentRenewalObserved":true}),
    )
}
pub(super) async fn run(arguments: &[OsString]) -> Result<(), ObservationError> {
    let [_, scratch, untouched, database, holder, epoch, start, end] = arguments else {
        return Err(ObservationError::InvalidInput);
    };
    let parse_time = |text: &OsString| {
        text.to_str()
            .ok_or(ObservationError::InvalidInput)?
            .parse::<i64>()
            .map_err(failure)
    };
    let epoch = epoch
        .to_str()
        .ok_or(ObservationError::InvalidInput)?
        .parse::<u64>()
        .map_err(failure)?;
    let expected = Expected {
        database: identifier(database)?,
        holder: identifier(holder)?,
        epoch: Epoch::new(epoch).map_err(failure)?,
        window_start: parse_time(start)?,
        window_end: parse_time(end)?,
    };
    let scratch = Path::new(scratch);
    let untouched = Path::new(untouched);
    check_sealed(untouched)?;
    check_scratch(scratch, untouched)?;
    let a_before = source_hash(scratch)?;
    let b_before = source_hash(untouched)?;
    let mut connection = TursoConnectOptions::new()
        .filename(scratch)
        .create_if_missing(false)
        .read_only(true)
        .connect()
        .await
        .map_err(failure)?;
    let result = sqlx::query::<Turso>(AUTHORITY)
        .fetch_one(&mut connection)
        .await
        .map_err(failure)?
        .try_get::<Option<String>, _>("authority")
        .map_err(failure)?;
    connection.close().await.map_err(failure)?;
    let a_after = source_hash(scratch)?;
    let b_after = source_hash(untouched)?;
    // Emit the exact statement/result even when a null/refused/stale record fails qualification.
    println!(
        "{}",
        serde_json::json!({"kind":"renewalAuthorityRead","statements":[{"sql":AUTHORITY,"result":result}],"scratchBeforeSha256":a_before,"scratchAfterSha256":a_after,"untouchedBeforeSha256":b_before,"untouchedAfterSha256":b_after})
    );
    if a_before != a_after || b_before != b_after {
        return Err(ObservationError::SourceChanged);
    }
    let observed = verify(
        result
            .as_deref()
            .ok_or_else(|| failure("authority is null; current renewal unobserved"))?,
        &expected,
    )?;
    println!(
        "{}",
        serde_json::json!({"kind":"renewalObserved","value":observed})
    );
    Ok(())
}

#[cfg(test)]
#[path = "renewal_observation_test.rs"]
mod tests;
