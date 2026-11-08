//! B2b on Tokio: the sign-in link's provisioning and digest read over the real
//! Unix socket into the real server, with the Fleet stores read back cold.
#![cfg(feature = "conformance")]
use crate::configuration;
use crate::organization_stamps::{
    self as stamps, ADMIN_AUDIENCE, AUDIENCE, ISSUER, NOW_SECONDS, PUBLIC_ORIGIN, SignInKeys,
    link_post,
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

pub(crate) const ORGANIZATION_A: &str = "019f0000-0000-7000-8000-00000000b2a1";
pub(crate) const ORGANIZATION_B: &str = "019f0000-0000-7000-8000-00000000b2b1";
pub(crate) const PROVISION: &str = "/internal/organization-provisioning";
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
pub(crate) struct LinkedCentral {
    pub(crate) server: TokioServer,
    pub(crate) socket: std::path::PathBuf,
}
pub(crate) async fn start(
    root: &Path,
    access: &JwksFixtureServer,
    keys: &SignInKeys,
    now_seconds: i64,
) -> ConformanceResult<LinkedCentral> {
    let socket = root.join("sign-in-link.sock");
    let now = chrono::DateTime::from_timestamp(now_seconds, 0).ok_or("fixture time")?;
    let mut controls = Vec::new();
    let legacy = collaboration_central_conformance::legacy_organization();
    for organization in [ORGANIZATION_A, ORGANIZATION_B, legacy.as_str()] {
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
            admin_audience: ADMIN_AUDIENCE.into(),
            public_origin: PUBLIC_ORIGIN.into(),
            jwks_url: keys.url.clone(),
            socket: socket.clone(),
            service_id: "019f0000-0000-7000-8000-00000000e501"
                .to_owned()
                .try_into()?,
        },
        controls,
    )
    .await?;
    Ok(LinkedCentral { server, socket })
}

pub(crate) fn provision_body(organization: &str, statement: &str) -> Value {
    json!({"organizationId":organization,"organizationStatement":statement})
}

pub(crate) type IdentityRow = (String, String, i64, i64);
pub(crate) type RecordRow = (
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
pub(crate) async fn stored(
    root: &Path,
    organization: &str,
) -> ConformanceResult<(Vec<IdentityRow>, Vec<RecordRow>)> {
    stored_file(root, &format!("fleet-{organization}.db")).await
}

async fn stored_file(
    root: &Path,
    file: &str,
) -> ConformanceResult<(Vec<IdentityRow>, Vec<RecordRow>)> {
    let mut store = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join(file))
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

/// Drive a conformance control through the server's owner-only HTTP route.
pub(crate) async fn control(
    base: &str,
    organization: &str,
    command: RuntimeControl,
) -> ConformanceResult<()> {
    reqwest::Client::builder()
        .no_proxy()
        .build()?
        .post(format!("{base}/admin/v1/conformance/runtime"))
        .header(
            "Cf-Access-Jwt-Assertion",
            collaboration_central_conformance::owner_assertion()?,
        )
        .json(&RuntimeControlRequest {
            address: ObjectAddress::Fleet {
                organization: organization.to_owned().try_into()?,
            },
            command,
        })
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare live-clock refusals while fixture errors propagate."
)]
#[tokio::test]
async fn organization_link_refuses_a_stamp_once_the_live_fleet_clock_reaches_exp()
-> ConformanceResult<()> {
    // Wrong build caught: validity checked once at start and trusted afterwards,
    // so today's commands keep running on the same live Fleet past exp.
    let root = tempfile::tempdir()?;
    let access = JwksFixtureServer::start().await?;
    let keys = SignInKeys::start().await?;
    let legacy = collaboration_central_conformance::legacy_organization();
    let legacy = legacy.as_str().to_owned();
    let statement = stamps::stamp(&legacy)?;
    let central = start(root.path(), &access, &keys, NOW_SECONDS).await?;
    let base = central.server.base_url().to_owned();
    let provisioned = link_post(
        &central.socket,
        PROVISION,
        &provision_body(&legacy, &statement),
    )
    .await?;
    let enroll = |peer: &'static str| enroll_machine(&base, peer);
    let before = enroll("019f0000-0000-7000-8000-00000000b2e1").await?;
    let exp = chrono::DateTime::from_timestamp(NOW_SECONDS - 60 + stamps::LIFETIME_SECONDS, 0)
        .ok_or("fixture exp")?;
    control(
        &base,
        &legacy,
        RuntimeControl::SetTime {
            now: RegistryTime(exp),
        },
    )
    .await?;
    let after = enroll("019f0000-0000-7000-8000-00000000b2e2").await?;
    let digest = link_post(&central.socket, DIGEST, &json!({"organizationId":legacy})).await?;
    central.server.shutdown().await?;

    assert_eq!(provisioned.0, 200);
    assert_eq!(before, 200);
    assert_eq!(
        after, 503,
        "the same live Fleet must refuse every command at exp"
    );
    assert_eq!(digest.0, 503);
    keys.shutdown().await?;
    access.shutdown().await?;
    Ok(())
}

