//! The first project epoch survives replay and rolls back with actual SDK admission.
use super::*;
use crate::{
    StorageError, read_mail_submission_epoch_in_scope, record_mail_submission_epoch_in_scope,
};
use board_sync_protocol::BoardDeliveryBody;
use collaboration_mail_protocol::{
    MailItemId, MailSender, NotificationBody, NotificationSource, RecordPointer,
};
use message_board::{
    EpochNumber, FeedCursor, NotifyIntentId, ProjectId, RecordId, RecordKind, RecordRef,
};
use sqlx::Connection;

fn first_time() -> chrono::DateTime<chrono::Utc> {
    "2026-10-20T12:00:00Z".parse().unwrap()
}

fn project_item(project: ProjectId) -> DeliveryItem<BoardDeliveryTypes> {
    let (item, _) = native_item_and_push();
    DeliveryItem::new(
        item.id(),
        MailSender::Project {
            project: project.clone(),
        },
        item.recipient().clone(),
        None,
        BoardDeliveryBody::Notification(NotificationBody {
            excerpt: "frozen project notification".to_owned().try_into().unwrap(),
            pointer: RecordPointer {
                project,
                subject: RecordRef {
                    kind: RecordKind::Post,
                    id: RecordId::generate(),
                },
                position: FeedCursor {
                    epoch: 2.try_into().unwrap(),
                    position: 1.try_into().unwrap(),
                },
            },
            source: NotificationSource::Intent {
                intent: NotifyIntentId::generate(),
            },
        }),
    )
    .unwrap()
}

async fn admit(
    scope: &mut durable_steps::turso_store::TursoScope<'_, NoDispatchHost>,
    item: DeliveryItem<BoardDeliveryTypes>,
    project: &ProjectId,
) -> Result<(), StoreError> {
    let id: MailItemId = item.id().as_uuid().to_string().try_into().unwrap();
    let limits = DeliveryLimits::new::<BoardDeliveryTypes>(
        16 * 1024 * 1024,
        20,
        16 * 1024 * 1024,
        20,
        16 * 1024 * 1024,
        minimum_settlement_allowance::<BoardDeliveryTypes>().map_err(StoreError::backend)?,
    )
    .map_err(StoreError::backend)?;
    let (_, operation) = Outbox::<_, BoardDeliveryTypes>::new(scope, limits)
        .enqueue_and_admit::<NativeSubmit>(item, PeerId::new(uuid::Uuid::now_v7()).unwrap())
        .await
        .map_err(fleet_sync_delivery::delivery_rollback)?;
    if operation.is_none() {
        return Err(StoreError::backend(
            "fresh fixture submission was not admitted",
        ));
    }
    record_mail_submission_epoch_in_scope(scope, &id, project, 2.try_into().unwrap(), first_time())
        .await
        .map_err(StoreError::backend)
}

#[tokio::test]
async fn the_first_epoch_is_insert_only_and_reopens_with_real_custody() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("interaction.sqlite");
    let owner = Arc::new(InteractionStore::open(&path).await.unwrap());
    let store = NativeStore::new(owner.clone(), Arc::new(NoDispatchHost));
    let project = ProjectId::generate();
    let item = project_item(project.clone());
    let id: MailItemId = item.id().as_uuid().to_string().try_into().unwrap();
    let admitted_project = project.clone();
    store
        .transaction(move |scope| {
            Box::pin(async move { admit(scope, item, &admitted_project).await })
        })
        .await
        .unwrap();
    let replay_id = id.clone();
    let replay_project = project.clone();
    store
        .transaction(move |scope| {
            Box::pin(async move {
                record_mail_submission_epoch_in_scope(
                    scope,
                    &replay_id,
                    &replay_project,
                    2.try_into().unwrap(),
                    first_time() + chrono::Duration::days(1),
                )
                .await
                .map_err(StoreError::backend)?;
                assert!(matches!(
                    record_mail_submission_epoch_in_scope(
                        scope,
                        &replay_id,
                        &replay_project,
                        3.try_into().unwrap(),
                        first_time()
                    )
                    .await,
                    Err(StorageError::InvalidStoredRecord)
                ));
                assert!(matches!(
                    record_mail_submission_epoch_in_scope(
                        scope,
                        &replay_id,
                        &ProjectId::generate(),
                        2.try_into().unwrap(),
                        first_time()
                    )
                    .await,
                    Err(StorageError::InvalidStoredRecord)
                ));
                Ok(())
            })
        })
        .await
        .unwrap();
    drop(store);
    Arc::try_unwrap(owner)
        .unwrap_or_else(|_| panic!("owner retained"))
        .close()
        .await
        .unwrap();
    let owner = InteractionStore::open(&path).await.unwrap();
    let saved = owner.mail_submission_epoch(&id).await.unwrap().unwrap();
    assert_eq!(saved.item_id, id);
    assert_eq!(saved.project, project);
    assert_eq!(saved.epoch, EpochNumber::try_from(2).unwrap());
    assert_eq!(saved.recorded_at, first_time());
    owner.close().await.unwrap();
}

