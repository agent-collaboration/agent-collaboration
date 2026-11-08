//! An admin-created workspace records the sign-in service as the installation
//! that wrote it. No machine can be enrolled under that ServiceId, and an
//! enrolled machine gains nothing from the workspace's stored origin: it works
//! on the admin's workspace exactly as on its own, through the real session.
#![cfg(feature = "conformance")]
use crate::configuration;
use crate::organization_stamps::{
    self as stamps, ADMIN_AUDIENCE, AUDIENCE, ISSUER, NOW_SECONDS, PUBLIC_ORIGIN, ProofKey,
    SignInKeys, link_post, link_post_with,
};
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerProtocol, BoardConsumerRequest, BoardPayload,
    FleetRequestEnvelope,
};
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, JwksFixtureServer, RuntimeControl, RuntimeControlRequest,
    enroll_admitted_peer, fixture_policy,
};
use collaboration_central_core::{ObjectAddress, RegistryTime};
use collaboration_central_tokio::{SignInLinkSettings, TokioServer, TokioSettings};
use durable_steps::Uuid;
use fleet_admission::{FleetOperation, FleetRequest, FleetRequestResult};
use fleet_sync_protocol::{ClientFrame, FrameId, OperationAnswer, OperationRequest, ServerFrame};
use futures_util::{SinkExt, StreamExt};
use message_board::RequestDecision;
use reqwest_websocket::{Message, WebSocket};
use serde_json::{Value, json};
use sqlx_core::{
    connection::{ConnectOptions, Connection},
    query_as::query_as,
};
use std::{path::Path, time::Duration};

const SIGN_IN_SERVICE: &str = "019f0000-0000-7000-8000-00000000e501";
const OWNER: &str = "019f0000-0000-7000-8000-00000000e502";
const SUBJECT: &str = "019f0000-0000-7000-8000-00000000e503";
const SESSION: &str = "019f0000-0000-7000-8000-00000000e504";
const POLICY: &str = "2d31b5d72716e64e255d8858c231f593155289fb03ed354c4b762890667a3c22";
const MACHINE_SERVICE: &str = "550e8400-e29b-41d4-a716-44665544e505";
const CREATE: &str = "/organization-admin/v1/create-workspace";

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

async fn start(
    root: &Path,
    access: &JwksFixtureServer,
    keys: &SignInKeys,
) -> ConformanceResult<(TokioServer, std::path::PathBuf)> {
    let socket = root.join("sign-in-link.sock");
    let server = TokioServer::start_with_sign_in_and_controls(
        settings(root, &access.url)?,
        SignInLinkSettings {
            issuer: ISSUER.into(),
            audience: AUDIENCE.into(),
            admin_audience: ADMIN_AUDIENCE.into(),
            public_origin: PUBLIC_ORIGIN.into(),
            jwks_url: keys.url.clone(),
            socket: socket.clone(),
            service_id: SIGN_IN_SERVICE.to_owned().try_into()?,
        },
        vec![RuntimeControlRequest {
            address: ObjectAddress::Fleet {
                organization: collaboration_central_conformance::legacy_organization(),
            },
            command: RuntimeControl::SetTime {
                now: RegistryTime(
                    chrono::DateTime::from_timestamp(NOW_SECONDS, 0).ok_or("fixture time")?,
                ),
            },
        }],
    )
    .await?;
    Ok((server, socket))
}

/// The owner's admin create over the link, with a real token and proof.
async fn admin_create(socket: &Path, workspace: &str, name: &str) -> ConformanceResult<Value> {
    let key = ProofKey::generate()?;
    let organization = legacy();
    let token = stamps::sign(
        &json!({"typ":"at+jwt","alg":"EdDSA","kid":"stamp-a"}),
        &json!({
            "iss":ISSUER,"aud":ADMIN_AUDIENCE,"sub":OWNER,"sid":SESSION,
            "iat":NOW_SECONDS - 10,"exp":NOW_SECONDS + 290,"cnf":{"jkt":key.thumbprint()?},
            "token_profile":"person-fleet","organization_id":organization,
            "authorization_subject_id":SUBJECT,"person_revision":1,"organization_revision":1,
            "permission_policy_digest":POLICY,"permissions":["workspace:create"],
            "second_factor_at":NOW_SECONDS - 60,"second_factor_method":"app",
            "session_expires_at_ms":(NOW_SECONDS + 86_400) * 1000,
            "authorization_snapshot_at_ms":NOW_SECONDS * 1000 - 10_000,
            "authorization_snapshot_valid_until_ms":NOW_SECONDS * 1000 + 200_000
        }),
        7,
    )?;
    let intent = json!({"v":1,"operation":"CreateWorkspace","organization":organization,
        "target":organization,"operationId":workspace,"params":{"name":name,"description":""}});
    let millis = NOW_SECONDS * 1000 - 1000;
    let jti = format!(
        "{:08x}-{:04x}-7000-8000-00000000e5ff",
        millis >> 16,
        millis & 0xffff
    );
    let proof = key.proof(&json!({
        "jti":jti,"htm":"POST","htu":format!("{PUBLIC_ORIGIN}{CREATE}"),"iat":NOW_SECONDS - 1,
        "ath":stamps::sha256_url(token.as_bytes()),
        "intent":stamps::sha256_url(stamps::canonical(&intent).as_bytes())
    }))?;
    let (status, body) = link_post_with(
        socket,
        CREATE,
        &[
            ("authorization", format!("DPoP {token}")),
            ("dpop", proof),
        ],
        &json!({"organizationId":organization,"operationId":workspace,"name":name,"description":""}),
    )
    .await?;
    if status != 200 {
        return Err(format!("admin create failed: {status}").into());
    }
    Ok(serde_json::from_slice(&body)?)
}

