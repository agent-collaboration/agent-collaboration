use super::*;
use crate::fleet_storage_fixture::{FleetStorageFixture, NOW, TestError};
use durable_steps::AtomicStore;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn machines_keep_independent_notice_positions_and_retention_floors() -> Result<(), TestError>
{
    // Arrange: both enrolled machines inhabit the same real SQLite store.
    let fixture = FleetStorageFixture::open().await?;
    let first = PeerId::new(durable_steps::Uuid::now_v7())?;
    let second = PeerId::new(durable_steps::Uuid::now_v7())?;
    fixture
        .enroll(first, "019f0000-0000-7000-8000-000000008101")
        .await?;
    fixture
        .enroll(second, "019f0000-0000-7000-8000-000000008102")
        .await?;
    let later = NOW.after(std::time::Duration::from_secs(60));
    let first_notice = BoardNotice::LeaseGranted {
        project: "019f0000-0000-7000-8000-000000008199"
            .to_owned()
            .try_into()?,
        epoch: fleet_sync_protocol::Epoch::new(12)?,
    };
    let second_notice = BoardNotice::LeaseSuperseded {
        project: "019f0000-0000-7000-8000-000000008199"
            .to_owned()
            .try_into()?,
        epoch: fleet_sync_protocol::Epoch::new(20)?,
    };
    let expected_first = first_notice.clone();
    let expected_second = second_notice.clone();

    fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                // Act: each starts at one; expiring A's prefix must leave B's newer row intact.
                append(
                    scope,
                    first,
                    durable_steps::Uuid::now_v7().to_string(),
                    first_notice,
                    NOW,
                )
                .await?;
                append(
                    scope,
                    second,
                    durable_steps::Uuid::now_v7().to_string(),
                    second_notice,
                    later,
                )
                .await?;
                Ok(())
            })
        })
        .await?;
    let fixture = fixture.reopen().await?;
    let (first_read, second_read, first_expired, second_retained) = fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                let first_read = read(scope, first, Some(NoticePosition::new(0)), later).await?;
                let second_read = read(scope, second, Some(NoticePosition::new(0)), later).await?;
                let expiry = SyncTime::from_unix_millis(
                    NOW.unix_millis()
                        .checked_add(RETENTION_MILLIS)
                        .ok_or_else(|| StoreError::backend("fixture expiry overflow"))?,
                );
                prune(scope, expiry).await?;
                Ok((
                    first_read,
                    second_read,
                    read(scope, first, Some(NoticePosition::new(0)), expiry).await?,
                    read(scope, second, Some(NoticePosition::new(0)), expiry).await?,
                ))
            })
        })
        .await?;
    let fixture = fixture.reopen().await?;
    let expired_after_reopen = fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                let expiry = SyncTime::from_unix_millis(
                    NOW.unix_millis()
                        .checked_add(RETENTION_MILLIS)
                        .ok_or_else(|| StoreError::backend("fixture expiry overflow"))?,
                );
                read(scope, first, Some(NoticePosition::new(0)), expiry).await
            })
        })
        .await?;
    assert_eq!(
        expired_after_reopen,
        NoticeRead::CursorExpired {
            floor: NoticePosition::new(1)
        }
    );
    // Assert: literal values come from the two submitted notices, not the query decoder.
    let NoticeRead::Page {
        notices: first_notices,
        ..
    } = first_read
    else {
        return Err("first machine lost its notice page".into());
    };
    let NoticeRead::Page {
        notices: second_notices,
        ..
    } = second_read
    else {
        return Err("second machine lost its notice page".into());
    };
    assert_eq!(
        first_notices,
        vec![PositionedBoardNotice {
            position: NoticePosition::new(1),
            kind: expected_first
        }]
    );
    assert_eq!(
        second_notices,
        vec![PositionedBoardNotice {
            position: NoticePosition::new(1),
            kind: expected_second
        }]
    );
    assert_eq!(
        first_expired,
        NoticeRead::CursorExpired {
            floor: NoticePosition::new(1)
        }
    );
    let NoticeRead::Page {
        notices: retained, ..
    } = second_retained
    else {
        return Err("expiring another machine removed the second stream".into());
    };
    assert_eq!(retained, second_notices);
    fixture.close().await?;
    Ok(())
}
