//! Convert epoch milliseconds to the public observation timestamp.
use collaboration_protocol::ObservationTimestamp;

#[expect(
    clippy::result_unit_err,
    reason = "The extraction preserves callers' existing timestamp conversion error boundary."
)]
pub fn timestamp(value: i64) -> Result<ObservationTimestamp, ()> {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(value)
        .ok_or(())?
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        .try_into()
        .map_err(|_| ())
}
