//! Completion retires the Router attempt while retaining the SDK install receipt.
use super::*;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture errors propagate."
)]
async fn completed_real_snapshot_install_removes_its_attempt_before_claim_returns() -> TestResult {
    let fixture = Box::pin(claim_fixture::ClaimFixture::start()).await?;
    let project = ProjectId::generate();
    project_claim::imported_snapshot(
        &fixture.central,
        &fixture.directory.path().join("source.sqlite"),
        &project,
    )
    .await?;
    let result = fixture.claim(&project).await?;
    let board_sync_protocol::ProjectClaimReply::Claimed { epoch, .. } = result else {
        return Err("real snapshot claim did not complete".into());
    };
    let receipt = fixture
        .registry
        .database(&project)
        .await?
        .installed_snapshot()
        .await?
        .ok_or("native install receipt")?;
    let attempt =
        attempt_record(&attempt_key(&fixture.projects.directory, &project, epoch)).await?;
    fixture.shutdown().await?;
    assert!(receipt.verified && receipt.ready);
    assert!(
        attempt.is_none(),
        "completed install retained its attempt row: {attempt:?}"
    );
    Ok(())
}