async fn enroll(base: &str, installation: &str, peer: &str) -> ConformanceResult<Value> {
    let credential = format!("credential-{peer}");
    Ok(reqwest::Client::builder()
        .no_proxy()
        .build()?
        .post(format!("{base}/admin/v1/machines/enroll"))
        .header(
            "Cf-Access-Jwt-Assertion",
            collaboration_central_conformance::owner_assertion()?,
        )
        .json(&json!({"kind":"enrollMachine","value":{
            "label":"origin machine","installation":installation,"peer":peer,
            "access_client_id":collaboration_central_conformance::access_client_id(&credential)?,
            "credential_hash":stamps::digest(&credential),"expires_at":"2033-05-18T03:33:20Z"
        }}))
        .send()
        .await?
        .json()
        .await?)
}

/// One Fleet request over the machine's admitted session.
async fn machine_request(
    socket: &mut WebSocket,
    workspace: &str,
    operation: FleetOperation,
) -> ConformanceResult<FleetRequestResult> {
    let request = FleetRequest {
        request_id: message_board::RequestId::try_from(Uuid::now_v7().to_string())?,
        workspace: message_board::WorkspaceId::try_from(workspace.to_owned())?,
        actor: message_board::RecordedActor::Human {
            human_id: message_board::HumanId::try_from("machine user".to_owned())?,
        },
        acting_for: None,
        protocol_version: message_board::ProtocolVersion::CURRENT,
        operation,
    };
    let frame = FrameId::new(Uuid::now_v7())?;
    socket
        .send(Message::Text(serde_json::to_string(&ClientFrame::<
            BoardConsumerProtocol,
        >::Request {
            frame,
            body: OperationRequest::Consumer(BoardConsumerRequest::Fleet(FleetRequestEnvelope {
                request_id: request.request_id.clone(),
                operation: BoardPayload::encode(&request)?,
            })),
        })?))
        .await?;
    // Bounded by the session's own answer; feed hints may arrive between.
    let answer = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let message = socket
                .next()
                .await
                .ok_or("session closed before answering")??;
            let bytes = match message {
                Message::Text(text) => text.into_bytes(),
                Message::Binary(bytes) => bytes.to_vec(),
                _ => continue,
            };
            if let Ok(ServerFrame::<BoardConsumerProtocol>::Answer {
                frame: actual,
                body,
            }) = serde_json::from_slice(&bytes)
                && actual == frame
            {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(body);
            }
        }
    })
    .await??;
    match answer {
        OperationAnswer::Consumer(BoardConsumerAnswer::Fleet {
            request_id,
            decision,
        }) if request_id == request.request_id => Ok(decision.decode()?),
        _ => Err("machine request answered another operation".into()),
    }
}

fn update(name: &str) -> ConformanceResult<FleetOperation> {
    Ok(FleetOperation::WorkspaceUpdate {
        name: message_board::ResourceName::try_from(name.to_owned())?,
        description: message_board::Description::try_from(String::new())?,
    })
}
fn accepted(result: &FleetRequestResult) -> bool {
    matches!(
        result,
        FleetRequestResult::Decided(RequestDecision::Accepted { .. })
    )
}

async fn seed_floors(root: &Path) -> ConformanceResult<()> {
    let organization = legacy();
    let mut store = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join("fleet.db"))
        .connect()
        .await?;
    let at = NOW_SECONDS * 1000;
    for statement in [
        format!(
            "INSERT INTO organization_authorization_floors VALUES('{organization}',1,'active','{POLICY}',0,{at},{at})"
        ),
        format!(
            "INSERT INTO person_organization_authorization_floors VALUES('{SUBJECT}','{organization}','{OWNER}',1,'active',{at},{at})"
        ),
    ] {
        sqlx_core::query::query::<sqlx_turso::Turso>(sqlx_core::sql_str::AssertSqlSafe(statement))
            .execute(&mut store)
            .await?;
    }
    store.close().await?;
    Ok(())
}

