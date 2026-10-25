use crate::e2e_support::{
    E2eResult, InterruptedImport, SourceSnapshot, import_status, install_completion_fault,
    published_snapshot, remove_fault, run_cli,
};
use codex_router_test_support::scenario_finalization::run_scenario;
use collaboration_central_core::{AdminImportPhase, CopyDirectorySummary};

#[tokio::test]
async fn actual_cli_retries_published_copy_after_completion_receipt_failure() -> E2eResult {
    let fixture = InterruptedImport::start("ib04-owner", "ib04-machine").await?;
    let source = SourceSnapshot::capture(&fixture.fixture.source)?;
    run_scenario(
        fixture,
        async |fixture| {
            fixture.central.stop_owner().await?;
            install_completion_fault(fixture.central.root()).await?;
            let endpoint = fixture.central.reopen_owner().await?;
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
            assert_eq!(
                import_status(
                    &fixture.central,
                    &fixture.project,
                    fixture.header.digest.clone(),
                )
                .await?,
                AdminImportPhase::Published {
                    position: fixture.header.manifest.position,
                }
            );

            fixture.central.stop_owner().await?;
            let published_copy =
                published_snapshot(fixture.central.root(), &fixture.project).await?;
            assert_eq!(published_copy.len(), 1);
            let before_projection = crate::upload_support::projection(fixture.central.root(), 1)
                .await
                .map_err(crate::e2e_support::promote_error)?;
            let first_summary: CopyDirectorySummary = serde_json::from_str(
                before_projection
                    .first()
                    .ok_or("published Directory summary missing")?,
            )?;
            assert_eq!(first_summary.database.to_string(), fixture.project);

            retry_published_import(
                fixture,
                &source,
                &published_copy,
                before_projection
                    .first()
                    .ok_or("published Directory summary missing")?,
            )
            .await
        },
        InterruptedImport::shutdown,
        || Ok(()),
    )
    .await
}

async fn retry_published_import(
    fixture: &mut InterruptedImport,
    source: &SourceSnapshot,
    published_copy: &[String],
    first_directory_summary: &str,
) -> E2eResult {
    remove_fault(fixture.central.root(), "fleet").await?;
    let retry_endpoint = fixture.central.reopen_owner().await?;
    let retry = run_cli(
        &fixture.fixture.source,
        &fixture.fixture.router_root,
        &fixture.fixture.report,
        Some(&retry_endpoint),
        &fixture.owner,
        &fixture.machine,
    )
    .await?;
    assert_eq!(
        retry.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&retry.stderr)
    );
    let report = crate::e2e_support::read_report(&fixture.fixture.report)?;
    assert_eq!(
        report.get("kind").and_then(serde_json::Value::as_str),
        Some("uploaded")
    );
    assert_retry_answers(&report)?;
    source.assert_unchanged(&fixture.fixture.source)?;
    assert_eq!(
        import_status(
            &fixture.central,
            &fixture.project,
            fixture.header.digest.clone(),
        )
        .await?,
        AdminImportPhase::Complete {
            position: fixture.header.manifest.position,
        }
    );

    fixture.central.stop_owner().await?;
    let prepared = fixture.prepared.as_ref().ok_or("prepared source missing")?;
    let published_copies =
        crate::upload_support::published_copies(fixture.central.root(), prepared, None)
            .await
            .map_err(crate::e2e_support::promote_error)?;
    assert_eq!(published_copies.len(), 3);
    assert_eq!(
        published_copies.first().ok_or("first copy missing")?,
        published_copy
    );

    let projection = crate::upload_support::projection(fixture.central.root(), 3)
        .await
        .map_err(crate::e2e_support::promote_error)?;
    assert_eq!(
        projection.first().map(String::as_str),
        Some(first_directory_summary)
    );
    assert_eq!(
        directory_project_ids(&projection)?,
        vec![
            "019a0000-0000-7000-8000-000000000015",
            "019a0000-0000-7000-8000-000000000016",
            "019a0000-0000-7000-8000-000000000017",
        ]
    );
    Ok(())
}

fn assert_retry_answers(report: &serde_json::Value) -> E2eResult {
    let publications = report
        .get("publication")
        .and_then(|publication| publication.get("projects"))
        .and_then(serde_json::Value::as_array)
        .ok_or("publication results missing")?;
    let answers = publications
        .iter()
        .map(|publication| {
            let project = publication
                .get("project")
                .and_then(serde_json::Value::as_str)
                .ok_or("publication project missing")?;
            let answer = publication
                .get("answer")
                .and_then(|answer| answer.get("kind"))
                .and_then(serde_json::Value::as_str)
                .ok_or("publication answer missing")?;
            Ok((project.to_owned(), answer.to_owned()))
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error + Send + Sync>>>()?;
    assert_eq!(
        answers,
        vec![
            (
                "019a0000-0000-7000-8000-000000000015".to_owned(),
                "alreadyImported".to_owned()
            ),
            (
                "019a0000-0000-7000-8000-000000000016".to_owned(),
                "imported".to_owned()
            ),
            (
                "019a0000-0000-7000-8000-000000000017".to_owned(),
                "imported".to_owned()
            ),
        ]
    );
    Ok(())
}

fn directory_project_ids(projection: &[String]) -> E2eResult<Vec<String>> {
    projection
        .iter()
        .map(|value| {
            let summary: CopyDirectorySummary = serde_json::from_str(value)?;
            Ok(summary.database.to_string())
        })
        .collect()
}
