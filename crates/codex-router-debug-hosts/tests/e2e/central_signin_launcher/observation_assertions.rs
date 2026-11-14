//! Literal consumer assertions for modern observation contents, never Debug dumps.
use super::TestResult;
use collaboration_central_conformance::signin_fixtures as signed;
use collaboration_central_core::{
    identity_contract::{OrganizationFenceState, PersonFenceState},
    signin_test_observation::SignInObservation,
};

#[expect(
    clippy::panic_in_result_fn,
    reason = "Opaque fixture assertions must not print raw observation material."
)]
pub(super) fn assert_empty(observation: &SignInObservation) -> TestResult {
    assert!(
        observation.complete
            && observation.statement.is_none()
            && observation.stamp.is_none()
            && observation.records.is_empty()
            && observation.organization_floor.is_none()
            && observation.person_floors.is_empty()
            && observation.used_requests.is_empty()
            && observation.workspaces.is_empty(),
        "unknown or fresh-root observation is not the complete empty contract"
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Signed fixture bytes and stamp metadata are compared only by booleans."
)]
pub(super) fn assert_stamp(
    observation: &SignInObservation,
    compact: &str,
    expires_at: i64,
) -> TestResult {
    assert!(
        observation.complete,
        "organization observation is incomplete"
    );
    assert!(
        observation.statement.as_deref() == Some(compact),
        "raw fixture statement differs"
    );
    let stamp = observation.stamp.as_ref().ok_or("stamp metadata missing")?;
    assert!(
        stamp.digest == signed::digest(compact)
            && stamp.key_id.as_deref() == Some("stamp-a")
            && stamp.expires_at == Some(expires_at),
        "stamp digest, key or signed lifetime metadata differs"
    );
    assert!(
        observation.workspaces.is_empty(),
        "observation invented held setup rows"
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Actual persisted change identities and timestamps are checked without observation dumps."
)]
pub(super) fn assert_record(
    observation: &SignInObservation,
    index: usize,
    organization: &str,
    request: &str,
    kind: &str,
    at_ms: i64,
) -> TestResult {
    let record = observation
        .records
        .get(index)
        .ok_or("expected change record missing")?;
    assert!(
        record.request_id.as_str() == request
            && serde_json::to_value(record.kind)?.as_str() == Some(kind)
            && record.target_id == organization
            && record.created_at_millis == at_ms,
        "persisted change kind, request, target or applying timestamp differs"
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Floors and used proof rows must come from the real modern HTTP interaction."
)]
pub(super) fn assert_floors_and_used_request(
    observation: &SignInObservation,
    request: &str,
) -> TestResult {
    let floor = observation
        .organization_floor
        .as_ref()
        .ok_or("organization floor missing")?;
    assert!(
        floor.revision.get() == 3 && floor.state == OrganizationFenceState::Active,
        "organization floor differs"
    );
    assert_eq!(observation.person_floors.len(), 1);
    let floor = observation
        .person_floors
        .first()
        .ok_or("person floor missing")?;
    assert!(
        floor.subject_id.to_string() == signed::OWNER_SUBJECT
            && floor.person_id.to_string() == signed::OWNER
            && floor.revision.get() == 4
            && floor.state == PersonFenceState::Active,
        "person floor differs"
    );
    assert_eq!(observation.used_requests.len(), 1);
    let used = observation
        .used_requests
        .first()
        .ok_or("used proof row missing")?;
    assert!(
        used.id.to_string() == request,
        "actual used proof identity differs"
    );
    Ok(())
}
