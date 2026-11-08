//! A sign-in key retired after every central restarted cold: a stamp stored
//! under it is refused after the next cold start, a new statement under it is
//! refused before any write, and stamps under the remaining key keep working.
#![cfg(feature = "conformance")]
use crate::organization_provisioning_link::{
    ORGANIZATION_A, PROVISION, control, provision_body, start, stored,
};
use crate::organization_stamps::{self as stamps, NOW_SECONDS, SignInKeys, link_post};
use collaboration_central_conformance::{ConformanceResult, JwksFixtureServer, RuntimeControl};
use serde_json::json;

const ORGANIZATION_B: &str = "019f0000-0000-7000-8000-00000000b2b1";
const ORGANIZATION_C: &str = "019f0000-0000-7000-8000-00000000b2c1";
const DIGEST: &str = "/internal/organization-statement-digest";
const PUSH: &str = "/internal/organization-authorization-revisions";

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare answers before and after the key is retired while fixture errors propagate."
)]
#[tokio::test]
async fn a_retired_key_refuses_its_stamps_after_a_cold_start() -> ConformanceResult<()> {
    // Wrong builds caught: a stamp trusted under a key the sign-in service no
    // longer publishes; a statement under a retired key accepted; a retirement
    // that stops the organizations stamped under the remaining key.
    let root = tempfile::tempdir()?;
    let access = JwksFixtureServer::start().await?;
    let keys = SignInKeys::start().await?;
    let central = start(root.path(), &access, &keys, NOW_SECONDS).await?;
    let under_retiring = stamps::stamp_under_second_key(ORGANIZATION_A)?;
    let stamped_retiring = link_post(
        &central.socket,
        PROVISION,
        &provision_body(ORGANIZATION_A, &under_retiring),
    )
    .await?;
    let stamped_remaining = link_post(
        &central.socket,
        PROVISION,
        &provision_body(ORGANIZATION_B, &stamps::stamp(ORGANIZATION_B)?),
    )
    .await?;
    let before = link_post(
        &central.socket,
        DIGEST,
        &json!({"organizationId":ORGANIZATION_A}),
    )
    .await?;
    central.server.shutdown().await?;

    keys.retire("stamp-b")?;
    let restarted = start(root.path(), &access, &keys, NOW_SECONDS).await?;
    let digest = link_post(
        &restarted.socket,
        DIGEST,
        &json!({"organizationId":ORGANIZATION_A}),
    )
    .await?;
    let setup_retry = link_post(
        &restarted.socket,
        PROVISION,
        &provision_body(ORGANIZATION_A, &under_retiring),
    )
    .await?;
    // A setup statement under the remaining key cannot revive the refused stamp.
    let other_setup = link_post(
        &restarted.socket,
        PROVISION,
        &provision_body(ORGANIZATION_A, &stamps::stamp(ORGANIZATION_A)?),
    )
    .await?;
    let push = link_post(
        &restarted.socket,
        PUSH,
        &json!({"kind":"organization","organizationId":ORGANIZATION_A,"revision":1,
            "state":"active",
            "permissionPolicyDigest":"2d31b5d72716e64e255d8858c231f593155289fb03ed354c4b762890667a3c22"}),
    )
    .await?;
    let new_under_retired = link_post(
        &restarted.socket,
        PROVISION,
        &provision_body(
            ORGANIZATION_C,
            &stamps::stamp_under_second_key(ORGANIZATION_C)?,
        ),
    )
    .await?;
    let remaining = link_post(
        &restarted.socket,
        DIGEST,
        &json!({"organizationId":ORGANIZATION_B}),
    )
    .await?;
    restarted.server.shutdown().await?;
    let (identity_a, records_a) = stored(root.path(), ORGANIZATION_A).await?;
    let (identity_c, records_c) = stored(root.path(), ORGANIZATION_C).await?;

    assert_eq!(
        (stamped_retiring.0, stamped_remaining.0, before.0),
        (200, 200, 200),
        "both keys published: both stamps accepted, the digest read works"
    );
    assert_eq!(
        digest.0, 503,
        "a stored stamp under the retired key is refused"
    );
    assert_eq!(
        setup_retry.0, 422,
        "a setup retry of the same statement no longer verifies under the retired key"
    );
    assert_eq!(
        other_setup.0, 409,
        "a valid setup statement over the refused stamp is refused"
    );
    assert_eq!(push.0, 503, "no floor reaches a refused stamp");
    assert_eq!(
        new_under_retired.0, 422,
        "a new statement under the retired key"
    );
    assert_eq!(remaining.0, 200, "the remaining key's stamp still works");
    assert_eq!(
        identity_a
            .iter()
            .map(|row| row.1.clone())
            .collect::<Vec<_>>(),
        vec![under_retiring],
        "the refused stamp stays as stored"
    );
    assert_eq!(records_a.len(), 1);
    assert!(
        identity_c.is_empty() && records_c.is_empty(),
        "nothing written for C"
    );
    keys.shutdown().await?;
    access.shutdown().await?;
    Ok(())
}

