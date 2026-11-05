//! A pre-fix seal can include a newer file without having exported its content.
use super::{
    cutover_host_fixture::CutoverHostFixture,
    native_snapshot_fixture::native_fixture,
    native_source_selection::{newer_writer, source_files},
    offline_cli_fixture::{RunFixture, TestResult},
    recovery_rows::require,
    saved_bundle_resume::FailFirstPublication,
};
use board_central_test_support::TestCentral;
use board_staging_cutover::{PreparedProjectBundle, ProjectBundleManifest, ProjectPreparation};
use message_board::ActorOrigin;
use sha2::{Digest, Sha256};
use std::{os::unix::fs::PermissionsExt, sync::atomic::Ordering};

#[tokio::test]
async fn pre_fix_bundle_with_a_newer_file_at_seal_time_refuses_before_any_import() -> TestResult {
    // Arrange real old snapshot pages and a failed publication, retained across helper runs.
    let host = CutoverHostFixture::new(None).map_err(boxed)?;
    let older = native_fixture(&host).await.map_err(boxed)?;
    let origin = ActorOrigin {
        installation: String::from(host.service.clone()).try_into()?,
        machine: "pre-fix bundle resume regression".to_owned().try_into()?,
    };
    let target_area = tempfile::tempdir()?;
    let target = TestCentral::start_with_peer(target_area.path(), &origin, host.peer).await?;
    let proxy = FailFirstPublication::start(target.base_url()).await?;
    let old_area = tempfile::tempdir()?;
    let old_central = TestCentral::start_with_peer(old_area.path(), &origin, host.peer).await?;
    let result = async {
        let artifacts = tempfile::tempdir()?;
        let run = RunFixture::new(&host, &older, &proxy.endpoint, artifacts.path())?;
        let first = run.execute().await?;
        require(!first.status.success(), "first publication did not fail")?;
        require(
            proxy.requests.load(Ordering::SeqCst) == 1,
            "failed publication was forwarded",
        )?;
        let old_bundle = PreparedProjectBundle::open(&run.bundle)?;
        let newer = newer_writer(&host, &older, &old_central).await?;
        require(
            old_bundle.manifest().header.manifest.tables != newer.manifest.tables,
            "newer writer has no unique content",
        )?;

        // Simulate the pre-fix sealer: unchanged e1 pages, but the real entire
        // directory (including e2) is fingerprinted before the final seal.
        // A current e2 preparation supplies the owning fingerprint encoder;
        // it is fixture input only, never published or used as the refusal oracle.
        let plan: serde_json::Value = serde_json::from_slice(&std::fs::read(&run.plan)?)?;
        let input = ProjectPreparation {
            run_id: uuid::Uuid::now_v7(),
            target: proxy.endpoint.parse()?,
            fleet: serde_json::from_value(plan["projects"][0]["fleet"].clone())?,
            claim_request: run.claim.clone(),
            canary_request: run.canary.clone(),
        };
        let reference_area = tempfile::tempdir()?;
        std::fs::set_permissions(
            reference_area.path(),
            std::fs::Permissions::from_mode(0o700),
        )?;
        let mut offline = host.acquire().map_err(boxed)?;
        let current = PreparedProjectBundle::prepare(
            &mut offline,
            newer.selected,
            input,
            reference_area.path(),
        )
        .await?;
        drop(offline);
        let mut manifest = old_bundle.manifest().clone();
        manifest.source_fingerprint = current.manifest().source_fingerprint.clone();
        seal_pre_fix_manifest(&run.bundle, &manifest)?;
        let retained = PreparedProjectBundle::open(&run.bundle)?;
        let source = &retained.manifest().source_directory;
        require(
            retained.manifest().source_fingerprint.matches(source)?,
            "pre-fix fingerprint did not already include the newer file",
        )?;
        require(
            retained.manifest().source.selected.file_epoch.value() == 1
                && source.join("e2.sqlite").is_file(),
            "retained bundle does not select e1 beside real e2",
        )?;
        let before = source_files(source)?;
        let sealed = std::fs::read(run.bundle.join("manifest.json"))?;
        let seal = std::fs::read(run.bundle.join("seal.json"))?;
        let requests = proxy.requests.load(Ordering::SeqCst);
        run.renew_owner(&proxy.endpoint)?;

        // Act: the current compiled helper reopens exactly those immutable IDs and pages.
        let retried = run.execute().await?;

        // Assert no owner import or archive, rather than a late archive failure.
        require(
            proxy.requests.load(Ordering::SeqCst) == requests,
            "pre-fix saved bundle reached Fleet or project import before current-source admission",
        )?;
        require(!retried.status.success(), "obsolete saved bundle succeeded")?;
        let report: serde_json::Value = serde_json::from_slice(&retried.stdout)?;
        require(
            report["projects"][0]["reason"]
                .as_str()
                .is_some_and(|reason| reason.contains("not the Router's current file")),
            "obsolete selection refusal missing",
        )?;
        require(
            target.copy_status(&older.selected.project).await.is_err(),
            "stale project imported",
        )?;
        require(
            before == source_files(source)?
                && !retained.manifest().backup_directory.try_exists()?,
            "refusal changed or archived source content",
        )?;
        require(
            std::fs::read(run.bundle.join("manifest.json"))? == sealed
                && std::fs::read(run.bundle.join("seal.json"))? == seal,
            "retry rebuilt the saved bundle or changed immutable request IDs",
        )?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let proxy_stopped = proxy.shutdown().await;
    let old_stopped = old_central.shutdown().await;
    let target_stopped = target.shutdown().await;
    result?;
    proxy_stopped?;
    old_stopped?;
    target_stopped?;
    Ok(())
}

fn seal_pre_fix_manifest(
    directory: &std::path::Path,
    manifest: &ProjectBundleManifest,
) -> TestResult {
    let bytes = serde_json::to_vec(manifest)?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    for (name, contents) in [
        ("manifest.json", bytes),
        (
            "seal.json",
            serde_json::to_vec(&serde_json::json!({"manifestDigest":digest}))?,
        ),
    ] {
        let path = directory.join(name);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        std::fs::write(&path, contents)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400))?;
    }
    Ok(())
}
fn boxed(error: impl std::fmt::Display) -> Box<dyn std::error::Error + Send + Sync> {
    std::io::Error::other(error.to_string()).into()
}
