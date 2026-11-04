//! Ordinary public Claim installs e2; cold inspection reads that actual physical file.
use super::{
    cutover_host_fixture::CutoverHostFixture,
    host_entrypoints::HOST_BASE_PREFIX,
    host_process::OwnedHost,
    native_snapshot_fixture::native_fixture,
    offline_cli_fixture::{RunFixture, TestResult},
};
use board_central_test_support::TestCentral;
use board_staging_cutover::{
    NativeProjectSnapshot, NativeSourceError, PreparedProjectBundle, SelectedWriterFile,
};
use collaboration_client::{BoardClientError, CollaborationClient};
use message_board::{ActorOrigin, BoardFailureKind, ProtocolVersion};
use std::{os::unix::fs::PermissionsExt, time::Duration};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert normal installed e2 and exact cold content before a new write."
)]
async fn ordinary_host_claim_installs_e2_with_exact_cold_replicated_tables() -> TestResult {
    // Arrange existing native source, real central and an explicit short private runtime area.
    let base = tempfile::Builder::new()
        .prefix(HOST_BASE_PREFIX)
        .tempdir_in("/tmp")?;
    std::fs::set_permissions(base.path(), std::fs::Permissions::from_mode(0o700))?;
    let root = std::fs::canonicalize(base.path())?;
    let fixture =
        CutoverHostFixture::new_in(&root).map_err(|e| std::io::Error::other(e.to_string()))?;
    let source = native_fixture(&fixture)
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let origin = ActorOrigin {
        installation: String::from(fixture.service.clone()).try_into()?,
        machine: "compiled cutover source".to_owned().try_into()?,
    };
    let central_area = tempfile::tempdir()?;
    let central = TestCentral::start_with_peer(central_area.path(), &origin, fixture.peer).await?;
    let result = async {
        let artifacts = tempfile::tempdir()?;
        let run = RunFixture::new(&fixture, &source, central.base_url(), artifacts.path())?;
        let published = run.execute().await?;
        assert!(
            published.status.success(),
            "{}",
            String::from_utf8_lossy(&published.stderr)
        );
        let bundle = PreparedProjectBundle::open(&run.bundle)?;
        std::fs::create_dir_all(fixture.root.join("fleet"))?;
        let enrollment = central.write_host_session(&fixture.root)?;
        std::fs::rename(enrollment, fixture.root.join("fleet/enrollment.json"))?;
        let host =
            OwnedHost::start(&fixture.root, &bundle.manifest().header.declared_owner).await?;
        let claimed = async {
            host.await_ready().await?;
            let directory = codex_router_host::router_collaboration_directory(&fixture.root);
            let client =
                CollaborationClient::connect(&directory, "cutover-cold-readback", "1").await?;
            let request = board_sync_protocol::ProjectClaimRequest {
                request_id: run.claim.clone(),
                protocol_version: ProtocolVersion::CURRENT,
                project_id: source.selected.project.clone(),
            };
            // Act through the real Host's public route and machine-session installer.
            let answer = tokio::time::timeout(Duration::from_secs(20), async {
                loop {
                    match client.project_claim(request.clone()).await {
                        Ok(board_sync_protocol::ProjectClaimReply::Pending { .. }) => {
                            tokio::task::yield_now().await;
                        }
                        Err(BoardClientError::Rejected(error))
                            if error.kind == BoardFailureKind::BoardUnavailable =>
                        {
                            tokio::task::yield_now().await;
                        }
                        other => return other,
                    }
                }
            })
            .await??;
            assert!(matches!(
                answer,
                board_sync_protocol::ProjectClaimReply::Claimed { epoch, start, .. }
                    if epoch.value() == 2 && start == bundle.manifest().header.manifest.position
            ));
            drop(client);
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        }
        .await;
        let stopped = host.stop().await;
        claimed?;
        stopped?;
        // Assert exact installed content only after the actual Host and native child exit.
        let mut offline = fixture
            .acquire()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let mut installed = NativeProjectSnapshot::open(
            &mut offline,
            SelectedWriterFile {
                project: source.selected.project.clone(),
                peer: fixture.peer,
                file_epoch: fleet_sync_protocol::Epoch::new(2)?,
            },
        )
        .await?;
        let checked = async {
            assert_eq!(installed.preflight().selected.file_epoch.value(), 2);
            assert_eq!(installed.preflight().application_epoch.get(), 1);
            assert_eq!(installed.preflight().high_water, source.cursor);
            installed.verify_replicated_tables(&bundle.manifest().header.manifest)?;
            let mut altered = bundle.manifest().header.manifest.clone();
            altered
                .tables
                .first_mut()
                .ok_or("source tables absent")?
                .digest = fleet_sync_protocol::TableDigest::new("0".repeat(64))?;
            assert!(matches!(
                installed.verify_replicated_tables(&altered),
                Err(NativeSourceError::ReplicatedTablesMismatch)
            ));
            let page = installed
                .next_page()
                .await?
                .ok_or("installed snapshot page missing")?;
            assert_eq!(page.contents.database, source.manifest.database);
            assert!(
                bundle
                    .manifest()
                    .source_fingerprint
                    .matches(&bundle.manifest().backup_directory)?
            );
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        }
        .await;
        let closed = installed.close().await;
        checked?;
        closed?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let stopped = central.shutdown().await;
    if let Err(error) = result {
        let retained = fixture._area.keep();
        let _parent = base.keep();
        return Err(format!("{error}; owned fixture retained at {}", retained.display()).into());
    }
    stopped?;
    Ok(())
}
