//! The CLI is the sole enrollment producer; central and production Host consume its output.
// Protocol assertions; every owned process is stopped before propagating errors.
#![deny(clippy::too_many_lines)]
use board_central_test_support::EnrollmentTestCentral;
use chrono::{Duration as ChronoDuration, Utc};
use codex_router_secret_store::SecretStore;
use collaboration_central_core::{EnrollmentDecision, EnrollmentRefusal};
use collaboration_client::CollaborationClient;
use message_board::public_replies::{RequestDecision, RequestResult, WriteReply};
use sha2::{Digest, Sha256};
use std::{error::Error, io, time::Duration};
#[path = "support/fleet_enrollment_failures.rs"]
mod failures;
#[path = "support/fleet_cli.rs"]
mod fleet_cli;
#[path = "support/fleet_fixture.rs"]
mod fleet_fixture;
#[path = "support/fleet_host.rs"]
mod fleet_host;
#[path = "support/fleet_enrollment_status.rs"]
mod status;
use fleet_fixture::ProducerFixture;
type TestResult<T = ()> = Result<T, Box<dyn Error>>;
const BOUND: Duration = Duration::from_secs(30);

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn actual_enrollment_command_attaches_production_host_and_creates_project() -> TestResult {
    let fixture = ProducerFixture::start().await?;
    fixture
        .run(async |fixture| {
            let root = &fixture.root;
            let router_root = &fixture.router_root;
            let central = &fixture.central;
            let directory = codex_router_host::router_collaboration_directory(router_root);
            assert_eq!(central.session_connections(), 0);
            let output = fleet_cli::run(fleet_cli::enrollment(
                router_root,
                central,
                &future_expiry(),
            )?)
            .await?;
            let reply = fleet_cli::require_success(&output)?;
            assert_eq!(reply["decision"]["kind"], "machineEnrolled");
            assert_eq!(reply["activation"], "nextRouterStart");
            assert_eq!(central.enrollment_calls(), 1);
            assert_eq!(
                central.session_connections(),
                0,
                "enrollment must not start a Host"
            );
            let (document, credential) = assert_produced_enrollment(router_root, central)?;
            let raw = credential
                .expose_secret()
                .strip_prefix("Bearer ")
                .ok_or("missing Bearer prefix")?;
            let before = fleet_cli::snapshot(router_root)?;
            let status_output = fleet_cli::status(router_root).await?;
            let status = fleet_cli::require_success(&status_output)?;
            assert_eq!(status["status"]["kind"], "valid");
            assert_eq!(
                status["status"]["enrollment"]["peer"],
                document.peer.to_string()
            );
            assert_eq!(before, fleet_cli::snapshot(router_root)?);
            assert_eq!(
                central.enrollment_calls(),
                1,
                "status contacted the owner route"
            );
            fleet_cli::assert_private(
                &output,
                &[central.owner_token(), central.access_client_secret(), raw],
            );
            fleet_cli::assert_private(
                &status_output,
                &[central.owner_token(), central.access_client_secret(), raw],
            );
            fleet_host::run_host_scenario(root.path(), async |host| {
                host.await_ready().await?;
                let client = CollaborationClient::connect(&directory, "producer-test", "1").await?;
                create_and_read_project(&client).await?;
                assert!(central.session_connections() > 0);
                assert!(
                    central
                        .session_authorization_hashes()
                        .map_err(io::Error::other)?
                        .contains(&format!(
                            "{:x}",
                            Sha256::digest(credential.expose_secret().as_bytes())
                        ))
                );
                Ok::<_, Box<dyn Error>>(())
            })
            .await?;
            Ok::<_, Box<dyn Error>>(())
        })
        .await
}

fn future_expiry() -> String {
    (Utc::now() + ChronoDuration::hours(1)).to_rfc3339()
}

async fn create_and_read_project(client: &CollaborationClient) -> TestResult {
    let (workspace, board, project) = project_requests()?;
    require_accepted(settled(|| client.workspace_create(workspace.clone())).await?)?;
    require_accepted(settled(|| client.board_create(board.clone())).await?)?;
    require_accepted(settled(|| client.project_create(project.clone())).await?)?;
    let shown = client
        .project_show(message_board::ProjectShowRequest {
            project_id: project.project_id.clone(),
        })
        .await?;
    assert_eq!(shown.project.id, project.project_id);
    assert!(matches!(
        shown.state,
        message_board::ProjectStateView::Local(_)
    ));
    Ok(())
}
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn require_accepted(reply: WriteReply) -> TestResult {
    assert!(
        matches!(
            reply.result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ),
        "central creation did not complete: {reply:?}"
    );
    Ok(())
}
async fn settled<Call, Reply>(mut call: Call) -> TestResult<WriteReply>
where
    Call: FnMut() -> Reply,
    Reply: std::future::Future<Output = Result<WriteReply, collaboration_client::BoardClientError>>,
{
    Ok(tokio::time::timeout(BOUND, async {
        loop {
            let reply = call().await?;
            if !matches!(
                reply.result,
                RequestResult::PendingUnreachable | RequestResult::ProjectCreationPending { .. }
            ) {
                return Ok::<_, collaboration_client::BoardClientError>(reply);
            }
            tokio::task::yield_now().await;
        }
    })
    .await??)
}

