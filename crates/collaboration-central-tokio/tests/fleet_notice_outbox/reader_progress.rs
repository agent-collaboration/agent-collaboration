use super::{session_fixture, support::*};
use board_sync_protocol::{BoardReadPage, FleetFeedRead};
use std::sync::Arc;

#[tokio::test]
async fn a_lost_offered_page_keeps_the_applied_cursor_and_a_later_append_still_hints() -> TestResult
{
    let root = tempfile::tempdir()?;
    let delivery_root = tempfile::tempdir()?;
    let delivery = Arc::new(
        session_fixture::Fixture::open_for_installation(delivery_root.path(), installation()?)
            .await?,
    );
    let io = Io::new(delivery.clone());
    let fixture = Fixture::open(root.path(), io).await?;
    assert_accepted(&fixture.decide(&workspace_create()?).await?)?;
    // The page through 2 is offered but deliberately never applied by the caller.
    assert!(
        matches!(fixture.feed(0).await?,BoardReadPage::FleetFeed(FleetFeedRead::Page {through,head,..}) if through.get()==2 && head.get()==2)
    );
    let initial = fixture.reader_rows().await?;
    assert_eq!(initial.len(), 1);
    assert_eq!(initial[0].1, SERVICE);
    assert_eq!(initial[0].2, 0);
    let private_id = durable_steps::Uuid::parse_str(&initial[0].0)?;
    assert_eq!(private_id.get_version_num(), 7);
    assert_accepted(&fixture.decide(&workspace_update()?).await?)?;
    fixture.deliver().await?;
    assert_eq!(
        notice_rows(&delivery).await?,
        1,
        "offering an unapplied page suppressed a later hint"
    );
    assert!(
        matches!(fixture.feed(4).await?,BoardReadPage::FleetFeed(FleetFeedRead::Page {records,..}) if records.is_empty())
    );
    fixture.feed(2).await?;
    let progressed = fixture.reader_rows().await?;
    assert_eq!(
        progressed,
        vec![(initial[0].0.clone(), SERVICE.to_owned(), 4)],
        "reader cursor or private row identity regressed"
    );
    fixture.objects.close().await?;
    delivery.objects.close().await?;
    Ok(())
}

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
