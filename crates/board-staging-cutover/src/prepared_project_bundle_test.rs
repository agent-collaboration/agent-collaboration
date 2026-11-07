use super::*;
use crate::{cutover_host_fixture::CutoverHostFixture, native_snapshot_fixture::native_fixture};
use fleet_admission::{FleetImportRecord, ImportProvenance, ImportedBoardState};
use message_board::{RecordScope, RecordTime};
use std::os::unix::fs::PermissionsExt;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn preparation(
    host: &CutoverHostFixture,
    project: message_board::ProjectId,
    time: RecordTime,
    scope: &RecordScope,
) -> TestResult<ProjectPreparation> {
    let workspace = scope.workspace.clone();
    let board = scope.board.clone().ok_or("fixture board absent")?;
    Ok(ProjectPreparation {
        run_id: Uuid::now_v7(),
        target: Url::parse("https://staging-cutover.invalid")?,
        fleet: FleetImportBundle {
            provenance: ImportProvenance {
                installation: String::from(host.service.clone()).try_into()?,
                machine: "preserved source".to_owned().try_into()?,
            },
            declared_owner: Uuid::now_v7().to_string().try_into()?,
            records: vec![
                FleetImportRecord::Workspace {
                    id: workspace.clone(),
                    name: "source workspace".to_owned().try_into()?,
                    description: "preserved workspace".to_owned().try_into()?,
                    repositories: vec![],
                    time,
                },
                FleetImportRecord::Board {
                    id: board.clone(),
                    workspace,
                    name: "source board".to_owned().try_into()?,
                    description: "preserved board".to_owned().try_into()?,
                    state: ImportedBoardState::Active,
                    time,
                },
                FleetImportRecord::Directory {
                    project,
                    board,
                    title: "source project".to_owned().try_into()?,
                    time,
                },
            ],
        },
        claim_request: RequestId::generate(),
        canary_request: RequestId::generate(),
    })
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The sealed artifact must exist before publication."
)]
async fn native_bundle_is_sealed_before_it_can_be_reopened() -> TestResult {
    // Arrange a real source and a preserved graph, with fixed request identities.
    let fixture = CutoverHostFixture::new(None)?;
    let source = native_fixture(&fixture).await?;
    let input = preparation(
        &fixture,
        source.selected.project.clone(),
        source.record_time,
        &source.scope,
    )?;
    let area = tempfile::tempdir()?;
    std::fs::set_permissions(area.path(), std::fs::Permissions::from_mode(0o700))?;
    let mut offline = fixture.acquire()?;
    // Act through the bundle owner, under the existing Host singleton lock.
    let bundle =
        PreparedProjectBundle::prepare(&mut offline, source.selected, input, area.path()).await?;
    // Assert a committed, owner-private artifact exists and the source remains intact.
    assert!(bundle.directory().join("manifest.json").is_file());
    assert!(bundle.directory().join("seal.json").is_file());
    assert!(source.path.is_file());
    Ok(())
}

struct BundleFixture {
    host: CutoverHostFixture,
    source: crate::native_snapshot_fixture::NativeFixture,
    input: ProjectPreparation,
    area: tempfile::TempDir,
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Insufficient capacity must refuse before artifact creation."
)]
async fn insufficient_capacity_leaves_source_and_preparation_directory_intact() -> TestResult {
    // Arrange actual native content, a private empty artifact directory and zero available bytes.
    let fixture = BundleFixture::new().await?;
    let mut offline = fixture.host.acquire()?;
    let source_directory = fixture.source.path.parent().ok_or("source parent absent")?;
    let original_inode = std::os::unix::fs::MetadataExt::ino(&source_directory.metadata()?);
    // Act through the production preparation owner with only its filesystem probe injected.
    let refused = PreparedProjectBundle::prepare_with_space_probe(
        &mut offline,
        fixture.source.selected.clone(),
        fixture.input.clone(),
        fixture.area.path(),
        |_| Ok(0),
    )
    .await;
    // Assert no prepared or partial artifact and no source or backup move.
    assert!(
        matches!(refused, Err(ProjectBundleError::InsufficientSpace { available:0, required }) if required > 0)
    );
    assert_eq!(std::fs::read_dir(fixture.area.path())?.count(), 0);
    assert!(fixture.source.path.is_file());
    assert_eq!(
        std::os::unix::fs::MetadataExt::ino(&source_directory.metadata()?),
        original_inode
    );
    assert!(!fixture.host.root.join("staging-cutover-backups").exists());
    // The same frozen intent can still prepare its exact content with real capacity.
    let admitted = fixture.prepare(&mut offline).await?;
    assert_eq!(admitted.manifest().source.manifest, fixture.source.manifest);
    assert_eq!(
        admitted.manifest().claim_request,
        fixture.input.claim_request
    );
    Ok(())
}