async fn workspace_origins(root: &Path) -> ConformanceResult<Vec<(String, String, String)>> {
    let mut store = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join("fleet.db"))
        .connect()
        .await?;
    let origins = query_as(
        "SELECT workspace_id, origin_installation, origin_machine FROM fleet_workspaces ORDER BY workspace_id",
    )
    .fetch_all(&mut store)
    .await?;
    store.close().await?;
    Ok(origins)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare the enrollment refusal, machine decisions and stored origins while fixture errors propagate."
)]
#[tokio::test]
async fn organization_admin_origin_grants_no_machine_anything() -> ConformanceResult<()> {
    // Wrong builds caught: a machine enrolled under the sign-in service's
    // ServiceId; a machine gaining or losing rights over a workspace because
    // the sign-in service wrote it.
    let root = tempfile::tempdir()?;
    let access = JwksFixtureServer::start().await?;
    let keys = SignInKeys::start().await?;
    let organization = legacy();
    let (server, socket) = start(root.path(), &access, &keys).await?;
    let provisioned = link_post(
        &socket,
        "/internal/organization-provisioning",
        &json!({"organizationId":organization,"organizationStatement":stamps::stamp(&organization)?}),
    )
    .await?;
    server.shutdown().await?;
    seed_floors(root.path()).await?;
    let (server, socket) = start(root.path(), &access, &keys).await?;
    let admin_workspace = "019f0000-0000-7000-8000-00000000e510";
    let own_workspace = "019f0000-0000-7000-8000-00000000e511";
    let created = admin_create(&socket, admin_workspace, "admin workspace").await?;
    server.shutdown().await?;
    let written = workspace_origins(root.path()).await?;
    let (server, _socket) = start(root.path(), &access, &keys).await?;
    let base = server.base_url().to_owned();
    let collision = enroll(
        &base,
        SIGN_IN_SERVICE,
        "019f0000-0000-7000-8000-00000000e512",
    )
    .await?;
    let client = ConformanceClient::new(&base)?;
    let mut machine = enroll_admitted_peer(
        &client,
        MACHINE_SERVICE,
        "019f0000-0000-7000-8000-00000000e513",
        "origin machine",
        "019f0000-0000-7000-8000-00000000e514",
    )
    .await?;
    // Positive control first: the machine works on its own records.
    let own_create = machine_request(
        &mut machine.socket,
        own_workspace,
        FleetOperation::WorkspaceCreate {
            name: message_board::ResourceName::try_from("machine workspace".to_owned())?,
            description: message_board::Description::try_from(String::new())?,
        },
    )
    .await?;
    let own_update = machine_request(
        &mut machine.socket,
        own_workspace,
        update("machine renamed")?,
    )
    .await?;
    // The admin's workspace: exactly the same ordinary rule, no more, no less.
    let admin_update = machine_request(
        &mut machine.socket,
        admin_workspace,
        update("admin renamed")?,
    )
    .await?;
    drop(machine);
    server.shutdown().await?;
    let origins = workspace_origins(root.path()).await?;
    let mut store = sqlx_turso::TursoConnectOptions::new()
        .filename(root.path().join("fleet.db"))
        .connect()
        .await?;
    let enrolled: Vec<(String,)> =
        query_as("SELECT service_id FROM machine_enrollments ORDER BY service_id")
            .fetch_all(&mut store)
            .await?;
    store.close().await?;

    assert_eq!(provisioned.0, 200);
    assert_eq!(created.pointer("/result"), Some(&json!("applied")));
    assert_eq!(
        written,
        vec![(
            admin_workspace.into(),
            SIGN_IN_SERVICE.into(),
            "sign-in service".into()
        )],
        "the admin's workspace names the sign-in service as the installation that wrote it"
    );
    assert_eq!(
        collision,
        json!({"kind":"refused","value":"invalidInput"}),
        "no machine may be enrolled under the sign-in service's ServiceId"
    );
    assert!(
        accepted(&own_create) && accepted(&own_update),
        "positive control"
    );
    assert!(
        accepted(&admin_update),
        "the admin's workspace follows the ordinary rule"
    );
    assert_eq!(enrolled, vec![(MACHINE_SERVICE.to_owned(),)]);
    // Each record names the installation that last wrote it; nothing else.
    assert_eq!(
        origins,
        vec![
            (
                admin_workspace.into(),
                MACHINE_SERVICE.into(),
                "origin machine".into()
            ),
            (
                own_workspace.into(),
                MACHINE_SERVICE.into(),
                "origin machine".into()
            ),
        ]
    );
    keys.shutdown().await?;
    access.shutdown().await?;
    Ok(())
}