/// Today's Access-era owner command against the legacy Fleet: enroll a machine.
async fn enroll_machine(base: &str, peer: &str) -> ConformanceResult<u16> {
    let credential = format!("machine-credential-{peer}");
    Ok(reqwest::Client::builder()
        .no_proxy()
        .build()?
        .post(format!("{base}/admin/v1/machines/enroll"))
        .header(
            "Cf-Access-Jwt-Assertion",
            collaboration_central_conformance::owner_assertion()?,
        )
        .json(&json!({"kind":"enrollMachine","value":{
            "label":"stamp gate machine","installation":"550e8400-e29b-41d4-a716-44665544b2e0",
            "peer":peer,
            "access_client_id":collaboration_central_conformance::access_client_id(&credential)?,
            "credential_hash":stamps::digest(&credential),"expires_at":"2033-05-18T03:33:20Z"
        }}))
        .send()
        .await?
        .status()
        .as_u16())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare the faulted and retried provisioning while fixture errors propagate."
)]
#[tokio::test]
async fn organization_link_rolls_back_a_stamp_whose_record_fails_then_a_retry_commits_both()
-> ConformanceResult<()> {
    // Wrong build caught: the stamp committed without its record.
    let root = tempfile::tempdir()?;
    let access = JwksFixtureServer::start().await?;
    let keys = SignInKeys::start().await?;
    let statement = stamps::stamp(ORGANIZATION_A)?;
    let central = start(root.path(), &access, &keys, NOW_SECONDS).await?;
    let base = central.server.base_url().to_owned();
    control(
        &base,
        ORGANIZATION_A,
        RuntimeControl::FailNextOrganizationRecord,
    )
    .await?;
    let failed = link_post(
        &central.socket,
        PROVISION,
        &provision_body(ORGANIZATION_A, &statement),
    )
    .await?;
    let unstamped = link_post(
        &central.socket,
        DIGEST,
        &json!({"organizationId":ORGANIZATION_A}),
    )
    .await?;
    let retried = link_post(
        &central.socket,
        PROVISION,
        &provision_body(ORGANIZATION_A, &statement),
    )
    .await?;
    central.server.shutdown().await?;
    let (identity, records) = stored(root.path(), ORGANIZATION_A).await?;

    assert_eq!(failed.0, 503);
    assert_eq!(
        unstamped.0, 404,
        "the failed record must leave no stamp behind"
    );
    assert_eq!(retried.0, 200);
    assert_eq!(identity.len(), 1);
    assert_eq!(records.len(), 1);
    keys.shutdown().await?;
    access.shutdown().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare the preserved legacy store while fixture errors propagate."
)]
#[tokio::test]
async fn organization_link_stamps_the_legacy_store_and_keeps_its_data_across_restart_and_repeats()
-> ConformanceResult<()> {
    // Wrong builds caught: a fresh store replacing the configured legacy
    // fleet.db; a repeat after restart writing a second record; two first
    // provisions at once leaving two stamps.
    let root = tempfile::tempdir()?;
    let access = JwksFixtureServer::start().await?;
    let keys = SignInKeys::start().await?;
    let legacy = collaboration_central_conformance::legacy_organization();
    let legacy = legacy.as_str().to_owned();
    let first = stamps::stamp(&legacy)?;
    let mut second_claims = stamps::claims(&legacy);
    stamps::set_claim(&mut second_claims, "iat", json!(NOW_SECONDS - 59))?;
    let second = stamps::sign(&stamps::stamp_header(), &second_claims, 7)?;
    let central = start(root.path(), &access, &keys, NOW_SECONDS).await?;
    let base = central.server.base_url().to_owned();
    // Today's data in the legacy store: an enrolled machine.
    let credential = "legacy-machine-credential";
    let enrolled = reqwest::Client::builder()
        .no_proxy()
        .build()?
        .post(format!("{base}/admin/v1/machines/enroll"))
        .header(
            "Cf-Access-Jwt-Assertion",
            collaboration_central_conformance::owner_assertion()?,
        )
        .json(&json!({"kind":"enrollMachine","value":{
            "label":"legacy machine","installation":"550e8400-e29b-41d4-a716-44665544b2a7",
            "peer":"019f0000-0000-7000-8000-00000000b2a7",
            "access_client_id":collaboration_central_conformance::access_client_id(credential)?,
            "credential_hash":stamps::digest(credential),"expires_at":"2033-05-18T03:33:20Z"
        }}))
        .send()
        .await?
        .status()
        .as_u16();
    let first_body = provision_body(&legacy, &first);
    let second_body = provision_body(&legacy, &second);
    let (one, two) = tokio::join!(
        link_post(&central.socket, PROVISION, &first_body),
        link_post(&central.socket, PROVISION, &second_body),
    );
    central.server.shutdown().await?;
    let restarted = start(root.path(), &access, &keys, NOW_SECONDS).await?;
    let repeat = link_post(
        &restarted.socket,
        PROVISION,
        &provision_body(&legacy, &first),
    )
    .await?;
    restarted.server.shutdown().await?;
    let (identity, records) = stored_file(root.path(), "fleet.db").await?;
    let mut store = sqlx_turso::TursoConnectOptions::new()
        .filename(root.path().join("fleet.db"))
        .connect()
        .await?;
    let machines: Vec<(String,)> = query_as("SELECT machine_id FROM machine_enrollments")
        .fetch_all(&mut store)
        .await?;
    store.close().await?;

    assert_eq!(enrolled, 200);
    assert_eq!((one?.0, two?.0, repeat.0), (200, 200, 200));
    assert_eq!(
        identity.len(),
        1,
        "two first provisions must leave one stamp"
    );
    assert_eq!(records.len(), 1, "repeats must not add a setup record");
    assert_eq!(
        machines,
        vec![("019f0000-0000-7000-8000-00000000b2a7".to_owned(),)]
    );
    assert!(
        !root.path().join(format!("fleet-{legacy}.db")).exists(),
        "the legacy organization must keep its original store name"
    );
    keys.shutdown().await?;
    access.shutdown().await?;
    Ok(())
}