fn project_requests() -> TestResult<(
    message_board::WorkspaceCreateRequest,
    message_board::BoardCreateRequest,
    message_board::ProjectCreateRequest,
)> {
    use message_board::*;
    let actor = Identity::Human {
        human_id: "enrollment-test-owner".to_owned().try_into()?,
    };
    let workspace_id = WorkspaceId::generate();
    let board_id = BoardId::generate();
    Ok((
        WorkspaceCreateRequest {
            request_id: RequestId::generate(),
            protocol_version: ProtocolVersion::CURRENT,
            workspace_id: workspace_id.clone(),
            name: "Enrollment workspace".to_owned().try_into()?,
            description: String::new().try_into()?,
            actor: actor.clone(),
            acting_for: None,
        },
        BoardCreateRequest {
            request_id: RequestId::generate(),
            protocol_version: ProtocolVersion::CURRENT,
            board_id: board_id.clone(),
            workspace_id,
            name: "Enrollment board".to_owned().try_into()?,
            description: String::new().try_into()?,
            kind: BoardKind::Area {},
            actor: actor.clone(),
            acting_for: None,
        },
        ProjectCreateRequest {
            request_id: RequestId::generate(),
            protocol_version: ProtocolVersion::CURRENT,
            project_id: ProjectId::generate(),
            board_id,
            title: "Enrollment project".to_owned().try_into()?,
            description: None,
            actor,
            acting_for: None,
        },
    ))
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn central_rejects_changed_machine_credential_without_edge_rewriting() -> TestResult {
    let fixture = ProducerFixture::start().await?;
    fixture
        .run(async |fixture| {
            let root = &fixture.root;
            let router_root = &fixture.router_root;
            let central = &fixture.central;
            let directory = codex_router_host::router_collaboration_directory(router_root);
            let output = fleet_cli::run(fleet_cli::enrollment(
                router_root,
                central,
                &future_expiry(),
            )?)
            .await?;
            fleet_cli::require_success(&output)?;
            let document = codex_router_host::read_fleet_enrollment_document(
                &router_root.join("fleet/enrollment.json"),
                router_root,
            )?;
            let store = codex_router_host::open_fleet_enrollment_store(router_root)?;
            let (_, key) = document.secret_keys()?;
            let wrong = "Bearer deliberately-wrong-producer-machine-credential";
            store.write_secret(
                &key,
                &codex_router_core::redaction::SecretString::new(wrong),
            )?;
            fleet_host::run_host_scenario(root.path(), async |host| {
                host.await_ready().await?;
                let status = central
                    .wait_for_credential_rejection()
                    .await
                    .map_err(io::Error::other)?;
                assert!(matches!(status, 401 | 403));
                assert_eq!(central.session_connections(), 0);
                assert!(
                    central
                        .session_authorization_hashes()
                        .map_err(io::Error::other)?
                        .contains(&format!("{:x}", Sha256::digest(wrong.as_bytes())))
                );
                let client =
                    CollaborationClient::connect(&directory, "denied-producer-test", "1").await?;
                let (workspace, _, _) = project_requests()?;
                assert_eq!(
                    client.workspace_create(workspace).await?.result,
                    RequestResult::PendingUnreachable
                );
                Ok::<_, Box<dyn Error>>(())
            })
            .await?;
            Ok::<_, Box<dyn Error>>(())
        })
        .await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn assert_produced_enrollment(
    router_root: &std::path::Path,
    central: &EnrollmentTestCentral,
) -> TestResult<(
    codex_router_host::FleetEnrollmentDocument,
    codex_router_core::redaction::SecretString,
)> {
    let directory = codex_router_host::router_collaboration_directory(router_root);
    let document = codex_router_host::read_fleet_enrollment_document(
        &router_root.join("fleet/enrollment.json"),
        router_root,
    )?;
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        router_root
            .join("fleet/enrollment.json")
            .metadata()?
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let store = codex_router_host::open_fleet_enrollment_store(router_root)?;
    let (_, credential_key) = document.secret_keys()?;
    let credential = store.read_secret(&credential_key)?;
    let raw = credential
        .expose_secret()
        .strip_prefix("Bearer ")
        .ok_or("missing Bearer prefix")?;
    assert_eq!(raw.len(), 64);
    assert!(raw.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let enrollments = central.enrollments().map_err(io::Error::other)?;
    assert_eq!(enrollments.len(), 1);
    let enrollment = enrollments
        .first()
        .ok_or("central observed no enrollment request")?;
    assert_eq!(
        enrollment.credential_hash,
        format!("{:x}", Sha256::digest(raw.as_bytes()))
    );
    assert_ne!(
        enrollment.credential_hash,
        format!(
            "{:x}",
            Sha256::digest(credential.expose_secret().as_bytes())
        )
    );
    assert_eq!(
        enrollment.peer,
        collaboration_runtime::load_installation_peer(&directory)?
    );
    assert_eq!(
        enrollment.installation.as_str(),
        String::from(collaboration_runtime::load_service_identity(&directory)?)
    );
    let before = fleet_cli::snapshot(router_root)?;
    for (_, bytes) in &before {
        for secret in [
            central.access_client_secret(),
            credential.expose_secret(),
            raw,
        ] {
            assert!(
                !bytes
                    .windows(secret.len())
                    .any(|window| window == secret.as_bytes()),
                "enrollment storage exposed a secret"
            );
        }
    }
    Ok((document, credential))
}
