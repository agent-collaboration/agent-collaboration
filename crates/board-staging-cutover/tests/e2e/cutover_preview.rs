//! The compiled dry run reads the declared inventory without taking Host authority.
use super::{
    cutover_host_fixture::CutoverHostFixture,
    native_snapshot_fixture::native_fixture,
    offline_cli_fixture::{RunFixture, TestResult},
};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert the compiled dry-run contract."
)]
async fn compiled_dry_run_prints_project_paths_and_target_without_authentication() -> TestResult {
    // Arrange a real native source; the target need not be reachable for a preview.
    let host =
        CutoverHostFixture::new(None).map_err(|error| std::io::Error::other(error.to_string()))?;
    let source = native_fixture(&host)
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let area = tempfile::tempdir()?;
    let run = RunFixture::new(&host, &source, "https://central.example/", area.path())?;
    let paths = codex_router_host::HostCoordinationPaths::for_router_root(&host.root);
    let peer_before = std::fs::read(host.identity_path("installation-peer.json"))?;
    // Act through the real compiled parser and signing runner.
    let output = tokio::process::Command::new(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/cargo_debug_signing_runner.sh"),
    )
    .arg(env!("CARGO_BIN_EXE_codex-router-board-staging-cutover"))
    .args(["offline", "--plan"])
    .arg(&run.plan)
    .arg("--dry-run")
    .output()
    .await?;
    // Assert the inventory, explicit endpoint and lack of any authority/artifact effects.
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let preview: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(preview["kind"], "cutoverPlan");
    assert_eq!(preview["target"], "https://central.example/");
    assert_eq!(
        preview["projects"][0]["project"],
        source.selected.project.as_str()
    );
    assert_eq!(
        preview["projects"][0]["epoch"],
        source.selected.file_epoch.value()
    );
    assert_eq!(
        preview["projects"][0]["bundle"],
        run.bundle.to_string_lossy().as_ref()
    );
    assert!(
        preview["projects"][0]["backup"]
            .as_str()
            .is_some_and(|path| path.contains("staging-cutover-backups"))
    );
    assert_eq!(
        std::fs::read(host.identity_path("installation-peer.json"))?,
        peer_before
    );
    assert!(!paths.instance_lock().exists());
    assert!(!run.bundle.exists());
    assert!(run.owner.exists());
    assert!(source.path.exists());
    Ok(())
}
