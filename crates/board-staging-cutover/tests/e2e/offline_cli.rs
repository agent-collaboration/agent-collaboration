//! The real compiled helper composes export, authenticated publication and archive.
use crate::{
    cutover_host_fixture::CutoverHostFixture,
    native_snapshot_fixture::native_fixture,
    offline_cli_fixture::{RunFixture, TestResult},
};
use board_central_test_support::TestCentral;
use board_staging_cutover::PreparedProjectBundle;
use codex_router_host::{HostCoordinationPaths, HostInstanceLock, InstanceAcquireError};
use message_board::ActorOrigin;
use std::os::unix::fs::MetadataExt;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert real process and physical custody results."
)]
async fn compiled_offline_run_publishes_archives_and_reconciles_the_same_bundle() -> TestResult {
    // Arrange an actual native source and authenticated TestCentral TCP owner route.
    let host = CutoverHostFixture::new(None).map_err(|e| std::io::Error::other(e.to_string()))?;
    let source = native_fixture(&host)
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let source_directory = source.path.parent().ok_or("source parent absent")?;
    std::fs::write(
        source_directory.join("older-epoch.keep"),
        b"whole directory custody",
    )?;
    let inode = source_directory.metadata()?.ino();
    let origin = ActorOrigin {
        installation: String::from(host.service.clone()).try_into()?,
        machine: "compiled cutover source".to_owned().try_into()?,
    };
    let central_area = tempfile::tempdir()?;
    let central = TestCentral::start_with_peer(central_area.path(), &origin, host.peer).await?;
    let result = async {
        let area = tempfile::tempdir()?;
        let run = RunFixture::new(&host, &source, central.base_url(), area.path())?;
        // Act through the compiled CLI rather than direct library composition.
        let first = run.execute().await?;
        assert!(
            first.status.success(),
            "{}",
            String::from_utf8_lossy(&first.stderr)
        );
        let first: serde_json::Value = serde_json::from_slice(&first.stdout)?;
        // Assert actual archive identity, unchanged snapshot and frozen public request IDs.
        assert_eq!(first["kind"], "offlineCompleted");
        assert_eq!(first["projects"][0]["kind"], "readyToClaim");
        let bundle = PreparedProjectBundle::open(&run.bundle)?;
        let manifest = bundle.manifest();
        assert_eq!(manifest.source.manifest, source.manifest);
        assert_eq!(manifest.source.high_water, source.cursor);
        assert_eq!(manifest.claim_request, run.claim);
        assert_eq!(manifest.canary_request, run.canary);
        assert_eq!(manifest.header.manifest.position.epoch.value(), 1);
        assert!(!source_directory.exists());
        assert_eq!(manifest.backup_directory.metadata()?.ino(), inode);
        assert_eq!(
            std::fs::read(manifest.backup_directory.join("older-epoch.keep"))?,
            b"whole directory custody"
        );
        assert!(
            manifest
                .source_fingerprint
                .matches(&manifest.backup_directory)?
        );
        assert!(!run.owner.exists());
        let sealed = std::fs::read(run.bundle.join("manifest.json"))?;
        // Rerun after a lost local result with fresh access but the original immutable plan.
        run.renew_owner(central.base_url())?;
        let second = run.execute().await?;
        assert!(
            second.status.success(),
            "{}",
            String::from_utf8_lossy(&second.stderr)
        );
        let second: serde_json::Value = serde_json::from_slice(&second.stdout)?;
        assert_eq!(
            second["projects"][0]["importId"],
            first["projects"][0]["importId"]
        );
        assert_eq!(
            second["projects"][0]["archive"]["kind"],
            "previouslyArchived"
        );
        assert_eq!(std::fs::read(run.bundle.join("manifest.json"))?, sealed);
        assert_eq!(manifest.backup_directory.metadata()?.ino(), inode);
        // A changed rerun is another intent, never permission to reinterpret the seal.
        let mut changed: serde_json::Value = serde_json::from_slice(&std::fs::read(&run.plan)?)?;
        changed["projects"][0]["claimRequest"] =
            serde_json::to_value(message_board::RequestId::generate())?;
        std::fs::write(&run.plan, serde_json::to_vec(&changed)?)?;
        run.renew_owner(central.base_url())?;
        let refused = run.execute().await?;
        assert!(
            !refused.status.success(),
            "changed claim identity must be refused"
        );
        let refused: serde_json::Value = serde_json::from_slice(&refused.stdout)?;
        assert_eq!(refused["projects"][0]["stage"], "preparation");
        assert_eq!(std::fs::read(run.bundle.join("manifest.json"))?, sealed);
        assert_eq!(manifest.backup_directory.metadata()?.ino(), inode);
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let stopped = central.shutdown().await;
    result?;
    stopped?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert exact Host authority while publication is causally held."
)]
async fn compiled_offline_helper_holds_the_source_host_lock_through_publication() -> TestResult {
    // Arrange actual source custody and an edge holding a committed real Fleet answer.
    let host = CutoverHostFixture::new(None).map_err(|e| std::io::Error::other(e.to_string()))?;
    let source = native_fixture(&host)
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let origin = ActorOrigin {
        installation: String::from(host.service.clone()).try_into()?,
        machine: "compiled cutover source".to_owned().try_into()?,
    };
    let central_area = tempfile::tempdir()?;
    let central = TestCentral::start_with_peer(central_area.path(), &origin, host.peer).await?;
    let mut edge = super::publication_hold::PublicationHold::start(central.base_url()).await?;
    let result = async {
        let area = tempfile::tempdir()?;
        let run = RunFixture::new(&host, &source, &edge.endpoint, area.path())?;
        let child = run
            .command()
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        // Act only once the actual helper has reached the owner HTTP publication boundary.
        edge.wait_for_committed_fleet().await?;
        let paths = HostCoordinationPaths::for_router_root(&host.root);
        let competing = HostInstanceLock::acquire(&paths);
        let held = matches!(competing, Err(InstanceAcquireError::AlreadyRunning));
        drop(competing);
        let source_still_present = source.path.is_file();
        edge.release();
        let output = child.wait_with_output().await?;
        // Assert the same source Host authority excludes restart until all effects finish.
        assert!(held, "source Host lock was released during publication");
        assert!(source_still_present);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let released = HostInstanceLock::acquire(&paths)?;
        drop(released);
        assert!(!source.path.exists());
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let edge_stopped = edge.shutdown().await;
    let central_stopped = central.shutdown().await;
    result?;
    edge_stopped?;
    central_stopped?;
    Ok(())
}
