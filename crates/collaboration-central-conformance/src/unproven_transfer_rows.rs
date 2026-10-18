//! Destructive C7 cases run last: removing a leased copy deliberately makes
//! that installation's next Welcome fail. Keep its current session throughout.
use crate::{
    ConformanceClient, ConformanceResult, ControlRequest, StorageCorruption, holder_fixtures::*,
    session_scenarios::SessionFixture,
};
use collaboration_central_core::*;
use collaboration_replication_protocol::*;

pub async fn run_unproven_transfer_rows(client: &ConformanceClient) -> ConformanceResult<()> {
    let mut fixture = SessionFixture::create(client).await?;
    client
        .control(ControlRequest::Corrupt {
            address: ObjectAddress::Registry,
            mutation: StorageCorruption::ForgetRegistryFloor {
                project: fixture.project.clone(),
            },
        })
        .await?;
    remove_copy(&fixture).await?;
    equal(
        client
            .admin(AdminRequest::TakeoverPreview {
                project: fixture.project.clone(),
            })
            .await?,
        AdminResult::Refused(AdminRefusal::RegistryEpochUnproven),
    )?;
    assert_no_grant(&mut fixture, "preview").await?;

    // A new project on the SAME live session avoids a new Hello after the
    // preceding intentional corruption. The target may reconnect: it has no
    // lease for either destructive fixture.
    fixture.project = project()?;
    fixture.seed_project().await?;
    let takeover = fixture.takeover(fixture.preview().await?)?;
    remove_copy(&fixture).await?;
    equal(
        client.admin(takeover).await?,
        AdminResult::Refused(AdminRefusal::RegistryEpochUnproven),
    )?;
    assert_no_grant(&mut fixture, "submitted fence").await?;
    fixture.close().await
}

async fn remove_copy(fixture: &SessionFixture<'_>) -> ConformanceResult<()> {
    fixture
        .client
        .control(ControlRequest::Corrupt {
            address: ObjectAddress::Copy(fixture.project.clone()),
            mutation: StorageCorruption::RemoveCopyState,
        })
        .await?;
    assert_absent(fixture).await
}

async fn assert_absent(fixture: &SessionFixture<'_>) -> ConformanceResult<()> {
    equal(
        fixture
            .client
            .call(
                ObjectAddress::Copy(fixture.project.clone()),
                WireCall::Copy(CopyCall::CopyStatus {
                    project: fixture.project.clone(),
                }),
            )
            .await?,
        WireResult::Copy(CopyResult::CopyStatus(None)),
    )
}

async fn assert_no_grant(
    fixture: &mut SessionFixture<'_>,
    boundary: &str,
) -> ConformanceResult<()> {
    let renew = fixture.renew(RequestId::new(uuid::Uuid::now_v7().to_string())?, 1)?;
    check(
        matches!(fixture.old_socket.request(renew).await?,
            SessionAnswer::Renewed { current, transfer_in_progress: None, .. }
            if current.epoch == epoch(1)? && current.holder == fixture.old.installation),
        "unproven transfer retains the original epoch/holder without an active transfer",
    )?;
    let (target, projects) = fixture.new.connect(fixture.client).await?;
    fixture.new_socket = target;
    check(
        !projects
            .iter()
            .any(|state| state.project == fixture.project),
        "target Welcome has no lease for the unproven project",
    )?;
    assert_absent(fixture).await?;
    println!(
        "STAGING C7 {boundary}: project={} refused RegistryEpochUnproven; old epoch1 Renewed/no transfer, target Welcome/no lease, CopyStatus absent before/after",
        fixture.project.as_str()
    );
    Ok(())
}
