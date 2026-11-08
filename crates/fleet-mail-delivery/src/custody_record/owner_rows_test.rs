#![allow(clippy::unwrap_used)]
use super::*;
use crate::custody_fixture::{Fixture, item_id, outbox_projection::enqueue};
use durable_steps::AtomicStore;
use sqlx_core::query::query;
use sqlx_turso::Turso;

async fn sql(fixture: &Fixture, statement: &'static str) {
    fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                query::<Turso>(statement)
                    .execute(scope.executor())
                    .await
                    .map_err(durable_steps::StoreError::backend)?;
                Ok(())
            })
        })
        .await
        .unwrap();
}

async fn owners(
    fixture: &Fixture,
) -> Result<(Vec<OutboxOwnerRow>, Vec<StatusOwedRow>), DeliveryError> {
    fixture
        .store
        .transaction(|scope| {
            Box::pin(async move {
                let outbox = outbox_owner_rows(scope).await;
                let owed = status_owed_rows(scope).await;
                Ok((outbox, owed))
            })
        })
        .await
        .map_err(|error| DeliveryError::Storage(error.to_string()))
        .and_then(|(outbox, owed)| Ok((outbox?, owed?)))
}

const OWED: &str = "INSERT INTO _fleet_sync_status_owed (id,submitter_kind,submitter_id,item_id,target_kind,status_route,version,status_kind) VALUES ('019f0000-0000-7000-8000-0000000000a1','machine','019f0000-0000-7000-8000-000000000001','01900000-0000-7000-8000-000000000001','peer','\"r\"',4,'sent')";

#[tokio::test]
async fn every_owner_row_is_read_as_stored_and_none_is_dropped() {
    // Arrange: one queued item, then a second row whose shape is recovered
    // (no delivery status), and one owed publication.
    let fixture = Fixture::new().await;
    enqueue(&fixture).await;
    sql(&fixture, OWED).await;

    // Act
    let (outbox, owed) = owners(&fixture).await.unwrap();

    // Assert: the enqueue's own stored status passes through unchanged.
    assert_eq!(
        outbox,
        vec![OutboxOwnerRow {
            item: item_id(1),
            record_kind: "item".to_owned(),
            custody: Some("queued".to_owned()),
        }]
    );
    assert_eq!(
        owed,
        vec![StatusOwedRow {
            id: Uuid::parse_str("019f0000-0000-7000-8000-0000000000a1").unwrap(),
            item: item_id(1),
            version: 4,
        }]
    );

    // A recovered row keeps its place with no custody value.
    sql(
        &fixture,
        "UPDATE _fleet_sync_outbox_items SET record_kind='recovered', status_kind=NULL",
    )
    .await;
    let (outbox, _) = owners(&fixture).await.unwrap();
    assert_eq!(
        outbox,
        vec![OutboxOwnerRow {
            item: item_id(1),
            record_kind: "recovered".to_owned(),
            custody: None,
        }]
    );
}

#[tokio::test]
async fn a_stored_value_that_does_not_decode_is_refused_with_its_table_and_field() {
    // Arrange
    let fixture = Fixture::new().await;
    enqueue(&fixture).await;
    sql(&fixture, OWED).await;
    sql(
        &fixture,
        "UPDATE _fleet_sync_status_owed SET version='four'",
    )
    .await;

    // Act
    let refused = owners(&fixture).await;

    // Assert
    assert!(
        matches!(
            &refused,
            Err(DeliveryError::CorruptRow {
                table: "_fleet_sync_status_owed",
                field: "version",
                ..
            })
        ),
        "{refused:?}"
    );
}
