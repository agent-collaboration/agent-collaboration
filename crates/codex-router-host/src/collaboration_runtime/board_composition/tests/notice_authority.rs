//! Disagreeing hints cross the real reaction owner; generic central remains the lease authority.
use super::*;
use fleet_sync_protocol::{
    DatabaseId, LeaseAnswer, LeaseRequest, LeaseState, LeaseStatus, OperationAnswer,
    OperationRequest, RenewAnswer, RenewRequest, RequestId,
};

async fn authoritative_lease(
    runtime: &Runtime,
) -> Result<LeaseStatus, Box<dyn std::error::Error + Send + Sync>> {
    // An independent session call observes the actual generic registry, not the
    // local writer or the hint. It does not apply this answer to the writer.
    let answer = runtime
        .requests
        .request(OperationRequest::Lease(LeaseRequest::Renew(RenewRequest {
            attempt: RequestId::new(durable_steps::Uuid::now_v7())?,
            database: DatabaseId::new(runtime.project.as_str().parse()?)?,
            epoch: runtime.epoch,
        })))
        .await
        .map_err(io::Error::other)?;
    let OperationAnswer::Lease(LeaseAnswer::Renew(RenewAnswer::Renewed {
        current,
        transfer_in_progress,
        ..
    })) = answer
    else {
        return Err(io::Error::other("generic registry did not renew the current writer").into());
    };
    assert!(
        transfer_in_progress.is_none(),
        "no authoritative release is requested"
    );
    assert_eq!(current.holder, runtime.central.peer);
    assert_eq!(current.epoch, runtime.epoch);
    assert_eq!(current.state, LeaseState::Held);
    assert!(current.remaining_ms > 0);
    Ok(current)
}

async fn check_hint_against_current_authority(
    hint: impl FnOnce(&Runtime) -> BoardNotice,
    expected_prompts: i64,
) -> TestResult {
    // Arrange: actual native writer and generic registry agree on the first grant.
    let runtime = Box::pin(Runtime::new()).await?;
    let before_authority = authoritative_lease(&runtime).await?;
    let writer = runtime.projects.ensure(&runtime.project).await?;
    let before = runtime.accepted_renewal_count()?;
    let mut reactions = runtime.reactions().await?;
    // Act: handle a hint that is deliberately not an authoritative lease answer.
    reactions
        .handle_pushed_notice(
            PositionedBoardNotice {
                position: NoticePosition::new(1),
                kind: hint(&runtime),
            },
            &runtime.projects,
            &Arc::downgrade(&runtime.admission),
            &Arc::downgrade(&runtime.fleet),
            &std::sync::Weak::new(),
            &runtime.requests,
        )
        .await?;
    // Assert: one actual native prompt, independently confirmed central authority,
    // the same writer object/epoch, and an active runtime rather than a stopped file.
    assert_eq!(runtime.accepted_renewal_count()?, before + expected_prompts);
    assert_eq!(runtime.progress().await?, NoticePosition::new(1));
    let after_authority = authoritative_lease(&runtime).await?;
    assert_eq!(after_authority.epoch, before_authority.epoch);
    let current = runtime.projects.ensure(&runtime.project).await?;
    assert!(
        Arc::ptr_eq(&writer, &current),
        "a hint cannot replace the writer"
    );
    assert!(matches!(current.store.writer_state().await?,
        fleet_sync_lease::WriterState::Writing {epoch} if epoch==after_authority.epoch));
    // activate is idempotent for Running and refuses Stopped. No test flag or
    // restarted worker can substitute for the original runtime staying active.
    current.activate(&runtime.project).await?;
    drop(current);
    drop(writer);
    runtime.close().await
}

#[tokio::test]
async fn granted_hint_with_a_different_epoch_cannot_replace_authoritative_writer() -> TestResult {
    let hinted_epoch = Epoch::new(73)?;
    check_hint_against_current_authority(
        |runtime| {
            assert_ne!(hinted_epoch, runtime.epoch);
            BoardNotice::LeaseGranted {
                project: runtime.project.clone(),
                epoch: hinted_epoch,
            }
        },
        0,
    )
    .await
}

#[tokio::test]
async fn superseded_hint_for_the_still_current_lease_does_not_stop_the_writer() -> TestResult {
    check_hint_against_current_authority(
        |runtime| BoardNotice::LeaseSuperseded {
            project: runtime.project.clone(),
            epoch: runtime.epoch,
        },
        1,
    )
    .await
}
