use super::*;
use crate::fleet_storage_fixture::{FleetStorageFixture, NOW, TestError};
use board_sync_protocol::BoardNotice;
use durable_steps::AtomicStore;

const fn at(position: u64) -> NoticePosition {
    NoticePosition::new(position)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn acknowledgments_only_move_forward_and_never_past_the_issued_head() -> Result<(), TestError>
{
    // Arrange: three notices issued to one machine.
    let fixture = FleetStorageFixture::open().await?;
    let peer = PeerId::new(durable_steps::Uuid::now_v7())?;
    fixture
        .enroll(peer, "019f0000-0000-7000-8000-000000008131")
        .await?;
    let (two, one, beyond) = (at(2), at(1), at(99));

    // Act
    let observed = crate::event_owed::scoped(fixture.object.transaction(move |scope| {
        Box::pin(async move {
            for _ in 0..3 {
                crate::machine_notice_store::append(
                    scope,
                    peer,
                    durable_steps::Uuid::now_v7().to_string(),
                    BoardNotice::ProjectListChanged {},
                    NOW,
                )
                .await?;
            }
            let fresh = progress(scope, peer).await?;
            let after_two = record_applied(scope, peer, two, NOW).await?;
            let after_one = record_applied(scope, peer, one, NOW).await?;
            let after_beyond = record_applied(scope, peer, beyond, NOW).await?;
            Ok((fresh, after_two, after_one, after_beyond))
        })
    }))
    .await?;

    // Assert: none at first; 2 sticks; 1 is ignored; 99 stops at the head (3).
    let (fresh, after_two, after_one, after_beyond) = observed;
    let progress_at = |applied| Some(NoticeProgress { issued: 3, applied });
    assert_eq!(fresh, progress_at(None));
    assert!(fresh.is_some_and(NoticeProgress::behind));
    assert_eq!(after_two, progress_at(Some(2)));
    assert_eq!(after_one, progress_at(Some(2)));
    assert_eq!(after_beyond, progress_at(Some(3)));
    assert!(!after_beyond.is_some_and(NoticeProgress::behind));
    fixture.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_machine_never_issued_a_notice_has_no_progress_to_record() -> Result<(), TestError> {
    // Arrange
    let fixture = FleetStorageFixture::open().await?;
    let peer = PeerId::new(durable_steps::Uuid::now_v7())?;
    let through = at(1);

    // Act
    let recorded = fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move { record_applied(scope, peer, through, NOW).await })
        })
        .await?;

    // Assert
    assert_eq!(recorded, None);
    fixture.close().await?;
    Ok(())
}
