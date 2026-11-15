//! Logical scenario roots pass this origin through the existing object controls.
use crate::ConformanceResult;
use collaboration_central_core::RegistryTime;

pub fn logical_scenario_origin() -> ConformanceResult<RegistryTime> {
    // Stay before the shared machine fixture's 2033 credential expiry and pin
    // before arranging timed state. Platform-real-time and edge-verifier
    // journeys keep their own real clock obligations and never use this value.
    Ok(RegistryTime(
        chrono::DateTime::from_timestamp_millis(1_893_456_000_000)
            .ok_or("logical scenario origin is outside the timestamp range")?,
    ))
}
