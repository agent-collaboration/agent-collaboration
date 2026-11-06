//! An unrelated machine remains valid when the shared Fleet clock crosses A's expiry.

use super::{fixture, reconnect_unrelated_installation};
use collaboration_central_conformance::{ConformanceResult, access_client_id, owner_assertion};
use collaboration_central_core::EnrollmentDecision;
use fleet_sync_protocol::PeerId;
use serde_json::json;

pub(super) async fn retain_unrelated_credential(
    fixture: &mut fixture::Fixture,
) -> ConformanceResult<()> {
    let credential = "normal Move target";
    let credential_hash = ring::digest::digest(&ring::digest::SHA256, credential.as_bytes())
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let expiry: chrono::DateTime<chrono::Utc> = "2034-05-18T03:33:20Z".parse()?;
    let result: EnrollmentDecision = fixture
        .http
        .post(format!("{}/admin/v1/machines/enroll", fixture.base()))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(&json!({"kind":"enrollMachine","value":{
            "installation":fixture::TARGET_SERVICE,
            "peer":fixture::TARGET_PEER,
            "label":credential,
            "access_client_id":access_client_id(credential)?,
            "credential_hash":credential_hash,
            "expires_at":expiry,
        }}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if !matches!(result, EnrollmentDecision::MachineEnrolled { peer, expires_at, .. }
        if peer == PeerId::try_from(fixture::TARGET_PEER.to_owned())? && expires_at == expiry)
    {
        return Err("unrelated credential did not retain its distinct later expiry".into());
    }
    // Re-authenticate B so its real connection binding carries the new expiry.
    reconnect_unrelated_installation(fixture).await
}
