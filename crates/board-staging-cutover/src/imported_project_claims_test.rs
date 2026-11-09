use super::*;
use crate::{
    ProjectPreparation,
    cutover_host_fixture::CutoverHostFixture,
    native_snapshot_fixture::{NativeFixture, native_fixture},
    offline_run_plan::PlannedProject,
};
use board_central_test_support::TestCentral;
use codex_router_host::{FleetAccess, FleetAccessReference, FleetEnrollmentDocument};
use codex_router_secret_store::{
    central_credentials::CentralCredential,
    fleet_secret_key::{FleetSecretKey, FleetSecretKind},
};
use fleet_admission::{FleetImportBundle, FleetImportRecord, ImportProvenance, ImportedBoardState};
use fleet_protocol::{PeerId, ReceiverId};
use message_board::ActorOrigin;
use std::os::unix::fs::PermissionsExt;
use url::Url;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; native fixture setup may fail."
)]
async fn sealed_source_without_its_original_backup_cannot_authorize_claim() -> TestResult {
    // Arrange a real sealed native snapshot whose original directory has not moved.
    let fixture = ClaimFixture::new(Url::parse("https://staging-cutover.invalid")?).await?;
    // Act on the same filesystem prerequisite used before any public Claim request.
    let observed = require_preserved_backup(&fixture.bundle);
    // Assert an immutable bundle cannot substitute for preserving the original source.
    assert!(matches!(observed, Err(PreservedBackupError::Unavailable)));
    assert!(fixture.source.path.is_file());
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertion follows joined real Central shutdown; setup may fail."
)]
async fn sealed_local_bundle_cannot_substitute_for_completed_central_publication() -> TestResult {
    // Arrange a live TestCentral and a real bundle, with no project import submitted.
    let root = tempfile::tempdir()?;
    let identity = CutoverHostFixture::new(None).map_err(boxed)?;
    let origin = ActorOrigin {
        installation: String::from(identity.service.clone()).try_into()?,
        machine: "unpublished Claim prerequisite".to_owned().try_into()?,
    };
    let central = TestCentral::start_with_peer(root.path(), &origin, identity.peer).await?;
    let observed = async {
        let fixture = ClaimFixture::new(Url::parse(central.base_url())?).await?;
        let owner = fixture.area.path().join("owner");
        owner_file(&fixture.plan.target, &owner)?;
        let publisher = StagingPublisher::from_owner_file(fixture.plan.target.clone(), &owner)?;
        // Act through the actual owner HTTP status route.
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(
            publisher.require_complete_import(&fixture.bundle).await,
        )
    }
    .await;
    let stopped = central.shutdown().await;
    // Assert after joining the real server even if the seeded wrong observer succeeds.
    assert!(observed?.is_err());
    stopped?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions protect untouched authentication; setup may fail."
)]
async fn another_active_central_origin_refuses_before_owner_authentication_is_consumed()
-> TestResult {
    // Arrange valid private metadata for a different target and an untouched Owner handoff.
    let fixture = ClaimFixture::new(Url::parse("https://staging-cutover.invalid")?).await?;
    let generation = uuid::Uuid::now_v7().to_string();
    let service = String::from(fixture.host.service.clone());
    let access = FleetSecretKey::new(FleetSecretKind::AccessClientSecret, &service, &generation)?
        .secret_key()?;
    let credential =
        FleetSecretKey::new(FleetSecretKind::MachineCredential, &service, &generation)?
            .secret_key()?;
    let document = FleetEnrollmentDocument {
        endpoint: "https://other-cutover.invalid/v1/session".to_owned(),
        access: FleetAccess::Cloudflare(FleetAccessReference {
            client_id: "fixture".to_owned(),
            client_secret_key: access.as_str().to_owned(),
        }),
        credential_key: credential.as_str().to_owned(),
        peer: fixture.host.peer,
        receiver: ReceiverId::new(uuid::Uuid::now_v7())?,
        relay: PeerId::new(uuid::Uuid::now_v7())?,
        expires_at: None,
    };
    let path = codex_router_host::router_fleet_enrollment_file(&fixture.host.root);
    std::fs::create_dir_all(path.parent().ok_or("enrollment parent missing")?)?;
    std::fs::write(&path, serde_json::to_vec(&document)?)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    let owner = fixture.area.path().join("owner");
    owner_file(&fixture.plan.target, &owner)?;
    // Act without a Host process: an invalid target must stop before auth or IPC.
    let observed = fixture
        .plan
        .claim_imported_projects(
            &fixture.host.home,
            OwnerAuthentication::OwnerCredentialFile {
                path: owner.clone(),
            },
        )
        .await;
    // Assert the declared origin is authoritative and the unrelated request consumes nothing.
    assert!(
        owner.is_file(),
        "wrong-target preflight consumed Owner authentication"
    );
    assert!(matches!(observed, Err(ClaimRunError::EnrollmentMismatch)));
    Ok(())
}

