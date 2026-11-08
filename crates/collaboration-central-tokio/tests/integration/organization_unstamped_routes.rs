//! Route classes on unstamped stores, by the Lead's transition ruling:
//! B2's own routes refuse on any unstamped store and reveal nothing; the
//! Access-era public routes keep working on the legacy store until the switch
//! and never reach a non-legacy object.
#![cfg(feature = "conformance")]
use crate::configuration;
use crate::organization_stamps::{
    self as stamps, ADMIN_AUDIENCE, AUDIENCE, ISSUER, NOW_SECONDS, PUBLIC_ORIGIN, ProofKey,
    SignInKeys, link_post, link_post_with,
};
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, JwksFixtureServer, RuntimeControl, RuntimeControlRequest,
    enroll_admitted_peer, fixture_policy,
};
use collaboration_central_core::{ObjectAddress, RegistryTime};
use collaboration_central_tokio::{SignInLinkSettings, TokioServer, TokioSettings};
use serde_json::json;
use sqlx_core::{
    connection::{ConnectOptions, Connection},
    query_scalar::query_scalar,
};
use std::{path::Path, time::Duration};

const OTHER: &str = "019f0000-0000-7000-8000-00000000f201";
const PERSON: &str = "019f0000-0000-7000-8000-00000000f202";
const SUBJECT: &str = "019f0000-0000-7000-8000-00000000f203";
const POLICY: &str = "2d31b5d72716e64e255d8858c231f593155289fb03ed354c4b762890667a3c22";
const PUSH: &str = "/internal/organization-authorization-revisions";

/// Every Access-era public route. Program Design step 2 deletes exactly these
/// in the deploy that provisions the owner's organization; until then they
/// serve only the legacy store.
const ACCESS_ERA_ROUTES: &[(&str, &str)] = &[
    ("GET", "/admin/v1/copies/{id}/health"),
    ("GET", "/admin/v1/copies"),
    ("POST", "/admin/v1/copies/{id}/repair"),
    ("POST", "/admin/v1/copies/migrate"),
    ("POST", "/admin/v1/import/fleet"),
    ("POST", "/admin/v1/import/projects/{id}"),
    ("POST", "/admin/v1/machines/enroll"),
    ("POST", "/admin/v1/machines/revoke"),
    ("POST", "/admin/v1/projects/{id}/move"),
    ("POST", "/admin/v1/projects/{id}/takeover/preview"),
    ("POST", "/admin/v1/projects/{id}/takeover"),
    ("POST", "/admin/v1/projects/{id}/retire"),
    ("GET", "/v1/session"),
];

/// B2's own routes: the private link's admin and push calls. B2 serves no
/// public route; the public paths later B2 work names are not served yet.
const B2_LINK_ADMIN_ROUTES: &[&str] = &[
    "/organization-admin/v1/create-workspace",
    "/organization-admin/v1/workspace-lookup",
    "/organization-admin/v1/change-records",
];
const B2_PUBLIC_PATHS_NOT_SERVED: &[(&str, &str)] = &[
    ("GET", "/fleet/v1/session"),
    ("POST", "/fleet/admin/v1/workspaces"),
];

fn legacy() -> String {
    collaboration_central_conformance::legacy_organization()
        .as_str()
        .to_owned()
}

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

fn pinned(organization: &str) -> ConformanceResult<RuntimeControlRequest> {
    Ok(RuntimeControlRequest {
        address: ObjectAddress::Fleet {
            organization: organization.to_owned().try_into()?,
        },
        command: RuntimeControl::SetTime {
            now: RegistryTime(
                chrono::DateTime::from_timestamp(NOW_SECONDS, 0).ok_or("fixture time")?,
            ),
        },
    })
}

async fn start(
    root: &Path,
    access: &JwksFixtureServer,
    keys: &SignInKeys,
    pinned_organizations: &[&str],
) -> ConformanceResult<(TokioServer, std::path::PathBuf)> {
    let socket = root.join("sign-in-link.sock");
    let controls = pinned_organizations
        .iter()
        .map(|organization| pinned(organization))
        .collect::<ConformanceResult<Vec<_>>>()?;
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
    Ok((server, socket))
}

