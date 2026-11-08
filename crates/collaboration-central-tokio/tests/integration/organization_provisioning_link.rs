//! B2b on Tokio: the sign-in link's provisioning and digest read over the real
//! Unix socket into the real server, with the Fleet stores read back cold.
#![cfg(feature = "conformance")]
use crate::configuration;
use crate::organization_stamps::{
    self as stamps, AUDIENCE, ISSUER, NOW_SECONDS, SignInKeys, link_post,
};
use collaboration_central_conformance::{
    ConformanceResult, JwksFixtureServer, RuntimeControl, RuntimeControlRequest, fixture_policy,
};
use collaboration_central_core::{ObjectAddress, RegistryTime};
use collaboration_central_tokio::{SignInLinkSettings, TokioServer, TokioSettings};
use serde_json::{Value, json};
use sqlx_core::{
    connection::{ConnectOptions, Connection},
    query_as::query_as,
};
use std::{path::Path, time::Duration};

const ORGANIZATION_A: &str = "019f0000-0000-7000-8000-00000000b2a1";
const ORGANIZATION_B: &str = "019f0000-0000-7000-8000-00000000b2b1";
const PROVISION: &str = "/internal/organization-provisioning";
const DIGEST: &str = "/internal/organization-statement-digest";

fn settings(root: &Path, jwks: &str) -> ConformanceResult<TokioSettings> {
    Ok(TokioSettings {
        legacy_organization: collaboration_central_conformance::legacy_organization(),
        root: root.into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.into(),
        registry: configuration::registry_config()?,
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
}

/// The real server with both organizations' Fleet clocks pinned before startup.
struct LinkedCentral {
    server: TokioServer,
    socket: std::path::PathBuf,
}
async fn start(
    root: &Path,
    access: &JwksFixtureServer,
    keys: &SignInKeys,
    now_seconds: i64,
) -> ConformanceResult<LinkedCentral> {
    let socket = root.join("sign-in-link.sock");
    let now = chrono::DateTime::from_timestamp(now_seconds, 0).ok_or("fixture time")?;
    let mut controls = Vec::new();
    for organization in [ORGANIZATION_A, ORGANIZATION_B] {
        controls.push(RuntimeControlRequest {
            address: ObjectAddress::Fleet {
                organization: organization.to_owned().try_into()?,
            },
            command: RuntimeControl::SetTime {
                now: RegistryTime(now),
            },
        });
    }
    let server = TokioServer::start_with_sign_in_and_controls(
        settings(root, &access.url)?,
        SignInLinkSettings {
            issuer: ISSUER.into(),
            audience: AUDIENCE.into(),
            jwks_url: keys.url.clone(),
            socket: socket.clone(),
        },
        controls,
    )
    .await?;
    Ok(LinkedCentral { server, socket })
}

fn provision_body(organization: &str, statement: &str) -> Value {
    json!({"organizationId":organization,"organizationStatement":statement})
}

type IdentityRow = (String, String, i64, i64);
type RecordRow = (
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    i64,
    i64,
);
/// Read a stopped server's Fleet store exactly, so overwrites and timestamps show.
async fn stored(
    root: &Path,
    organization: &str,
) -> ConformanceResult<(Vec<IdentityRow>, Vec<RecordRow>)> {
    let mut store = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join(format!("fleet-{organization}.db")))
        .connect()
        .await?;
    let identity = query_as(
        "SELECT id, organization_statement, created_at, updated_at FROM organization_identity ORDER BY id",
    )
    .fetch_all(&mut store)
    .await?;
    let records = query_as(
        "SELECT id, request_id, kind, origin, actor_person_id, permission_checked, target_id, created_at, updated_at FROM organization_change_records ORDER BY id",
    )
    .fetch_all(&mut store)
    .await?;
    store.close().await?;
    Ok((identity, records))
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare the link's answers and the cold-read stores while fixture errors propagate."
)]
#[tokio::test]
async fn organization_link_provisions_once_and_a_repeat_changes_nothing() -> ConformanceResult<()> {
    // Wrong builds caught: a repeat writing a second record or rewriting the
    // stamp; "provisioned" without the commit; a person invented on set-up.
    let root = tempfile::tempdir()?;
    let access = JwksFixtureServer::start().await?;
    let keys = SignInKeys::start().await?;
    let statement = stamps::stamp(ORGANIZATION_A)?;
    let central = start(root.path(), &access, &keys, NOW_SECONDS).await?;

    let first = link_post(
        &central.socket,
        PROVISION,
        &provision_body(ORGANIZATION_A, &statement),
    )
    .await?;
    let repeat = link_post(
        &central.socket,
        PROVISION,
        &provision_body(ORGANIZATION_A, &statement),
    )
    .await?;
    let digest = link_post(
        &central.socket,
        DIGEST,
        &json!({"organizationId":ORGANIZATION_A}),
    )
    .await?;
    // The public TCP listener has no private route, before any credential.
    let public = reqwest::Client::builder()
        .no_proxy()
        .build()?
        .post(format!("{}{PROVISION}", central.server.base_url()))
        .json(&provision_body(ORGANIZATION_A, &statement))
        .send()
        .await?
        .status()
        .as_u16();
    central.server.shutdown().await?;
    let (identity, records) = stored(root.path(), ORGANIZATION_A).await?;
    assert_eq!(public, 404);

    assert_eq!(first, (200, br#"{"result":"provisioned"}"#.to_vec()));
    assert_eq!(repeat, first);
    assert_eq!(digest.0, 200);
    assert_eq!(
        serde_json::from_slice::<Value>(&digest.1)?,
        json!({"digest":stamps::digest(&statement),"expiresAt":NOW_SECONDS - 60 + stamps::LIFETIME_SECONDS,"signingKeyId":"stamp-a"})
    );
    let at = NOW_SECONDS * 1000;
    assert_eq!(identity, vec![(ORGANIZATION_A.into(), statement, at, at)]);
    assert_eq!(records.len(), 1);
    let record = records.first().ok_or("setup record missing")?;
    assert_eq!(
        (
            record.1.as_str(),
            record.2.as_str(),
            record.3.as_str(),
            &record.4,
            &record.5,
            record.6.as_str(),
            record.7,
        ),
        (
            ORGANIZATION_A,
            "organizationProvisioned",
            "signInProvisioning",
            &None,
            &None,
            ORGANIZATION_A,
            at
        )
    );
    assert_eq!(
        durable_steps::Uuid::parse_str(&record.0)?.get_version_num(),
        7
    );
    keys.shutdown().await?;
    access.shutdown().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare refusals with unchanged cold-read stores while fixture errors propagate."
)]
#[tokio::test]
async fn organization_link_refuses_forged_foreign_and_unpublished_stamps_before_writing()
-> ConformanceResult<()> {
    // Wrong builds caught: a stamp trusted without its signature or profile;
    // B's statement stamping A's store; an unstamped store answering a digest.
    let root = tempfile::tempdir()?;
    let access = JwksFixtureServer::start().await?;
    let keys = SignInKeys::start().await?;
    let central = start(root.path(), &access, &keys, NOW_SECONDS).await?;
    let unstamped_digest = link_post(
        &central.socket,
        DIGEST,
        &json!({"organizationId":ORGANIZATION_A}),
    )
    .await?;
    let mut refusals = Vec::new();
    let valid = stamps::stamp(ORGANIZATION_A)?;
    let edited = stamps::tamper_signature(&valid)?;
    let wrong = |field: &str, value: Value| {
        let mut claims = stamps::claims(ORGANIZATION_A);
        stamps::set_claim(&mut claims, field, value)?;
        stamps::sign(&stamps::stamp_header(), &claims, 7)
    };
    let cases = vec![
        ("edited signature", edited),
        ("wrong purpose", wrong("purpose", json!("fleet-person"))?),
        (
            "wrong audience",
            wrong("aud", json!("https://other.invalid"))?,
        ),
        (
            "wrong issuer",
            wrong("iss", json!("https://other.invalid"))?,
        ),
        (
            "longer than 120 days",
            wrong(
                "exp",
                json!(NOW_SECONDS - 60 + stamps::LIFETIME_SECONDS + 1),
            )?,
        ),
        ("B's real statement", stamps::stamp(ORGANIZATION_B)?),
        (
            "wrong type",
            stamps::sign(
                &json!({"typ":"at+jwt","alg":"EdDSA","kid":"stamp-a"}),
                &stamps::claims(ORGANIZATION_A),
                7,
            )?,
        ),
        (
            "unpublished key",
            stamps::sign(
                &json!({"typ":"fleet-organization-stamp+jwt","alg":"EdDSA","kid":"stamp-z"}),
                &stamps::claims(ORGANIZATION_A),
                9,
            )?,
        ),
    ];
    for (case, statement) in cases {
        let (status, body) = link_post(
            &central.socket,
            PROVISION,
            &provision_body(ORGANIZATION_A, &statement),
        )
        .await?;
        refusals.push((case, status, body.is_empty()));
    }
    central.server.shutdown().await?;
    let (identity, records) = stored(root.path(), ORGANIZATION_A).await?;

    assert_eq!(unstamped_digest.0, 404);
    assert_eq!(
        refusals,
        vec![
            ("edited signature", 422, true),
            ("wrong purpose", 422, true),
            ("wrong audience", 422, true),
            ("wrong issuer", 422, true),
            ("longer than 120 days", 422, true),
            ("B's real statement", 403, true),
            ("wrong type", 422, true),
            ("unpublished key", 422, true),
        ]
    );
    assert!(identity.is_empty());
    assert!(records.is_empty());
    keys.shutdown().await?;
    access.shutdown().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare renewal answers and the cold-read store while fixture errors propagate."
)]
#[tokio::test]
async fn organization_link_renewal_replaces_only_the_named_stamp() -> ConformanceResult<()> {
    // Wrong builds caught: a late statement prepared against the older stamp
    // winning; a setup retry restoring the old stamp; a lost-reply renewal
    // replay refused or recorded twice.
    let root = tempfile::tempdir()?;
    let access = JwksFixtureServer::start().await?;
    let keys = SignInKeys::start().await?;
    let original = stamps::stamp(ORGANIZATION_A)?;
    let renewal = stamps::renewal(ORGANIZATION_A, &original, 10)?;
    let late = stamps::renewal(ORGANIZATION_A, &original, 11)?;
    let central = start(root.path(), &access, &keys, NOW_SECONDS).await?;

    let answers = vec![
        link_post(
            &central.socket,
            PROVISION,
            &provision_body(ORGANIZATION_A, &original),
        )
        .await?
        .0,
        link_post(
            &central.socket,
            PROVISION,
            &provision_body(ORGANIZATION_A, &renewal),
        )
        .await?
        .0,
        link_post(
            &central.socket,
            PROVISION,
            &provision_body(ORGANIZATION_A, &late),
        )
        .await?
        .0,
        link_post(
            &central.socket,
            PROVISION,
            &provision_body(ORGANIZATION_A, &original),
        )
        .await?
        .0,
        link_post(
            &central.socket,
            PROVISION,
            &provision_body(ORGANIZATION_A, &renewal),
        )
        .await?
        .0,
    ];
    let digest = link_post(
        &central.socket,
        DIGEST,
        &json!({"organizationId":ORGANIZATION_A}),
    )
    .await?;
    central.server.shutdown().await?;
    let (identity, records) = stored(root.path(), ORGANIZATION_A).await?;

    assert_eq!(answers, vec![200, 200, 409, 200, 200]);
    assert_eq!(
        serde_json::from_slice::<Value>(&digest.1)?,
        json!({"digest":stamps::digest(&renewal),"expiresAt":NOW_SECONDS - 50 + stamps::LIFETIME_SECONDS,"signingKeyId":"stamp-a"})
    );
    assert_eq!(identity.len(), 1);
    assert_eq!(identity.first().ok_or("identity row missing")?.1, renewal);
    let kinds = records
        .iter()
        .map(|record| (record.1.clone(), record.2.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![
            (
                ORGANIZATION_A.to_owned(),
                "organizationProvisioned".to_owned()
            ),
            (stamps::digest(&renewal), "stampRenewed".to_owned()),
        ]
    );
    keys.shutdown().await?;
    access.shutdown().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare post-restart refusals while fixture errors propagate."
)]
#[tokio::test]
async fn organization_link_refuses_a_stamp_edited_on_disk_or_expired_after_restart()
-> ConformanceResult<()> {
    // Wrong builds caught: a stored stamp trusted without rechecking its
    // signature at start; validity checked only at start, not at each use.
    let root = tempfile::tempdir()?;
    let access = JwksFixtureServer::start().await?;
    let keys = SignInKeys::start().await?;
    let statement = stamps::stamp(ORGANIZATION_A)?;
    let other = stamps::stamp(ORGANIZATION_B)?;
    let central = start(root.path(), &access, &keys, NOW_SECONDS).await?;
    assert_eq!(
        link_post(
            &central.socket,
            PROVISION,
            &provision_body(ORGANIZATION_A, &statement)
        )
        .await?
        .0,
        200
    );
    assert_eq!(
        link_post(
            &central.socket,
            PROVISION,
            &provision_body(ORGANIZATION_B, &other)
        )
        .await?
        .0,
        200
    );
    central.server.shutdown().await?;
    let tampered = stamps::tamper_signature(&statement)?;
    let mut store = sqlx_turso::TursoConnectOptions::new()
        .filename(root.path().join(format!("fleet-{ORGANIZATION_A}.db")))
        .connect()
        .await?;
    sqlx_core::query::query::<sqlx_turso::Turso>(
        "UPDATE organization_identity SET organization_statement = ?",
    )
    .bind(tampered)
    .execute(&mut store)
    .await?;
    store.close().await?;

    let restarted = start(root.path(), &access, &keys, NOW_SECONDS).await?;
    let edited = link_post(
        &restarted.socket,
        DIGEST,
        &json!({"organizationId":ORGANIZATION_A}),
    )
    .await?;
    let retry = link_post(
        &restarted.socket,
        PROVISION,
        &provision_body(ORGANIZATION_A, &statement),
    )
    .await?;
    let untouched = link_post(
        &restarted.socket,
        DIGEST,
        &json!({"organizationId":ORGANIZATION_B}),
    )
    .await?;
    restarted.server.shutdown().await?;
    // The same valid stamp, observed after its exp on the Fleet's own clock.
    let expired_at = NOW_SECONDS - 60 + stamps::LIFETIME_SECONDS;
    let later = start(root.path(), &access, &keys, expired_at).await?;
    let expired = link_post(
        &later.socket,
        DIGEST,
        &json!({"organizationId":ORGANIZATION_B}),
    )
    .await?;
    later.server.shutdown().await?;

    assert_eq!(edited.0, 503);
    assert_eq!(
        retry.0, 409,
        "a setup retry must not answer over a refused stamp"
    );
    assert_eq!(untouched.0, 200);
    assert_eq!(expired.0, 503);
    keys.shutdown().await?;
    access.shutdown().await?;
    Ok(())
}