#[tokio::test]
async fn an_aborted_admission_keeps_neither_custody_nor_the_epoch() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("interaction.sqlite");
    let owner = Arc::new(InteractionStore::open(&path).await.unwrap());
    let store = NativeStore::new(owner.clone(), Arc::new(NoDispatchHost));
    let project = ProjectId::generate();
    let item = project_item(project.clone());
    let sdk_id = item.id();
    let id: MailItemId = sdk_id.as_uuid().to_string().try_into().unwrap();
    let failure = store
        .transaction(move |scope| {
            Box::pin(async move {
                admit(scope, item, &project).await?;
                Err::<(), _>(StoreError::backend(
                    "rollback after SDK admission and epoch",
                ))
            })
        })
        .await
        .unwrap_err();
    assert!(
        failure
            .to_string()
            .contains("rollback after SDK admission and epoch")
    );
    drop(store);
    Arc::try_unwrap(owner)
        .unwrap_or_else(|_| panic!("owner retained"))
        .close()
        .await
        .unwrap();
    let owner = Arc::new(InteractionStore::open(&path).await.unwrap());
    assert!(owner.mail_submission_epoch(&id).await.unwrap().is_none());
    let store = NativeStore::new(owner.clone(), Arc::new(NoDispatchHost));
    store
        .transaction(move |scope| {
            Box::pin(async move {
                let limits = DeliveryLimits::new::<BoardDeliveryTypes>(
                    16 * 1024 * 1024,
                    20,
                    16 * 1024 * 1024,
                    20,
                    16 * 1024 * 1024,
                    minimum_settlement_allowance::<BoardDeliveryTypes>()
                        .map_err(StoreError::backend)?,
                )
                .map_err(StoreError::backend)?;
                let status = Outbox::<_, BoardDeliveryTypes>::new(scope, limits)
                    .status(sdk_id)
                    .await
                    .map_err(StoreError::backend)?;
                assert!(matches!(
                    status.state,
                    fleet_sync_delivery::OutboxStatusState::Unknown
                ));
                assert!(
                    read_mail_submission_epoch_in_scope(scope, &id)
                        .await
                        .map_err(StoreError::backend)?
                        .is_none()
                );
                Ok(())
            })
        })
        .await
        .unwrap();
    drop(store);
    Arc::try_unwrap(owner)
        .unwrap_or_else(|_| panic!("owner retained"))
        .close()
        .await
        .unwrap();
}

#[tokio::test]
async fn corrupt_epoch_rows_are_refused_after_cold_reopen() {
    for (column, value) in [
        ("project", "not-a-project"),
        ("epoch", "0"),
        ("epoch", "-1"),
        ("recorded_at", "not-a-clock"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("interaction.sqlite");
        let owner = InteractionStore::open(&path).await.unwrap();
        let id = MailItemId::generate();
        let project = ProjectId::generate();
        sqlx::query(
            "INSERT INTO mail_submission_epochs VALUES (?,?,2,'2026-10-20T12:00:00.000000000Z')",
        )
        .bind(id.as_str())
        .bind(project.as_str())
        .execute(&mut *owner.writer.lock().await)
        .await
        .unwrap();
        let update = match column {
            "project" => "UPDATE mail_submission_epochs SET project=?",
            "epoch" => "UPDATE mail_submission_epochs SET epoch=?",
            "recorded_at" => "UPDATE mail_submission_epochs SET recorded_at=?",
            _ => panic!("unlisted column"),
        };
        sqlx::query(update)
            .bind(value)
            .execute(&mut *owner.writer.lock().await)
            .await
            .unwrap();
        owner.close().await.unwrap();
        let owner = InteractionStore::open(&path).await.unwrap();
        let lookup = owner.mail_submission_epoch(&id).await;
        assert!(
            matches!(lookup, Err(StorageError::InvalidStoredRecord)),
            "{column}"
        );
        owner.close().await.unwrap();
    }
}

#[tokio::test]
async fn the_previous_schema_upgrades_and_partial_epoch_migrations_refuse_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("interaction.sqlite");
    crate::schema_prefix_fixture::create_native_prefix(&path, 202610190004).await;
    let owner = InteractionStore::open(&path).await.unwrap();
    assert!(
        owner
            .mail_submission_epoch(&MailItemId::generate())
            .await
            .unwrap()
            .is_none()
    );
    owner.close().await.unwrap();

    for statement in [
        "DROP TABLE mail_submission_epochs",
        "DELETE FROM _sqlx_migrations WHERE version=202610200001",
        "ALTER TABLE mail_submission_epochs ADD COLUMN unexpected TEXT",
        "DROP TABLE installation_peer_directory; DELETE FROM _sqlx_migrations WHERE version=202610190004",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("interaction.sqlite");
        crate::schema_prefix_fixture::create_native_prefix(&path, 202610200001).await;
        let mut connection = sqlx_turso::TursoConnection::connect_with(
            &sqlx_turso::TursoConnectOptions::new().filename(&path),
        )
        .await
        .unwrap();
        sqlx::raw_sql(statement)
            .execute(&mut connection)
            .await
            .unwrap();
        connection.close().await.unwrap();
        let bytes = tokio::fs::read(&path).await.unwrap();
        assert!(
            matches!(
                InteractionStore::open(&path).await,
                Err(StorageError::InvalidSchema)
            ),
            "{statement}"
        );
        assert_eq!(tokio::fs::read(&path).await.unwrap(), bytes, "{statement}");
    }
}
