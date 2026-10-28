//! Exercise the prebuilt CLI against the actual Host claim port and real central.
use super::*;
use board_sync_protocol::{ProjectClaimRefusal, ProjectClaimReply};
use claim_fixture::ClaimFixture;
use std::os::unix::fs::PermissionsExt;

async fn invoke_claim(
    binary: &Path,
    api: &Path,
    project: &ProjectId,
    request: &RequestId,
) -> Result<ProjectClaimReply, Box<dyn std::error::Error + Send + Sync>> {
    let output = timeout(
        BOUND,
        tokio::process::Command::new(binary)
            .args([
                "project",
                "claim",
                project.as_str(),
                "--request-id",
                request.as_str(),
                "--service-directory",
            ])
            .arg(api)
            .arg("--json")
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    assert!(
        output.status.success(),
        "claim CLI failed ({}): stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let body: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    Ok(serde_json::from_value(
        body.pointer("/result/record")
            .ok_or("claim CLI result envelope")?
            .clone(),
    )?)
}

#[tokio::test]
#[ignore = "requires an explicitly prebuilt absolute BOARD_CLAIM_CLI_BINARY; run this journey explicitly"]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn claim_cli_installs_the_imported_prefix_and_reports_native_not_found() -> TestResult {
    let binary = std::path::PathBuf::from(std::env::var("BOARD_CLAIM_CLI_BINARY")?);
    assert!(
        binary.is_absolute() && binary.is_file(),
        "prebuild agent-collaboration and supply its absolute path"
    );
    let fixture = Box::pin(ClaimFixture::start()).await?;
    let project = ProjectId::generate();
    let (prefix, expected) = project_claim::imported_snapshot(
        &fixture.central,
        &fixture.directory.path().join("source.sqlite"),
        &project,
    )
    .await?;
    let directory = fixture.directory.path().join("api");
    std::fs::create_dir(&directory)?;
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
    let app = (collaboration_service::ApplicationFixture::new(
        "550e8400-e29b-41d4-a716-446655440000",
        "00000000-0000-4000-8000-000000000002",
    )?
    .with_project_admission(fixture.admission.clone())
    .with_project_claim(fixture.projects.clone()))
    .build();
    let api = collaboration_mcp::test_support::ServedCollaborationApi::start(&directory, app)?;
    let attempt = RequestId::generate();
    let claimed = timeout(BOUND, async {
        loop {
            let reply = invoke_claim(&binary, &directory, &project, &attempt).await?;
            if !matches!(reply, ProjectClaimReply::Pending { .. }) {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(reply);
            }
        }
    })
    .await??;
    assert!(
        matches!(claimed, ProjectClaimReply::Claimed { epoch, start, .. }
        if epoch.value() == 2 && start == prefix.position)
    );
    assert_eq!(
        invoke_claim(&binary, &directory, &project, &attempt).await?,
        claimed
    );
    let store = fixture
        .registry
        .existing_project(&project)
        .await?
        .ok_or("CLI returned before publication")?;
    let mut read = store.snapshot().await?;
    assert_eq!(read.record(&project.into()).await?, Some(expected));
    read.finish().await?;
    drop(store);
    assert_eq!(
        invoke_claim(
            &binary,
            &directory,
            &ProjectId::generate(),
            &RequestId::generate()
        )
        .await?,
        ProjectClaimReply::Refused(ProjectClaimRefusal::NotFound),
    );
    println!(
        "CLAIM_CLI_REAL_IMPORT prefix={} epoch=2 stable-replay native-not-found",
        serde_json::to_string(&prefix.position)?
    );
    api.stop().await?;
    fixture.shutdown().await
}
