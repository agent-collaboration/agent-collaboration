//! A statement must still be valid when its applying transaction runs, not
//! only when it verified. Each case holds a verified provisioning at the
//! pre-apply barrier, moves the owning Fleet's clock, then releases it.
#![cfg(feature = "conformance")]
use crate::organization_provisioning_link::{
    ORGANIZATION_A, PROVISION, control, provision_body, start, stored,
};
use crate::organization_provisioning_races::held_provisioning;
use crate::organization_stamps::{self as stamps, NOW_SECONDS, SignInKeys, link_post};
use collaboration_central_conformance::{ConformanceResult, JwksFixtureServer, RuntimeControl};
use serde_json::json;
use std::path::Path;

const DIGEST: &str = "/internal/organization-statement-digest";

/// Hold `held`'s provisioning after it verifies, move the Fleet's clock to
/// `now_seconds`, then release it into its applying transaction.
async fn held_across(
    base: &str,
    socket: &Path,
    held: String,
    now_seconds: i64,
) -> ConformanceResult<(u16, Vec<u8>)> {
    let key = held_provisioning()?;
    control(
        base,
        ORGANIZATION_A,
        RuntimeControl::HoldNextDispatch { key: key.clone() },
    )
    .await?;
    let held_socket = socket.to_owned();
    let held_call = tokio::spawn(async move {
        link_post(
            &held_socket,
            PROVISION,
            &provision_body(ORGANIZATION_A, &held),
        )
        .await
        .map_err(|error| error.to_string())
    });
    // Returns once the held call has verified and stopped before applying.
    control(
        base,
        ORGANIZATION_A,
        RuntimeControl::AwaitHeldDispatch { key: key.clone() },
    )
    .await?;
    control(
        base,
        ORGANIZATION_A,
        RuntimeControl::SetTime {
            now: collaboration_central_core::RegistryTime(
                chrono::DateTime::from_timestamp(now_seconds, 0).ok_or("fixture time")?,
            ),
        },
    )
    .await?;
    control(base, ORGANIZATION_A, RuntimeControl::ResumeDispatch { key }).await?;
    Ok(held_call.await??)
}

/// A renewal of `predecessor` that expires `shorter_by` seconds before it, so
/// the stored stamp is still valid when the renewal's own `exp` arrives.
fn short_renewal(predecessor: &str, shorter_by: i64) -> ConformanceResult<String> {
    let mut claims = stamps::claims(ORGANIZATION_A);
    stamps::set_claim(&mut claims, "iat", json!(NOW_SECONDS - 50))?;
    stamps::set_claim(
        &mut claims,
        "exp",
        json!(NOW_SECONDS - 60 + stamps::LIFETIME_SECONDS - shorter_by),
    )?;
    stamps::set_claim(&mut claims, "replaces", json!(stamps::digest(predecessor)))?;
    stamps::sign(&stamps::stamp_header(), &claims, 7)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare the held answers and the cold-read stores while fixture errors propagate."
)]
#[tokio::test]
async fn a_first_provision_held_past_its_exp_is_refused_and_one_released_before_it_applies()
-> ConformanceResult<()> {
    // Wrong build caught: expiry checked only when the statement verified, so
    // a statement released into its transaction at `exp` still stamps the
    // store. The positive control is released one second before `exp` and the
    // organization is then usable.
    let exp = NOW_SECONDS - 60 + stamps::LIFETIME_SECONDS;
    let mut cases = Vec::new();
    for released_at in [exp, exp - 1] {
        let root = tempfile::tempdir()?;
        let access = JwksFixtureServer::start().await?;
        let keys = SignInKeys::start().await?;
        let central = start(root.path(), &access, &keys, NOW_SECONDS).await?;
        let base = central.server.base_url().to_owned();
        let held = held_across(
            &base,
            &central.socket,
            stamps::stamp(ORGANIZATION_A)?,
            released_at,
        )
        .await?;
        let digest = link_post(
            &central.socket,
            DIGEST,
            &json!({"organizationId":ORGANIZATION_A}),
        )
        .await?;
        central.server.shutdown().await?;
        let (identity, records) = stored(root.path(), ORGANIZATION_A).await?;
        cases.push((held.0, digest.0, identity.len(), records.len()));
        keys.shutdown().await?;
        access.shutdown().await?;
    }

    assert_eq!(
        cases.first(),
        Some(&(422, 404, 0, 0)),
        "released at exp: refused, unprovisioned, no identity, stamp or record"
    );
    assert_eq!(
        cases.get(1),
        Some(&(200, 200, 1, 1)),
        "released one second before exp: provisioned and readable"
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare the held answers and the exact stored rows while fixture errors propagate."
)]
#[tokio::test]
async fn a_renewal_or_replay_held_past_its_exp_changes_nothing() -> ConformanceResult<()> {
    // Wrong builds caught: a renewal whose expiry was checked only on arrival
    // replacing a still-valid stored stamp after its own `exp`; a check placed
    // only on the initialize and replace paths, so a held byte-identical replay
    // still answers Provisioned after `exp`.
    let original = stamps::stamp(ORGANIZATION_A)?;
    let original_exp = NOW_SECONDS - 60 + stamps::LIFETIME_SECONDS;
    let renewal = short_renewal(&original, 1_000)?;
    let renewal_exp = original_exp - 1_000;
    let mut cases = Vec::new();
    for (held, released_at) in [(renewal, renewal_exp), (original.clone(), original_exp)] {
        let root = tempfile::tempdir()?;
        let access = JwksFixtureServer::start().await?;
        let keys = SignInKeys::start().await?;
        let central = start(root.path(), &access, &keys, NOW_SECONDS).await?;
        let provisioned = link_post(
            &central.socket,
            PROVISION,
            &provision_body(ORGANIZATION_A, &original),
        )
        .await?;
        central.server.shutdown().await?;
        let before = stored(root.path(), ORGANIZATION_A).await?;
        let central = start(root.path(), &access, &keys, NOW_SECONDS).await?;
        let base = central.server.base_url().to_owned();
        let answer = held_across(&base, &central.socket, held, released_at).await?;
        central.server.shutdown().await?;
        let after = stored(root.path(), ORGANIZATION_A).await?;
        cases.push((
            provisioned.0,
            answer.0,
            before == after,
            after.0.len(),
            after.1.len(),
        ));
        keys.shutdown().await?;
        access.shutdown().await?;
    }

    assert_eq!(
        cases.first(),
        Some(&(200, 422, true, 1, 1)),
        "a renewal released at its exp, its predecessor still valid: refused, rows exactly unchanged"
    );
    assert_eq!(
        cases.get(1),
        Some(&(200, 422, true, 1, 1)),
        "a byte-identical replay released at its exp: refused, rows exactly unchanged"
    );
    Ok(())
}
