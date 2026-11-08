//! B2e apparatus on Tokio: one injected Fleet clock reaches every Fleet,
//! including an organization first created by its own provisioning, and moving
//! it moves Fleets created afterwards too. Access keeps the system clock.
#![cfg(feature = "conformance")]
use crate::fleet_clock::FleetClock;
use crate::organization_admin_link::settings;
use crate::organization_stamps::{
    self as stamps, ADMIN_AUDIENCE, AUDIENCE, ISSUER, NOW_SECONDS, PUBLIC_ORIGIN, SignInKeys,
    link_post,
};
use collaboration_central_conformance::{ConformanceResult, JwksFixtureServer};
use collaboration_central_tokio::{SignInLinkSettings, TokioServer};
use serde_json::{Value, json};

/// Never named to central before its own first provisioning.
const FIRST_NEW: &str = "019f0000-0000-7000-8000-00000000d6a1";
/// Created only after the clock moved.
const LATER_NEW: &str = "019f0000-0000-7000-8000-00000000d6a2";
const MOVE_SECONDS: i64 = 3_600;

async fn observe(server: &TokioServer, organization: &str) -> ConformanceResult<Value> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .build()?
        .post(format!(
            "{}{}",
            server.base_url(),
            collaboration_central_core::ORGANIZATION_RECORDS_PATH
        ))
        .header(
            "Cf-Access-Jwt-Assertion",
            collaboration_central_conformance::owner_assertion()?,
        )
        .json(&json!({"organization":organization}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

/// A renewal of `predecessor` issued after the clock moves, so a Fleet still
/// reading t0 finds it not yet valid.
fn renewal_after_move(organization: &str, predecessor: &str) -> ConformanceResult<String> {
    let issued = NOW_SECONDS + MOVE_SECONDS - 10;
    let mut claims = stamps::claims(organization);
    stamps::set_claim(&mut claims, "iat", json!(issued))?;
    stamps::set_claim(&mut claims, "exp", json!(issued + 86_400))?;
    stamps::set_claim(&mut claims, "replaces", json!(stamps::digest(predecessor)))?;
    stamps::sign(&stamps::stamp_header(), &claims, 7)
}

fn record_times(observed: &Value) -> Vec<Value> {
    observed["records"]
        .as_array()
        .map(|records| {
            records
                .iter()
                .map(|record| record["createdAtMillis"].clone())
                .collect()
        })
        .unwrap_or_default()
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The journey keeps every answer and observation together while fixture errors propagate."
)]
#[tokio::test]
async fn a_new_organization_runs_on_the_injected_fleet_clock_and_follows_it()
-> ConformanceResult<()> {
    // Wrong builds caught: a Fleet first created by provisioning running on
    // the system clock (its statement, issued in the fixture's future, is
    // refused); a moved clock not reaching Fleets created afterwards; Access
    // checked on the Fleet clock (the owner assertion is refused).
    let root = tempfile::tempdir()?;
    let access = JwksFixtureServer::start().await?;
    let keys = SignInKeys::start().await?;
    let start = NOW_SECONDS * 1000;
    let clock = FleetClock::at(start);
    let socket = root.path().join("sign-in-link.sock");
    let server = TokioServer::start_with_sign_in_controls_and_fleet_clock(
        settings(root.path(), &access.url)?,
        SignInLinkSettings {
            issuer: ISSUER.into(),
            audience: AUDIENCE.into(),
            admin_audience: ADMIN_AUDIENCE.into(),
            public_origin: PUBLIC_ORIGIN.into(),
            jwks_url: keys.url.clone(),
            socket: socket.clone(),
            service_id: "019f0000-0000-7000-8000-00000000e501"
                .to_owned()
                .try_into()?,
        },
        Vec::new(),
        clock.shared(),
    )
    .await?;

    let first = link_post(
        &socket,
        "/internal/organization-provisioning",
        &json!({"organizationId":FIRST_NEW,"organizationStatement":stamps::stamp(FIRST_NEW)?}),
    )
    .await?;
    let first_rows = observe(&server, FIRST_NEW).await?;
    clock.set(start + MOVE_SECONDS * 1000);
    let woken = server
        .conformance_platform()
        .conformance_wake_fleets()
        .await?;
    let later = link_post(
        &socket,
        "/internal/organization-provisioning",
        &json!({"organizationId":LATER_NEW,
            "organizationStatement":stamps::first_statement(LATER_NEW, MOVE_SECONDS)?}),
    )
    .await?;
    let later_rows = observe(&server, LATER_NEW).await?;
    // The already-live Fleet acts on the moved time: a renewal issued after t0
    // is not yet valid on a clock still at t0.
    let renewal = link_post(
        &socket,
        "/internal/organization-provisioning",
        &json!({"organizationId":FIRST_NEW,
            "organizationStatement":renewal_after_move(FIRST_NEW, &stamps::stamp(FIRST_NEW)?)?}),
    )
    .await?;
    let first_after = observe(&server, FIRST_NEW).await?;
    server.shutdown().await?;

    assert_eq!(
        first.0, 200,
        "first provisioning of a never-named organization"
    );
    assert_eq!(
        record_times(&first_rows),
        vec![json!(start)],
        "{first_rows}"
    );
    assert!(woken >= 1, "the live Fleet was woken after the move");
    assert_eq!(later.0, 200, "provisioning after the clock moved");
    assert_eq!(
        record_times(&later_rows),
        vec![json!(start + MOVE_SECONDS * 1000)],
        "{later_rows}"
    );
    assert_eq!(
        renewal.0, 200,
        "the live Fleet accepts a renewal issued after t0"
    );
    assert_eq!(
        record_times(&first_after),
        vec![json!(start), json!(start + MOVE_SECONDS * 1000)],
        "the live Fleet records its renewal at the moved time: {first_after}"
    );
    Ok(())
}
