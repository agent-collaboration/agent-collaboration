//! Real authenticated HTTP publication of an unchanged native snapshot.
use crate::{cutover_host_fixture::CutoverHostFixture, native_snapshot_fixture::native_fixture};
use board_central_test_support::TestCentral;
use board_staging_cutover::{PreparedProjectBundle, ProjectPreparation, StagingPublisher};
use codex_router_secret_store::central_credentials::CentralCredential;
use collaboration_central_core::{
    AdminImportPhase, AdminProjectImportAnswer, AdminProjectImportRequest,
};
use fleet_admission::{FleetImportBundle, FleetImportRecord, ImportProvenance, ImportedBoardState};
use message_board::{ActorOrigin, RequestId};
use std::os::unix::fs::PermissionsExt;
use uuid::Uuid;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
fn boxed(error: impl std::fmt::Display) -> Box<dyn std::error::Error + Send + Sync> {
    std::io::Error::other(error.to_string()).into()
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Only a real complete Central import may make publication successful."
)]
async fn native_bundle_publication_completes_once_with_the_same_import_identity() -> TestResult {
    // Arrange a native source and real enrolled TestCentral with owner verification.
    let fixture = CutoverHostFixture::new(None).map_err(boxed)?;
    let source = native_fixture(&fixture).await.map_err(boxed)?;
    let origin = ActorOrigin {
        installation: String::from(fixture.service.clone()).try_into()?,
        machine: "cutover source".to_owned().try_into()?,
    };
    let central_root = tempfile::tempdir()?;
    let central = TestCentral::start_with_peer(central_root.path(), &origin, fixture.peer).await?;
    let result = async {
        let area = tempfile::tempdir()?;
        std::fs::set_permissions(area.path(), std::fs::Permissions::from_mode(0o700))?;
        let input = preparation(&origin, &source, central.base_url())?;
        let mut offline = fixture.acquire().map_err(boxed)?;
        let mut other_run_input = input.clone();
        other_run_input.run_id = Uuid::now_v7();
        let prepared = PreparedProjectBundle::prepare(
            &mut offline,
            source.selected.clone(),
            input,
            area.path(),
        )
        .await?;
        assert_eq!(prepared.manifest().source.manifest, source.manifest);
        assert_eq!(prepared.manifest().source.high_water, source.cursor);
        let credential_path = area.path().join("owner-credential");
        CentralCredential::Owner {
            endpoint: central.base_url().to_owned(),
            assertion: collaboration_central_conformance::owner_assertion()?,
            expires_at: "2099-01-01T00:00:00Z".to_owned(),
        }
        .write_private_file(&credential_path)?;
        let publisher = StagingPublisher::from_owner_file(
            url::Url::parse(central.base_url())?,
            &credential_path,
        )?;
        // Act through the new publisher over actual owner-authenticated TCP HTTP.
        let confirmed = publisher.publish(&mut offline, &prepared).await?;
        assert!(confirmed.matches_bundle(&prepared)?);
        // Assert the durable workflow header/phase, then replay and compare identity.
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(20))
            .build()?;
        let status = AdminProjectImportRequest::Status {
            digest: prepared.manifest().header.digest.clone(),
        };
        let route = format!(
            "{}/admin/v1/import/projects/{}",
            central.base_url(),
            prepared.manifest().source.selected.project.as_str()
        );
        let before: AdminProjectImportAnswer = http
            .post(&route)
            .header(
                "Cf-Access-Jwt-Assertion",
                collaboration_central_conformance::owner_assertion()?,
            )
            .json(&status)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let AdminProjectImportAnswer::Progress(before) = before else {
            return Err("published workflow absent".into());
        };
        assert_eq!(before.header, prepared.manifest().header);
        assert_eq!(
            before.phase,
            AdminImportPhase::Complete {
                position: prepared.manifest().header.manifest.position
            }
        );
        publisher.publish(&mut offline, &prepared).await?;
        let after: AdminProjectImportAnswer = http
            .post(route)
            .header(
                "Cf-Access-Jwt-Assertion",
                collaboration_central_conformance::owner_assertion()?,
            )
            .json(&status)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(after, AdminProjectImportAnswer::Progress(before));
        let other_run = PreparedProjectBundle::prepare(
            &mut offline,
            source.selected,
            other_run_input,
            area.path(),
        )
        .await?;
        assert!(!confirmed.matches_bundle(&other_run)?);
        assert!(!credential_path.exists());
        assert!(source.path.is_file());
        assert!(!prepared.manifest().backup_directory.exists());
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let stopped = central.shutdown().await;
    result?;
    stopped?;
    Ok(())
}

pub(super) fn preparation(
    origin: &ActorOrigin,
    source: &crate::native_snapshot_fixture::NativeFixture,
    endpoint: &str,
) -> TestResult<ProjectPreparation> {
    let declared_owner = Uuid::now_v7().to_string().try_into()?;
    let workspace = source.scope.workspace.clone();
    let board = source.scope.board.clone().ok_or("source board absent")?;
    let time = source.record_time;
    let input = ProjectPreparation {
        run_id: Uuid::now_v7(),
        target: url::Url::parse(endpoint)?,
        claim_request: RequestId::generate(),
        canary_request: RequestId::generate(),
        fleet: FleetImportBundle {
            provenance: ImportProvenance {
                installation: origin.installation.clone(),
                machine: origin.machine.clone(),
            },
            declared_owner,
            records: vec![
                FleetImportRecord::Workspace {
                    id: workspace.clone(),
                    name: "cutover workspace".to_owned().try_into()?,
                    description: "preserved workspace".to_owned().try_into()?,
                    repositories: vec![],
                    time,
                },
                FleetImportRecord::Board {
                    id: board.clone(),
                    workspace,
                    name: "cutover board".to_owned().try_into()?,
                    description: "preserved board".to_owned().try_into()?,
                    state: ImportedBoardState::Active,
                    time,
                },
                FleetImportRecord::Directory {
                    project: source.selected.project.clone(),
                    board,
                    title: "cutover project".to_owned().try_into()?,
                    time,
                },
            ],
        },
    };
    Ok(input)
}