struct ClaimFixture {
    host: CutoverHostFixture,
    source: NativeFixture,
    area: tempfile::TempDir,
    plan: OfflineRunPlan,
    bundle: PreparedProjectBundle,
}

impl ClaimFixture {
    async fn new(target: Url) -> TestResult<Self> {
        let host = CutoverHostFixture::new(None).map_err(boxed)?;
        let source = native_fixture(&host).await.map_err(boxed)?;
        let workspace = source.scope.workspace.clone();
        let board = source.scope.board.clone().ok_or("source board absent")?;
        let time = source.record_time;
        let input = ProjectPreparation {
            run_id: uuid::Uuid::now_v7(),
            target: target.clone(),
            fleet: FleetImportBundle {
                provenance: ImportProvenance {
                    installation: String::from(host.service.clone()).try_into()?,
                    machine: "preserved Claim source".to_owned().try_into()?,
                },
                declared_owner: uuid::Uuid::now_v7().to_string().try_into()?,
                records: vec![
                    FleetImportRecord::Workspace {
                        id: workspace.clone(),
                        name: "workspace".to_owned().try_into()?,
                        description: "preserved".to_owned().try_into()?,
                        repositories: vec![],
                        time,
                    },
                    FleetImportRecord::Board {
                        id: board.clone(),
                        workspace,
                        name: "board".to_owned().try_into()?,
                        description: "preserved".to_owned().try_into()?,
                        state: ImportedBoardState::Active,
                        time,
                    },
                    FleetImportRecord::Directory {
                        project: source.selected.project.clone(),
                        board,
                        title: "project".to_owned().try_into()?,
                        time,
                    },
                ],
            },
            claim_request: RequestId::generate(),
            canary_request: RequestId::generate(),
        };
        let area = tempfile::tempdir()?;
        std::fs::set_permissions(area.path(), std::fs::Permissions::from_mode(0o700))?;
        let plan = OfflineRunPlan {
            admission: crate::offline_run_plan::RunAdmission::Isolated,
            run_id: input.run_id,
            router_root: host.root.clone(),
            service_id: host.service.clone(),
            peer_id: host.peer,
            target,
            bundle_parent: area.path().to_owned(),
            projects: vec![PlannedProject {
                selected: source.selected.clone(),
                fleet: input.fleet.clone(),
                claim_request: input.claim_request.clone(),
                canary_request: input.canary_request.clone(),
            }],
        };
        let bundle = {
            let mut offline = host.acquire()?;
            PreparedProjectBundle::prepare(
                &mut offline,
                source.selected.clone(),
                input,
                area.path(),
            )
            .await?
        };
        Ok(Self {
            host,
            source,
            area,
            plan,
            bundle,
        })
    }
}

fn owner_file(target: &Url, path: &Path) -> TestResult {
    CentralCredential::Owner {
        endpoint: target.as_str().to_owned(),
        assertion: collaboration_central_conformance::owner_assertion()?,
        expires_at: "2099-01-01T00:00:00Z".to_owned(),
    }
    .write_private_file(path)?;
    Ok(())
}

fn boxed(error: impl std::fmt::Display) -> Box<dyn std::error::Error + Send + Sync> {
    std::io::Error::other(error.to_string()).into()
}