impl BundleFixture {
    async fn new() -> TestResult<Self> {
        let host = CutoverHostFixture::new(None)?;
        let source = native_fixture(&host).await?;
        let input = preparation(
            &host,
            source.selected.project.clone(),
            source.record_time,
            &source.scope,
        )?;
        let area = tempfile::tempdir()?;
        std::fs::set_permissions(area.path(), std::fs::Permissions::from_mode(0o700))?;
        Ok(Self {
            host,
            source,
            input,
            area,
        })
    }

    async fn prepare(
        &self,
        offline: &mut OfflineHost,
    ) -> Result<PreparedProjectBundle, ProjectBundleError> {
        PreparedProjectBundle::prepare(
            offline,
            self.source.selected.clone(),
            self.input.clone(),
            self.area.path(),
        )
        .await
    }
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Reopening must keep frozen source facts and request identities."
)]
async fn reopened_bundle_retains_native_hashes_time_and_request_identities() -> TestResult {
    // Arrange actual native source facts, not an expected value computed by the bundle.
    let fixture = BundleFixture::new().await?;
    let mut offline = fixture.host.acquire()?;
    let bundle = fixture.prepare(&mut offline).await?;
    let directory = bundle.directory().to_owned();
    drop(bundle);
    // Act as a resumed process, without opening the native source again.
    let reopened = PreparedProjectBundle::open(&directory)?;
    let manifest = reopened.manifest();
    // Assert literal inputs and native manifest hashes survive the sealed artifact.
    assert_eq!(manifest.source.manifest, fixture.source.manifest);
    assert_eq!(manifest.source.high_water, fixture.source.cursor);
    assert_eq!(manifest.source.latest_record_at, fixture.source.record_time);
    assert_eq!(manifest.claim_request, fixture.input.claim_request);
    assert_eq!(manifest.canary_request, fixture.input.canary_request);
    assert_eq!(manifest.run_id, fixture.input.run_id);
    assert_eq!(manifest.target_origin, "https://staging-cutover.invalid");
    assert_eq!(manifest.fleet, fixture.input.fleet);
    assert_eq!(
        manifest.header.manifest.tables,
        fixture.source.manifest.tables
    );
    assert_eq!(manifest.header.manifest.position.epoch.value(), 1);
    assert_eq!(
        manifest.header.summary,
        BoardAnnotation::ProjectSummary {
            latest_record_at: fixture.source.record_time
        }
    );
    assert_eq!(directory.metadata()?.permissions().mode() & 0o777, 0o700);
    for index in 0..manifest.header.pages {
        let raw = reopened.page_bytes(index)?;
        let page = reopened.page(index)?;
        assert_eq!(
            raw,
            std::fs::read(directory.join("pages").join(page_name(index)))?
        );
        assert_eq!(page.position, manifest.header.manifest.position);
    }
    assert!(
        manifest
            .source_fingerprint
            .matches(fixture.source.path.parent().ok_or("source parent absent")?)?
    );
    assert!(!manifest.backup_directory.exists());
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A semantically unchanged but modified page is not the frozen request bytes."
)]
async fn page_byte_changes_are_rejected_even_when_json_content_is_unchanged() -> TestResult {
    // Arrange a sealed real source page and change only its trailing whitespace.
    let fixture = BundleFixture::new().await?;
    let mut offline = fixture.host.acquire()?;
    let bundle = fixture.prepare(&mut offline).await?;
    let page_path = bundle.directory().join("pages").join(page_name(0));
    let mut bytes = std::fs::read(&page_path)?;
    bytes.push(b' ');
    std::fs::set_permissions(&page_path, std::fs::Permissions::from_mode(0o600))?;
    std::fs::write(page_path, bytes)?;
    // Act and assert both an existing capability and a reopened one refuse.
    assert!(matches!(
        bundle.page_bytes(0),
        Err(ProjectBundleError::InvalidArtifact)
    ));
    assert!(matches!(
        PreparedProjectBundle::open(bundle.directory()),
        Err(ProjectBundleError::InvalidArtifact)
    ));
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Incomplete preparation cannot yield a publishable bundle."
)]
async fn a_missing_seal_refuses_reopen() -> TestResult {
    // Arrange an otherwise complete artifact with its commit seal absent.
    let fixture = BundleFixture::new().await?;
    let mut offline = fixture.host.acquire()?;
    let bundle = fixture.prepare(&mut offline).await?;
    std::fs::remove_file(bundle.directory().join("seal.json"))?;
    // Act and assert no incomplete capability is returned.
    assert!(PreparedProjectBundle::open(bundle.directory()).is_err());
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Private content must not be reopened from a public artifact directory."
)]
async fn a_public_bundle_directory_refuses_reopen() -> TestResult {
    // Arrange a sealed artifact whose directory is accidentally made public.
    let fixture = BundleFixture::new().await?;
    let mut offline = fixture.host.acquire()?;
    let bundle = fixture.prepare(&mut offline).await?;
    std::fs::set_permissions(bundle.directory(), std::fs::Permissions::from_mode(0o755))?;
    // Act and assert the public path is rejected before serving pages.
    assert!(matches!(
        PreparedProjectBundle::open(bundle.directory()),
        Err(ProjectBundleError::UnsafePath)
    ));
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "An existing run's immutable identities cannot be replaced."
)]
async fn preparing_the_same_run_again_preserves_the_original_bundle() -> TestResult {
    // Arrange a committed run, then offer a different canary under the same run ID.
    let mut fixture = BundleFixture::new().await?;
    let mut offline = fixture.host.acquire()?;
    let bundle = fixture.prepare(&mut offline).await?;
    let original = std::fs::read(bundle.directory().join("manifest.json"))?;
    fixture.input.canary_request = RequestId::generate();
    // Act through the same owning preparation API.
    let result = fixture.prepare(&mut offline).await;
    // Assert no replacement, including the exact original bytes.
    assert!(
        matches!(result, Err(ProjectBundleError::Io(ref error)) if error.kind() == std::io::ErrorKind::AlreadyExists)
    );
    assert_eq!(
        std::fs::read(bundle.directory().join("manifest.json"))?,
        original
    );
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The preserved graph is required before opening a source writer."
)]
async fn a_missing_graph_parent_refuses_before_source_open_or_artifact_creation() -> TestResult {
    // Arrange a graph missing its workspace, while a retained reader forbids writer opening.
    let mut fixture = BundleFixture::new().await?;
    fixture
        .input
        .fleet
        .records
        .retain(|record| !matches!(record, FleetImportRecord::Workspace { .. }));
    let retained = fleet_project_writer::RetainedFile::open_read_only(
        &fixture.source.path,
        board_fleet_sync::BoardProjectSchema::new()?,
    )
    .await?;
    let mut offline = fixture.host.acquire()?;
    // Act and assert the existing Fleet rule wins before native writer admission.
    assert!(matches!(
        fixture.prepare(&mut offline).await,
        Err(ProjectBundleError::InvalidGraph)
    ));
    assert_eq!(std::fs::read_dir(fixture.area.path())?.count(), 0);
    drop(retained);
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "All old epochs and sidecars belong to the source directory fingerprint."
)]
async fn adding_an_older_epoch_sibling_invalidates_the_source_fingerprint() -> TestResult {
    // Arrange a whole-directory fingerprint after native handles close.
    let fixture = BundleFixture::new().await?;
    let mut offline = fixture.host.acquire()?;
    let bundle = fixture.prepare(&mut offline).await?;
    let directory = fixture.source.path.parent().ok_or("source parent absent")?;
    assert!(bundle.manifest().source_fingerprint.matches(directory)?);
    // Act: add an older-file trap, not the selected e1 file.
    std::fs::write(
        directory.join("e0.sqlite"),
        b"old file must travel with the directory",
    )?;
    // Assert a later backup cannot accept the old fingerprint.
    assert!(!bundle.manifest().source_fingerprint.matches(directory)?);
    Ok(())
}
