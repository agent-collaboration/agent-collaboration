//! The routing-bug model on Tokio: organization A's Fleet store holds B's
//! genuinely signed stamp. Every A-named call that needs the stamp is refused,
//! nothing stored changes, and no Copy object is ever opened.
#![cfg(feature = "conformance")]
use crate::organization_provisioning_link::{
    ORGANIZATION_A, ORGANIZATION_B, PROVISION, provision_body, start, stored,
};
use crate::organization_stamps::{self as stamps, NOW_SECONDS, SignInKeys, link_post};
use collaboration_central_conformance::{ConformanceResult, JwksFixtureServer};
use serde_json::json;
use sqlx_core::connection::{ConnectOptions, Connection};
use std::path::Path;

const DIGEST: &str = "/internal/organization-statement-digest";

/// Native object files under the root; a Copy object opens `copy-<id>.db`.
fn object_files(root: &Path) -> ConformanceResult<Vec<String>> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if name.ends_with(".db") {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare the answers, the exact stored rows and the object files while fixture errors propagate."
)]
#[tokio::test]
async fn a_store_holding_another_organizations_stamp_refuses_every_stamped_call()
-> ConformanceResult<()> {
    // Wrong builds caught: a stored stamp trusted without matching the Fleet
    // it is stored in (the digest read answers 200, a setup answers 200); a
    // refusal that rewrites the stored stamp; a stamped call that reaches a
    // Copy object.
    let root = tempfile::tempdir()?;
    let access = JwksFixtureServer::start().await?;
    let keys = SignInKeys::start().await?;
    let central = start(root.path(), &access, &keys, NOW_SECONDS).await?;
    let provisioned = link_post(
        &central.socket,
        PROVISION,
        &provision_body(ORGANIZATION_A, &stamps::stamp(ORGANIZATION_A)?),
    )
    .await?;
    central.server.shutdown().await?;
    // The routing bug: A's store now holds B's statement, as stored by B.
    let mut store = sqlx_turso::TursoConnectOptions::new()
        .filename(root.path().join(format!("fleet-{ORGANIZATION_A}.db")))
        .connect()
        .await?;
    sqlx_core::query::query::<sqlx_turso::Turso>(
        "UPDATE organization_identity SET organization_statement = ?",
    )
    .bind(stamps::stamp(ORGANIZATION_B)?)
    .execute(&mut store)
    .await?;
    store.close().await?;
    let before = stored(root.path(), ORGANIZATION_A).await?;
    let files_before = object_files(root.path())?;

    let restarted = start(root.path(), &access, &keys, NOW_SECONDS).await?;
    let digest = link_post(
        &restarted.socket,
        DIGEST,
        &json!({"organizationId":ORGANIZATION_A}),
    )
    .await?;
    let setup = link_post(
        &restarted.socket,
        PROVISION,
        &provision_body(ORGANIZATION_A, &stamps::stamp(ORGANIZATION_A)?),
    )
    .await?;
    let foreign = link_post(
        &restarted.socket,
        PROVISION,
        &provision_body(ORGANIZATION_A, &stamps::stamp(ORGANIZATION_B)?),
    )
    .await?;
    restarted.server.shutdown().await?;
    let after = stored(root.path(), ORGANIZATION_A).await?;
    let files_after = object_files(root.path())?;
    keys.shutdown().await?;
    access.shutdown().await?;

    assert_eq!(provisioned.0, 200, "A provisions before the damage");
    assert_eq!(digest.0, 503, "the A-named stamp-guarded read is refused");
    assert_eq!(
        setup.0, 409,
        "an A-named setup is refused over a stamp nothing trusts"
    );
    assert_eq!(foreign.0, 403, "B's statement sent to A is refused");
    assert_eq!(after, before, "nothing stored changes");
    assert_eq!(
        files_after, files_before,
        "no object store was opened by the calls"
    );
    assert!(
        files_after.iter().all(|name| !name.starts_with("copy-")),
        "no Copy object exists: {files_after:?}"
    );
    Ok(())
}
