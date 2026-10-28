use crate::{
    AccessCallerClass, AccessVerificationOutcome, ConformanceClient, ConformanceResult,
    scenario_assertions::equal,
};
use collaboration_central_core::AccessRefusal;
/// The `exp` claim of an assertion, read in memory so a run can wait for it.
/// Nothing else of the token is returned.
pub fn assertion_expiry(assertion: &str) -> ConformanceResult<chrono::DateTime<chrono::Utc>> {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    let payload = assertion.split('.').nth(1).ok_or("assertion payload")?;
    let claims: serde_json::Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload)?)?;
    let seconds = claims
        .get("exp")
        .and_then(serde_json::Value::as_i64)
        .ok_or("assertion exp")?;
    chrono::DateTime::from_timestamp(seconds, 0).ok_or_else(|| "assertion exp range".into())
}

/// A6 on a staging fault central. `inner` is a real session app token whose
/// app has a short session duration; `outer` is the owner's long-lived token,
/// which authenticates every control request. The inner token is accepted
/// while valid, then the same bytes are submitted after the real clock passes
/// its expiry and must be refused Expired, with the outer token accepted
/// around it.
pub async fn run_staging_expiry_row(
    client: &ConformanceClient,
    inner: String,
    outer: String,
) -> ConformanceResult<()> {
    use AccessVerificationOutcome::{Accepted, Refused};
    let expires = assertion_expiry(&inner)?;
    equal(
        client
            .verify_access_assertion(inner.clone(), "/v1/session")
            .await?,
        Accepted(AccessCallerClass::Machine),
    )?;
    println!(
        "STAGING A6 session token accepted; it expires in {}s",
        (expires - chrono::Utc::now()).num_seconds()
    );
    let wait = (expires - chrono::Utc::now() + chrono::TimeDelta::seconds(5))
        .to_std()
        .unwrap_or_default();
    tokio::time::sleep(wait).await;
    equal(
        client.verify_access_assertion(inner, "/v1/session").await?,
        Refused(AccessRefusal::Expired),
    )?;
    equal(
        client
            .verify_access_assertion(outer, "/admin/v1/machines/enroll")
            .await?,
        Accepted(AccessCallerClass::Owner),
    )?;
    println!("STAGING A6 same token after its expiry refused Expired, live token accepted: passed");
    Ok(())
}