/// A correctly signed, in-window admin request for `organization`.
async fn admin_call(
    socket: &Path,
    organization: &str,
    path: &str,
    serial: u16,
) -> ConformanceResult<(u16, Vec<u8>)> {
    let key = ProofKey::generate()?;
    let token = stamps::sign(
        &json!({"typ":"at+jwt","alg":"EdDSA","kid":"stamp-a"}),
        &json!({
            "iss":ISSUER,"aud":ADMIN_AUDIENCE,"sub":PERSON,"sid":"019f0000-0000-7000-8000-00000000f204",
            "iat":NOW_SECONDS - 10,"exp":NOW_SECONDS + 290,"cnf":{"jkt":key.thumbprint()?},
            "token_profile":"person-fleet","organization_id":organization,
            "authorization_subject_id":SUBJECT,"person_revision":1,"organization_revision":1,
            "permission_policy_digest":POLICY,
            "permissions":["workspace:create","organization:view","organization:readAudit"],
            "second_factor_at":NOW_SECONDS - 60,"second_factor_method":"app",
            "session_expires_at_ms":(NOW_SECONDS + 86_400) * 1000,
            "authorization_snapshot_at_ms":NOW_SECONDS * 1000 - 10_000,
            "authorization_snapshot_valid_until_ms":NOW_SECONDS * 1000 + 200_000
        }),
        7,
    )?;
    let workspace = "019f0000-0000-7000-8000-00000000f210";
    let (intent, body) = match path {
        "/organization-admin/v1/create-workspace" => (
            json!({"v":1,"operation":"CreateWorkspace","organization":organization,
                "target":organization,"operationId":workspace,
                "params":{"name":"research","description":""}}),
            json!({"organizationId":organization,"operationId":workspace,
                "name":"research","description":""}),
        ),
        "/organization-admin/v1/workspace-lookup" => (
            json!({"v":1,"operation":"WorkspaceLookup","organization":organization,
                "target":workspace,"operationId":null,"params":{}}),
            json!({"organizationId":organization,"workspaceId":workspace}),
        ),
        _ => (
            json!({"v":1,"operation":"ReadChangeRecords","organization":organization,
                "target":organization,"operationId":null,"params":{"after":null,"limit":10}}),
            json!({"organizationId":organization,"limit":10}),
        ),
    };
    let millis = NOW_SECONDS * 1000 - 1000;
    let jti = format!(
        "{:08x}-{:04x}-7000-8000-{:012x}",
        millis >> 16,
        millis & 0xffff,
        serial
    );
    let proof = key.proof(&json!({
        "jti":jti,"htm":"POST","htu":format!("{PUBLIC_ORIGIN}{path}"),"iat":NOW_SECONDS - 1,
        "ath":stamps::sha256_url(token.as_bytes()),
        "intent":stamps::sha256_url(stamps::canonical(&intent).as_bytes())
    }))?;
    link_post_with(
        socket,
        path,
        &[("authorization", format!("DPoP {token}")), ("dpop", proof)],
        &body,
    )
    .await
}

async fn public(
    base: &str,
    method: &str,
    path: &str,
    owner: bool,
) -> ConformanceResult<(u16, String)> {
    let client = reqwest::Client::builder().no_proxy().build()?;
    let url = format!("{base}{path}");
    let mut request = if method == "GET" {
        client.get(url)
    } else {
        client.post(url).json(&json!({}))
    };
    if owner {
        request = request.header(
            "Cf-Access-Jwt-Assertion",
            collaboration_central_conformance::owner_assertion()?,
        );
    }
    let response = request.send().await?;
    Ok((response.status().as_u16(), response.text().await?))
}