async fn set_time_millis(base: &str, now_millis: i64) -> ConformanceResult<()> {
    control(
        base,
        ORGANIZATION_A,
        RuntimeControl::SetTime {
            now: collaboration_central_core::RegistryTime(
                chrono::DateTime::from_timestamp_millis(now_millis).ok_or("fixture time")?,
            ),
        },
    )
    .await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare the answers and fetches across the key cache's refresh bound while fixture errors propagate."
)]
#[tokio::test]
async fn a_warm_key_cache_refuses_a_retired_key_once_its_refresh_bound_passes()
-> ConformanceResult<()> {
    // Wrong build caught: a cached key kept after the sign-in service retired
    // it. The cache keeps a fetched key set for min(max-age, 300 s) from its
    // receipt on the Fleet's clock; the fixture sends no max-age, so a retired
    // key still verifies at t0 + 299 999 ms and is refused from t0 + 300 000 ms,
    // when the cache first refreshes. Nothing asks for an unknown
    // kid before the bound, so nothing refreshes early. No restart, no wait.
    let root = tempfile::tempdir()?;
    let access = JwksFixtureServer::start().await?;
    let keys = SignInKeys::start().await?;
    let central = start(root.path(), &access, &keys, NOW_SECONDS).await?;
    let base = central.server.base_url().to_owned();
    let t0 = NOW_SECONDS * 1000;
    let first = stamps::stamp_under_second_key_at(ORGANIZATION_A, 0)?;
    let cached = link_post(
        &central.socket,
        PROVISION,
        &provision_body(ORGANIZATION_A, &first),
    )
    .await?;
    let warmed = keys.fetches();
    keys.retire("stamp-b")?;
    set_time_millis(&base, t0 + 299_999).await?;
    let inside_bound = link_post(
        &central.socket,
        PROVISION,
        &provision_body(
            ORGANIZATION_A,
            &stamps::stamp_under_second_key_at(ORGANIZATION_A, 1)?,
        ),
    )
    .await?;
    let inside_fetches = keys.fetches();
    set_time_millis(&base, t0 + 300_000).await?;
    let past_bound = link_post(
        &central.socket,
        PROVISION,
        &provision_body(
            ORGANIZATION_A,
            &stamps::stamp_under_second_key_at(ORGANIZATION_A, 2)?,
        ),
    )
    .await?;
    let boundary_fetches = keys.fetches();
    // The refresh also re-checked A's stored stamp, signed by the retired key:
    // a setup statement under the remaining key now meets a refused stamp.
    let over_refused = link_post(
        &central.socket,
        PROVISION,
        &provision_body(ORGANIZATION_A, &stamps::stamp(ORGANIZATION_A)?),
    )
    .await?;
    // Positive control: the refreshed key set is usable for a current key.
    let remaining_key = link_post(
        &central.socket,
        PROVISION,
        &provision_body(ORGANIZATION_B, &stamps::stamp(ORGANIZATION_B)?),
    )
    .await?;
    central.server.shutdown().await?;
    let (identity, records) = stored(root.path(), ORGANIZATION_A).await?;

    assert_eq!(cached.0, 200, "the key is published when the cache fills");
    assert_eq!(
        inside_bound.0, 200,
        "inside the bound the cached key still verifies (setup retry: no change)"
    );
    assert_eq!(
        past_bound.0, 422,
        "from the bound on, the retired key is refused"
    );
    assert_eq!(
        (warmed, inside_fetches, boundary_fetches),
        (1, 1, 3),
        // At the bound the stale set is refreshed to re-check the stored stamp
        // (fetch 2); the incoming kid then misses and its one forced refresh
        // follows (fetch 3). Each verification fetches at most once.
        "one fetch warms the cache, none inside the bound, refreshes only at it"
    );
    assert_eq!(
        over_refused.0, 409,
        "after the refresh the stamp stored under the retired key is refused"
    );
    assert_eq!(
        remaining_key.0, 200,
        "a statement under the remaining key provisions another organization"
    );
    assert_eq!(
        (
            identity.iter().map(|row| row.1.clone()).collect::<Vec<_>>(),
            records.len()
        ),
        (vec![first], 1),
        "A keeps exactly its first stamp and record"
    );
    keys.shutdown().await?;
    access.shutdown().await?;
    Ok(())
}
