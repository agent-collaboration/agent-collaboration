//! Central's calls to a workspace store: batches, reads, ids and migrations at open.

use crate::storage_support::{
    HeldClock, open_platform, read_store, single_integer, texts_in_column, workspace,
};
use central_storage::{
    BatchOutcome, RowExpectation, SafeInteger, SqlValue, StatementBatch, StoreCall, StoreFailure,
    StoreReply,
};
use collaboration_central_core::CentralPlatform;
use collaboration_central_core::storage_conformance::{
    FAILING_MIGRATION_FIRST_TABLE, alarm_from_earliest_widget_due, count_owners,
    failing_second_statement_migrator, insert_owner, insert_widget, list_applied_migrations,
    list_tables, list_widgets, widgets_migrator,
};

#[tokio::test]
async fn ids_minted_for_one_batch_keep_mint_order_while_the_clock_is_held() {
    let root = tempfile::tempdir().unwrap();
    let platform = open_platform(
        root.path(),
        HeldClock::at(1_700_000_000_000),
        widgets_migrator(),
    )
    .await;
    let minted: Vec<String> = (0..32)
        .map(|_| platform.ids().new_id().hyphenated().to_string())
        .collect();
    let mut statements = vec![insert_owner("owner-1", RowExpectation::Exactly(1))];
    statements.extend(minted.iter().enumerate().map(|(index, id)| {
        insert_widget(
            id,
            "owner-1",
            SqlValue::Text(format!("widget {index}")),
            SqlValue::Integer(SafeInteger::new(1).unwrap()),
            None,
        )
    }));

    let reply = platform
        .call_store(
            workspace(),
            StoreCall::Batch(StatementBatch {
                statements,
                // Workspace stores have no alarm; the update is ignored, not run.
                alarm: alarm_from_earliest_widget_due(),
            }),
        )
        .await
        .unwrap();

    assert!(matches!(
        reply,
        StoreReply::Batch(BatchOutcome::Applied { .. })
    ));
    // list_widgets orders by widget_id: identity order must equal mint order.
    assert_eq!(
        texts_in_column(&read_store(&platform, list_widgets()).await, 0),
        minted
    );
    for id in &minted {
        assert_eq!(uuid::Uuid::parse_str(id).unwrap().get_version_num(), 7);
    }
    platform.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_migration_failing_on_its_second_statement_leaves_no_partial_schema() {
    let root = tempfile::tempdir().unwrap();
    let clock = HeldClock::at(1_700_000_000_000);

    let first = open_platform(root.path(), clock.clone(), widgets_migrator()).await;
    let seeded = first
        .call_store(
            workspace(),
            StoreCall::Batch(StatementBatch {
                statements: vec![insert_owner("owner-1", RowExpectation::Exactly(1))],
                alarm: central_storage::AlarmUpdate::Unchanged,
            }),
        )
        .await
        .unwrap();
    assert!(matches!(
        seeded,
        StoreReply::Batch(BatchOutcome::Applied { .. })
    ));
    let applied_before = read_store(&first, list_applied_migrations()).await;
    first.shutdown().await.unwrap();

    let failing = open_platform(
        root.path(),
        clock.clone(),
        failing_second_statement_migrator(),
    )
    .await;
    assert_eq!(
        failing
            .call_store(workspace(), StoreCall::Read(count_owners()))
            .await,
        Err(StoreFailure::Unavailable)
    );
    failing.shutdown().await.unwrap();

    let reopened = open_platform(root.path(), clock, widgets_migrator()).await;
    let tables = texts_in_column(&read_store(&reopened, list_tables()).await, 0);
    assert!(
        !tables
            .iter()
            .any(|table| table == FAILING_MIGRATION_FIRST_TABLE),
        "{tables:?}"
    );
    // The whole history row, not only its version: description, installed_on, success,
    // checksum and execution_time must all be untouched by the failed migration.
    assert_eq!(
        applied_before.columns,
        [
            "version",
            "description",
            "installed_on",
            "success",
            "checksum",
            "execution_time"
        ]
    );
    assert_eq!(applied_before.rows.len(), 1);
    assert_eq!(
        applied_before.rows[0][0],
        SqlValue::Integer(SafeInteger::new(1).unwrap())
    );
    assert_eq!(
        read_store(&reopened, list_applied_migrations()).await,
        applied_before
    );
    assert_eq!(
        single_integer(&read_store(&reopened, count_owners()).await),
        1
    );
    reopened.shutdown().await.unwrap();
}