/// Rows B2 writes, in one organization's store; zero when the store is absent.
async fn b2_rows(root: &Path, file: &str) -> ConformanceResult<i64> {
    let path = root.join(file);
    if !path.exists() {
        return Ok(0);
    }
    let mut store = sqlx_turso::TursoConnectOptions::new()
        .filename(path)
        .connect()
        .await?;
    let mut total = 0;
    for table in [
        "organization_identity",
        "organization_change_records",
        "organization_used_requests",
        "organization_authorization_floors",
        "person_organization_authorization_floors",
    ] {
        total += query_scalar::<sqlx_turso::Turso, i64>(sqlx_core::sql_str::AssertSqlSafe(
            format!("SELECT count(*) FROM {table}"),
        ))
        .fetch_one(&mut store)
        .await?;
    }
    store.close().await?;
    Ok(total)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare each route's answer and the stores while fixture errors propagate."
)]
#[tokio::test]
async fn b2_routes_refuse_on_unstamped_stores_and_reveal_nothing() -> ConformanceResult<()> {
    // Wrong builds caught: a verified person reading or changing an unstamped
    // store (legacy or not) over the link; a floor landing before the stamp;
    // a B2 path answering on the public face, or answering differently from
    // any unserved path.
    let root = tempfile::tempdir()?;
    let access = JwksFixtureServer::start().await?;
    let keys = SignInKeys::start().await?;
    let legacy = legacy();
    let (server, socket) = start(root.path(), &access, &keys, &[&legacy, OTHER]).await?;
    let base = server.base_url().to_owned();
    let mut link_answers = Vec::new();
    let mut serial = 1;
    for organization in [legacy.as_str(), OTHER] {
        for path in B2_LINK_ADMIN_ROUTES {
            link_answers.push((
                organization.to_owned(),
                (*path).to_owned(),
                admin_call(&socket, organization, path, serial).await?,
            ));
            serial += 1;
        }
        let push = link_post(
            &socket,
            PUSH,
            &json!({"kind":"organization","organizationId":organization,"revision":1,
                "state":"active","permissionPolicyDigest":POLICY}),
        )
        .await?;
        link_answers.push((organization.to_owned(), PUSH.to_owned(), push));
    }
    let unserved = public(&base, "POST", "/no-such-route", true).await?;
    let mut b2_public = Vec::new();
    for (method, path) in B2_PUBLIC_PATHS_NOT_SERVED {
        b2_public.push(public(&base, method, path, true).await?);
    }
    let mut link_on_public = Vec::new();
    for path in B2_LINK_ADMIN_ROUTES.iter().copied().chain([
        PUSH,
        "/internal/organization-provisioning",
        "/internal/organization-statement-digest",
    ]) {
        link_on_public.push(public(&base, "POST", path, true).await?.0);
    }
    server.shutdown().await?;
    let legacy_rows = b2_rows(root.path(), "fleet.db").await?;
    let other_rows = b2_rows(root.path(), &format!("fleet-{OTHER}.db")).await?;

    for (organization, path, answer) in &link_answers {
        assert_eq!(
            answer,
            &(503, Vec::new()),
            "{path} on unstamped {organization} refuses and reveals nothing"
        );
    }
    assert_eq!(link_answers.len(), 8);
    assert!(
        b2_public.iter().all(|answer| answer == &unserved),
        "B2 public paths answer exactly like any unserved path: {b2_public:?} vs {unserved:?}"
    );
    assert_eq!(
        link_on_public,
        vec![404; 6],
        "every private link route, metadata included, is absent from the public face"
    );
    assert_eq!((legacy_rows, other_rows), (0, 0), "nothing was written");
    keys.shutdown().await?;
    access.shutdown().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare each route's reachability and the stores while fixture errors propagate."
)]
#[tokio::test]
async fn access_era_routes_keep_serving_only_the_unstamped_legacy_store() -> ConformanceResult<()> {
    // Wrong builds caught: an Access-era route refused on the legacy store
    // before the switch (the owner's production uses them); an Access-era
    // route creating or reaching a non-legacy Fleet object.
    let root = tempfile::tempdir()?;
    let access = JwksFixtureServer::start().await?;
    let keys = SignInKeys::start().await?;
    let legacy = legacy();
    let (server, _socket) = start(root.path(), &access, &keys, &[&legacy]).await?;
    let base = server.base_url().to_owned();
    // Positive control, before any stamp: a machine is enrolled and admitted
    // to a live session on the legacy store.
    let client = ConformanceClient::new(&base)?;
    // Kept as a result so the store isolation below is checked even when a
    // wrong build breaks the session first.
    let control = enroll_admitted_peer(
        &client,
        "550e8400-e29b-41d4-a716-44665544f205",
        "019f0000-0000-7000-8000-00000000f206",
        "transition machine",
        "019f0000-0000-7000-8000-00000000f207",
    )
    .await
    .map(drop)
    .map_err(|error| error.to_string());
    let unserved = public(&base, "POST", "/no-such-route", true).await?;
    let mut answers = Vec::new();
    for (method, route) in ACCESS_ERA_ROUTES {
        let path = route.replace("{id}", "019f0000-0000-7000-8000-00000000f208");
        answers.push((*route, public(&base, method, &path, true).await?));
    }
    server.shutdown().await?;
    let mut fleets = std::fs::read_dir(root.path())?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with("fleet") && name.ends_with(".db"))
        .collect::<Vec<_>>();
    fleets.sort();

    assert_eq!(
        fleets,
        vec!["fleet.db".to_owned()],
        "Access-era routes created no non-legacy Fleet store"
    );
    assert_eq!(
        control,
        Ok(()),
        "positive control: enrollment and a live session on the unstamped legacy store"
    );
    for (route, answer) in &answers {
        assert!(
            answer.0 != 404 && answer != &unserved,
            "{route} is an Access-era route served on the legacy store: {answer:?}"
        );
    }
    assert_eq!(answers.len(), ACCESS_ERA_ROUTES.len());
    keys.shutdown().await?;
    access.shutdown().await?;
    Ok(())
}
