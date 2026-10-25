//! Actual preceding migrations retain owned history, SDK custody and push identity.
use super::*;
use sqlx::Connection;

fn historical_refusal(
    target: collaboration_protocol::SessionRef,
) -> crate::InteractionHistoryRecord {
    let requester: message_board::SessionRef = target.try_into().unwrap();
    crate::InteractionHistoryRecord::RefusedApproval {
        requester: requester.clone(),
        approver: message_board::Identity::Session { session: requester },
        refusal: crate::RefusedTypedApproval {
            request_id: "pre-read-context-history".into(),
            title: "Historical refusal".into(),
            description: None,
            subject: None,
            options: vec![crate::RefusedApprovalOption {
                option_id: "reject".into(),
                label: "Reject".into(),
                provider_kind: "fixture".into(),
            }],
            reason: "Retain this exact historical record".into(),
        },
    }
}

fn limits() -> DeliveryLimits {
    DeliveryLimits::new::<BoardDeliveryTypes>(
        16 * 1024 * 1024,
        20,
        16 * 1024 * 1024,
        20,
        16 * 1024 * 1024,
        minimum_settlement_allowance::<BoardDeliveryTypes>().unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn twelve_migration_composed_file_preserves_history_and_sdk_custody_through_upgrade_and_reopen()
 {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("interaction.sqlite");
    crate::schema_prefix_fixture::create_native_prefix(&path, 202610210006).await;
    let connection = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new()
            .filename(&path)
            .foreign_keys(true),
    )
    .await
    .unwrap();
    // Arrangement only: the old producer owns a genuine twelve-migration file.
    // Do not run today's initializer, or downgrade today's schema to fabricate it.
    let preceding = Arc::new(InteractionStore {
        writer: tokio::sync::Mutex::new(connection),
    });
    let adapter = NativeStore::new(preceding.clone(), Arc::new(NoDispatchHost));
    let (item, draft) = native_item_and_push();
    let reference = fleet_sync_protocol::DeliveryItemRef {
        item: item.id(),
        digest: item.digest().clone(),
    };
    let expected_item = serde_json::to_value(&item).unwrap();
    let push = draft.push_id.clone();
    let history = historical_refusal(draft.target.clone());
    let history_json = serde_json::to_string(&history).unwrap();
    let stamp = "2026-10-21T12:00:00.000000000Z";
    enqueue_and_project(&adapter, item, draft).await.unwrap();
    let expected_push =
        serde_json::to_value(preceding.get_push_record(&push).await.unwrap().unwrap()).unwrap();
    {
        let mut writer = preceding.writer.lock().await;
        sqlx::query("INSERT INTO typed_interaction_history(request_id,record_json,created_at) VALUES(?,?,?)")
            .bind(history.request_id()).bind(&history_json).bind(stamp).execute(&mut *writer).await.unwrap();
        sqlx::query("UPDATE interaction_history_revision SET revision=1 WHERE metadata_id=1")
            .execute(&mut *writer)
            .await
            .unwrap();
        let migration_count: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations")
            .fetch_one(&mut *writer)
            .await
            .unwrap();
        assert_eq!(
            migration_count, 12,
            "fixture was not the actual preceding schema"
        );
        let new_tables: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name='mail_read_contexts'",
        )
        .fetch_one(&mut *writer)
        .await
        .unwrap();
        assert_eq!(new_tables, 0);
    }
    drop(adapter);
    Arc::try_unwrap(preceding)
        .unwrap_or_else(|_| panic!("retained preceding owner"))
        .close()
        .await
        .unwrap();

    // Act: today's public opener must admit that exact file and run migration13.
    for _ in 0..2 {
        let owner = Arc::new(
            InteractionStore::open(&path)
                .await
                .expect("genuine twelve-step composed prefix must migrate"),
        );
        assert_eq!(
            serde_json::to_value(owner.get_push_record(&push).await.unwrap().unwrap()).unwrap(),
            expected_push
        );
        {
            let mut writer = owner.writer.lock().await;
            let row: (String, String) = sqlx::query_as(
                "SELECT record_json,created_at FROM typed_interaction_history WHERE request_id=?",
            )
            .bind(history.request_id())
            .fetch_one(&mut *writer)
            .await
            .unwrap();
            assert_eq!(row, (history_json.clone(), stamp.to_owned()));
            let migrations: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations")
                .fetch_one(&mut *writer)
                .await
                .unwrap();
            assert_eq!(migrations, 13);
            let contexts: i64 = sqlx::query_scalar("SELECT count(*) FROM mail_read_contexts")
                .fetch_one(&mut *writer)
                .await
                .unwrap();
            assert_eq!(
                contexts, 0,
                "upgrade invented admission metadata for historical mail"
            );
        }
        let adapter = NativeStore::new(owner.clone(), Arc::new(NoDispatchHost));
        let reference = reference.clone();
        let (status, envelope) = adapter
            .transaction(move |scope| {
                Box::pin(async move {
                    let mut outbox = Outbox::<_, BoardDeliveryTypes>::new(scope, limits());
                    let status = outbox
                        .status(reference.item)
                        .await
                        .map_err(fleet_sync_delivery::delivery_rollback)?
                        .state;
                    let envelope = outbox
                        .resolve_submit(&reference)
                        .await
                        .map_err(fleet_sync_delivery::delivery_rollback)?
                        .item;
                    Ok((status, envelope))
                })
            })
            .await
            .unwrap();
        assert!(matches!(
            status,
            fleet_sync_delivery::OutboxStatusState::Known {
                status: fleet_sync_protocol::DeliveryStatus::Queued,
                ..
            }
        ));
        let fleet_sync_protocol::DeliveryEnvelope::Item(retained) = envelope else {
            panic!("original custody changed to a control");
        };
        assert_eq!(
            serde_json::to_value(retained).unwrap(),
            expected_item,
            "native custody body or identity changed across migration/reopen"
        );
        drop(adapter);
        Arc::try_unwrap(owner)
            .unwrap_or_else(|_| panic!("retained upgraded owner"))
            .close()
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn partial_unknown_newer_or_malformed_read_context_schema_is_refused_without_rewriting() {
    for statement in [
        "DROP TABLE mail_read_contexts",
        "DELETE FROM _sqlx_migrations WHERE version=202610220001",
        "DROP INDEX mail_read_context_conversation",
        "ALTER TABLE mail_read_contexts ADD COLUMN unexpected TEXT",
        "UPDATE _sqlx_migrations SET checksum=x'00' WHERE version=202610220001",
        "UPDATE _sqlx_migrations SET version=202610220002 WHERE version=202610220001",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("interaction.sqlite");
        crate::schema_prefix_fixture::create_native_prefix(&path, 202610220001).await;
        let mut writer = sqlx_turso::TursoConnection::connect_with(
            &sqlx_turso::TursoConnectOptions::new().filename(&path),
        )
        .await
        .unwrap();
        sqlx::raw_sql(statement).execute(&mut writer).await.unwrap();
        writer.close().await.unwrap();
        let before = tokio::fs::read(&path).await.unwrap();
        assert!(
            matches!(
                InteractionStore::open(&path).await,
                Err(crate::StorageError::InvalidSchema)
            ),
            "accepted malformed schema: {statement}"
        );
        assert_eq!(
            tokio::fs::read(&path).await.unwrap(),
            before,
            "refusal rewrote the file: {statement}"
        );
    }
}
