use super::{session_fixture, support::*};
use board_sync_protocol::{BoardReadPage, FleetFeedRead};
use std::sync::Arc;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn future_and_expired_cursors_neither_register_a_reader_nor_advance_an_existing_one()
-> TestResult {
    let root = tempfile::tempdir()?;
    let delivery_root = tempfile::tempdir()?;
    let delivery = Arc::new(
        session_fixture::Fixture::open_for_installation(delivery_root.path(), installation()?)
            .await?,
    );
    let io = Io::new(delivery.clone());
    let fixture = Fixture::open(root.path(), io.clone()).await?;
    assert!(matches!(
        fixture
            .feed_from("550e8400-e29b-41d4-a716-446655440499", 1)
            .await?,
        BoardReadPage::FleetFeed(FleetFeedRead::CursorInvalid)
    ));
    assert!(fixture.reader_rows().await?.is_empty());
    assert_accepted(&fixture.decide(&workspace_create()?).await?)?;
    fixture.feed(1).await?;
    let before = fixture.reader_rows().await?;
    assert!(matches!(
        fixture.feed(3).await?,
        BoardReadPage::FleetFeed(FleetFeedRead::CursorInvalid)
    ));
    assert_eq!(fixture.reader_rows().await?, before);
    io.age_retention();
    assert_accepted(&fixture.decide(&workspace_update()?).await?)?;
    assert!(
        matches!(fixture.feed_from("550e8400-e29b-41d4-a716-446655440499",0).await?,BoardReadPage::FleetFeed(FleetFeedRead::CursorExpired {floor}) if floor.get()==2)
    );
    assert!(
        matches!(fixture.feed(0).await?,BoardReadPage::FleetFeed(FleetFeedRead::CursorExpired {floor}) if floor.get()==2)
    );
    assert_eq!(fixture.reader_rows().await?, before);
    fixture.objects.close().await?;
    delivery.objects.close().await?;
    Ok(())
}
