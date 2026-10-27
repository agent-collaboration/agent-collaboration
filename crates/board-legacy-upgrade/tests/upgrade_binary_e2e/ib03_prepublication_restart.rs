use crate::e2e_support::{
    E2eResult, InterruptedImport, SourceSnapshot, import_status, install_prepublication_fault,
    remove_fault, run_cli,
};
use codex_router_test_support::scenario_finalization::run_scenario;
use collaboration_central_core::AdminImportPhase;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn actual_cli_resumes_staged_import_after_central_reopens() -> E2eResult {
    let fixture = InterruptedImport::start("ib03-owner", "ib03-machine").await?;
    let source = SourceSnapshot::capture(&fixture.fixture.source)?;
    run_scenario(
        fixture,
        async |fixture| {
            fixture.central.stop_owner().await?;
            install_prepublication_fault(fixture.central.root(), &fixture.copy).await?;
            let endpoint = fixture.central.reopen_owner().await?;
            let first_endpoint = endpoint.clone();
            let interrupted = run_cli(
                &fixture.fixture.source,
                &fixture.fixture.router_root,
                &fixture.fixture.report,
                Some(&endpoint),
                &fixture.owner,
                &fixture.machine,
            )
            .await?;
            assert_eq!(interrupted.status.code(), Some(2));
            let report = crate::e2e_support::read_report(&fixture.fixture.report)?;
            assert_eq!(
                report.get("kind").and_then(serde_json::Value::as_str),
                Some("refused")
            );
            assert_eq!(import_status(
                &fixture.central,
                &fixture.project,
                fixture.header.digest.clone(),
            ).await?, AdminImportPhase::Reserved);

            fixture.central.stop_owner().await?;
            let published_tables = crate::upload_support::read(
                fixture.central.root(), &fixture.copy,
                "SELECT CAST(count(*) AS TEXT) AS value FROM sqlite_schema WHERE type='table' AND name='posts'",
            ).await.map_err(crate::e2e_support::promote_error)?;
            let publication_rows = crate::upload_support::read(
                fixture.central.root(), &fixture.copy,
                "SELECT CAST(count(*) AS TEXT) AS value FROM _copy_summary",
            ).await.map_err(crate::e2e_support::promote_error)?;
            let staged_pages = crate::upload_support::read(
                fixture.central.root(), &fixture.copy,
                "SELECT payload AS value FROM _fleet_sync_seed_pages ORDER BY page",
            ).await.map_err(crate::e2e_support::promote_error)?;
            assert_eq!(published_tables, vec!["0"]);
            assert_eq!(publication_rows, vec!["0"]);
            assert!(!staged_pages.is_empty());

            remove_fault(fixture.central.root(), &fixture.copy).await?;
            let retry_endpoint = fixture.central.reopen_owner().await?;
            assert_eq!(retry_endpoint, first_endpoint);
            let retry = run_cli(
                &fixture.fixture.source,
                &fixture.fixture.router_root,
                &fixture.fixture.report,
                Some(&retry_endpoint),
                &fixture.owner,
                &fixture.machine,
            )
            .await?;
            assert_eq!(retry.status.code(), Some(0), "{}", String::from_utf8_lossy(&retry.stderr));
            let report = crate::e2e_support::read_report(&fixture.fixture.report)?;
            assert_eq!(
                report.get("kind").and_then(serde_json::Value::as_str),
                Some("uploaded")
            );
            let first_publication = report
                .get("publication")
                .and_then(|publication| publication.get("projects"))
                .and_then(serde_json::Value::as_array)
                .and_then(|projects| projects.first())
                .ok_or("publication result missing")?;
            assert_eq!(
                first_publication
                    .get("answer")
                    .and_then(|answer| answer.get("kind"))
                    .and_then(serde_json::Value::as_str),
                Some("imported")
            );
            source.assert_unchanged(&fixture.fixture.source)?;
            fixture.central.stop_owner().await?;
            let snapshot = crate::e2e_support::published_snapshot(
                fixture.central.root(), &fixture.project,
            ).await?;
            assert_eq!(snapshot.len(), 1);
            Ok(())
        },
        InterruptedImport::shutdown,
        || Ok(()),
    )
    .await
}
