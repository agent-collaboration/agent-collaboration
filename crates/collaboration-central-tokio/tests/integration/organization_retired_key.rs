//! A sign-in key retired after every central restarted cold: a stamp stored
//! under it is refused after the next cold start, a new statement under it is
//! refused before any write, and stamps under the remaining key keep working.
#![cfg(feature = "conformance")]
use crate::organization_provisioning_link::{
    ORGANIZATION_A, PROVISION, provision_body, start, stored,
};
use crate::organization_stamps::{self as stamps, NOW_SECONDS, SignInKeys, link_post};
use collaboration_central_conformance::{ConformanceResult, JwksFixtureServer};
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
