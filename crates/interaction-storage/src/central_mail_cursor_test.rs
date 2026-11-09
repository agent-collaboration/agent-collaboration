//! Clearing a learned prefix preserves actual SDK custody and terminal knowledge.
use super::*;
use fleet_mail_delivery::Inbox;
use fleet_protocol::{DeliveryStatus, StatusPublication, StatusVersion};

#[derive(Debug, Eq, PartialEq, sqlx::FromRow)]
struct CustodySnapshot {
    outbox: String,
    inbox: String,
}

async fn custody_snapshot(owner: &InteractionStore) -> CustodySnapshot {
    sqlx::query_as(
        "SELECT (SELECT json_group_array(json_object('item',item_id,'payload',payload,'digest',digest,'status',status_kind,'acceptance',status_acceptance,'version',status_version,'live',live,'bytes',payload_bytes,'settlement',settlement_charge_bytes,'slot',capacity_slot)) FROM (SELECT * FROM _fleet_sync_outbox_items ORDER BY item_id)) AS outbox,(SELECT json_group_array(json_object('item',item_id,'payload',payload,'digest',digest,'progress',progress,'outcome',outcome_kind,'live',live,'bytes',payload_bytes,'settlement',settlement_charge_bytes)) FROM (SELECT * FROM _fleet_sync_inbox_items ORDER BY item_id)) AS inbox",
    ).fetch_one(&mut *owner.writer.lock().await).await.unwrap()
}

#[tokio::test]
async fn clearing_central_status_cursor_is_repeatable_and_preserves_pending_inbox_outbox_and_delivered()
 {
    // Arrange real public SDK admissions, including a retained terminal sender row.
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("interaction.sqlite");
    let owner = Arc::new(InteractionStore::open(&path).await.unwrap());
    let store = NativeStore::new(owner.clone(), Arc::new(NoDispatchHost));
    let (pending, draft) = native_item_and_push();
    enqueue_and_project(&store, pending, draft).await.unwrap();
    let (delivered, draft) = native_item_and_push();
    let delivered_id = delivered.id();
    enqueue_and_project(&store, delivered, draft).await.unwrap();
    let (incoming, _) = native_item_and_push();
    store.transaction(move |scope| Box::pin(async move {
        let limits = DeliveryLimits::new::<BoardDeliveryTypes>(
            16 * 1024 * 1024, 20, 16 * 1024 * 1024,
            minimum_settlement_allowance::<BoardDeliveryTypes>().unwrap(),
        ).unwrap();
        Inbox::<_, BoardDeliveryTypes, _>::new(scope, incoming.recipient().peer, limits, &NoDeliveryConsumer)
            .enqueue_local(incoming, &NoDeliveryConsumer, NoDispatchHost.now()).await.map_err(fleet_mail_delivery::delivery_rollback)?;
        Outbox::<_, BoardDeliveryTypes, _>::new(scope, limits, &NoDeliveryConsumer)
            .publish_status(StatusPublication {
                item: delivered_id,
                status: DeliveryStatus::Delivered { acceptance: collaboration_mail_protocol::CarrierAcceptance::Started },
                version: StatusVersion::new(1), cancel: None,
            }, NoDispatchHost.now()).await.map_err(fleet_mail_delivery::delivery_rollback)?;
        sqlx::query("INSERT INTO _fleet_sync_outbox_status(singleton,cursor,created_at,updated_at) VALUES(1,100,1,1)")
            .execute(scope.executor()).await?;
        Ok(())
    })).await.unwrap();
    let before = custody_snapshot(&owner).await;
    assert!(before.outbox.contains("delivered"));
    assert_ne!(before.inbox, "[]");

    // Act twice through the file owner, then cold reopen that same database.
    owner.clear_central_mail_status_cursor().await.unwrap();
    owner.clear_central_mail_status_cursor().await.unwrap();
    assert_eq!(custody_snapshot(&owner).await, before);
    drop(store);
    Arc::try_unwrap(owner)
        .unwrap_or_else(|_| panic!("retained mail owner"))
        .close()
        .await
        .unwrap();
    let reopened = InteractionStore::open(&path).await.unwrap();
    assert_eq!(custody_snapshot(&reopened).await, before);
    let mut writer = reopened.writer.lock().await;
    let cursors: i64 = sqlx::query_scalar("SELECT count(*) FROM _fleet_sync_outbox_status")
        .fetch_one(&mut *writer)
        .await
        .unwrap();
    assert_eq!(cursors, 0);
    drop(writer);
    reopened.close().await.unwrap();
}
