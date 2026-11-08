//! B2e apparatus on Tokio: the owner-only conformance observer answers over
//! HTTP with what the organization's Fleet store holds, agrees with the store
//! file, survives a restart, and never creates a store for an organization
//! central has not opened.
#![cfg(feature = "conformance")]
use crate::organization_admin_link::{
    CREATE, ORGANIZATION, OWNER, OWNER_SUBJECT, Person, answer, counts, create_body, create_intent,
    fresh_request_id, seed_floors, start,
};
use crate::organization_stamps::{self as stamps, NOW_SECONDS, ProofKey, SignInKeys, link_post};
use collaboration_central_conformance::{ConformanceResult, JwksFixtureServer};
use collaboration_central_core::AccessCaller;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const UNOPENED: &str = "019f0000-0000-7000-8000-00000000d2ff";

async fn observe(
    base_url: &str,
    organization: &str,
    assertion: Option<String>,
) -> ConformanceResult<(u16, Value)> {
    let mut request = reqwest::Client::builder()
        .no_proxy()
        .build()?
        .post(format!(
            "{base_url}{}",
            collaboration_central_core::ORGANIZATION_RECORDS_PATH
        ))
        .json(&json!({"organization":organization}));
    if let Some(assertion) = assertion {
        request = request.header("Cf-Access-Jwt-Assertion", assertion);
    }
    let response = request.send().await?;
    let status = response.status().as_u16();
    let bytes = response.bytes().await?;
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    Ok((status, body))
}

/// A fixture-signed assertion with the admin audience for `caller`.
fn admin_assertion(caller: AccessCaller) -> ConformanceResult<String> {
    collaboration_central_conformance::sign_assertion(
        &collaboration_central_conformance::fixture_claims(caller, true),
        collaboration_central_conformance::FIXTURE_KID,
    )
}

fn owner_access() -> Option<String> {
    collaboration_central_conformance::owner_assertion().ok()
}

fn length(observed: &Value, field: &str) -> ConformanceResult<i64> {
    Ok(i64::try_from(
        observed[field]
            .as_array()
            .ok_or_else(|| format!("{field} is not a list"))?
            .len(),
    )?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The journey keeps every observation and the file counts together while fixture errors propagate."
)]
#[tokio::test]
async fn the_owner_observer_reports_what_the_fleet_store_holds_across_a_restart()
-> ConformanceResult<()> {
    // Wrong builds caught: the route missing or open to a non-owner; a capped
    // or partial read disagreeing with the store file; an observation that
    // only works before a restart; an observation that opens a store.
    let root = tempfile::tempdir()?;
    let access = JwksFixtureServer::start().await?;
    let keys = SignInKeys::start().await?;
    let statement = stamps::stamp(ORGANIZATION)?;
    let (server, socket) = start(root.path(), &access, &keys).await?;
    let provisioned = link_post(
        &socket,
        "/internal/organization-provisioning",
        &json!({"organizationId":ORGANIZATION,"organizationStatement":statement}),
    )
    .await?;
    server.shutdown().await?;
    seed_floors(root.path()).await?;
    let (server, socket) = start(root.path(), &access, &keys).await?;
    let owner = Person {
        id: OWNER,
        subject: OWNER_SUBJECT,
        revision: 4,
        permissions: vec!["workspace:create"],
        second_factor_at: NOW_SECONDS - 60,
        key: ProofKey::generate()?,
    };
    let operation = "019f0000-0000-7000-8000-00000000d3a1";
    let created = answer(
        &socket,
        CREATE,
        &owner.headers(
            CREATE,
            &create_intent(operation, "observed"),
            &fresh_request_id(),
        )?,
        &create_body(operation, "observed"),
    )
    .await?;

    let refused = observe(server.base_url(), ORGANIZATION, None).await?;
    // Valid assertions from the same Access with the admin audience, from
    // callers who are not the owner: another person, and a machine.
    let other_person = observe(
        server.base_url(),
        ORGANIZATION,
        Some(admin_assertion(AccessCaller::Owner {
            email: "someone-else@example.test".into(),
        })?),
    )
    .await?;
    let machine = observe(
        server.base_url(),
        ORGANIZATION,
        Some(admin_assertion(AccessCaller::Machine {
            client_id: collaboration_central_conformance::access_client_id("observer-machine")?,
        })?),
    )
    .await?;
    let (live_status, live) = observe(server.base_url(), ORGANIZATION, owner_access()).await?;
    let (unopened_status, unopened) = observe(server.base_url(), UNOPENED, owner_access()).await?;
    server.shutdown().await?;
    let stored = counts(root.path()).await?;
    let (server, _socket) = start(root.path(), &access, &keys).await?;
    let (restarted_status, restarted) =
        observe(server.base_url(), ORGANIZATION, owner_access()).await?;
    server.shutdown().await?;

    assert_eq!(provisioned.0, 200, "provisioning");
    assert_eq!(created.0, 200, "workspace creation: {}", created.1);
    assert_eq!(refused.0, 401, "no assertion observes nothing");
    assert_eq!(
        other_person.0, 401,
        "another person's valid admin assertion observes nothing"
    );
    assert_eq!(
        machine.0, 401,
        "a machine's valid admin-audience assertion observes nothing"
    );
    assert_eq!(live_status, 200, "{live}");
    assert_eq!(live["complete"], true);
    assert_eq!(live["statement"], json!(statement));
    assert_eq!(live["stampedOrganization"], json!(ORGANIZATION));
    assert_eq!(
        live["stamp"]["digest"],
        json!(format!("{:x}", Sha256::digest(statement.as_bytes())))
    );
    assert_eq!(
        (
            length(&live, "workspaces")?,
            length(&live, "records")?,
            length(&live, "usedRequests")?,
        ),
        stored,
        "the observer agrees with the store file"
    );
    assert_eq!(
        stored,
        (1, 2, 1),
        "one workspace, setup and creation records, one marker"
    );
    assert_eq!(length(&live, "personFloors")?, 2);
    assert_eq!(live["organizationFloor"]["revision"], 3);
    assert_eq!(restarted_status, 200);
    assert_eq!(restarted, live, "the same observation after the restart");
    assert_eq!(unopened_status, 200);
    assert_eq!(unopened["complete"], true);
    assert_eq!(unopened["statement"], Value::Null);
    assert!(
        !root.path().join(format!("fleet-{UNOPENED}.db")).exists(),
        "observing an unopened organization created its store"
    );
    Ok(())
}
