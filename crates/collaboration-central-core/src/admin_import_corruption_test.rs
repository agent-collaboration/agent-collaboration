//! Malformed physical import state cannot become continuation or publication authority.
use super::{
    admin_import_columns_tests::{command, header, id, read},
    workspace_columns_tests::prepare,
};
use crate::fleet_storage_fixture::{NOW, TestError};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use sqlx_core::query::query;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Invalid tags/classes, partial phases, narrowed counters and missing/gapped/extra manifest rows must fail the real production header reconstruction."
)]
async fn admin_import_rejects_corrupt_header_phase_and_ordered_manifest_rows()
-> Result<(), TestError> {
    for sql in [
        "UPDATE fleet_admin_imports SET schema_version='-1'",
        "UPDATE fleet_admin_imports SET schema_version='01'",
        "UPDATE fleet_admin_imports SET schema_version='18446744073709551616'",
        "UPDATE fleet_admin_imports SET schema_fingerprint=''",
        "UPDATE fleet_admin_imports SET manifest_epoch='2'",
        "UPDATE fleet_admin_imports SET manifest_sequence='01'",
        "UPDATE fleet_admin_imports SET pages=0",
        "UPDATE fleet_admin_imports SET pages=4294967296",
        "UPDATE fleet_admin_imports SET pages=1.5",
        "UPDATE fleet_admin_imports SET manifest_table_count=15",
        "UPDATE fleet_admin_imports SET manifest_table_count=17",
        "UPDATE fleet_admin_imports SET manifest_table_count=16.5",
        "UPDATE fleet_admin_imports SET manifest_table_count=-1",
        "UPDATE fleet_admin_imports SET summary_kind='future'",
        "UPDATE fleet_admin_imports SET summary_time_basis='future'",
        "UPDATE fleet_admin_imports SET summary_record_at=NULL",
        "UPDATE fleet_admin_imports SET summary_time_basis='legacy_unknown'",
        "UPDATE fleet_admin_imports SET phase_kind='future'",
        "UPDATE fleet_admin_imports SET phase_kind='published'",
        "UPDATE fleet_admin_imports SET phase_kind='complete'",
        "UPDATE fleet_admin_imports SET published_epoch='1'",
        "UPDATE fleet_admin_imports SET published_epoch='1',published_sequence='18446744073709551615'",
        "UPDATE fleet_admin_imports SET phase_kind='published',published_epoch='1',published_sequence='0'",
        "UPDATE fleet_admin_imports SET declared_owner=''",
        "UPDATE fleet_admin_imports SET provenance_installation='not-a-service'",
        "UPDATE fleet_admin_imports SET provenance_machine=' padded '",
        "UPDATE fleet_admin_imports SET created_at=0.5",
        "UPDATE fleet_admin_imports SET updated_at=0.5",
        "DELETE FROM fleet_admin_import_manifest_tables WHERE ordinal=15",
        "UPDATE fleet_admin_import_manifest_tables SET ordinal=99 WHERE ordinal=0",
        "UPDATE fleet_admin_import_manifest_tables SET ordinal=1.5 WHERE ordinal=0",
        "UPDATE fleet_admin_import_manifest_tables SET row_count='01' WHERE ordinal=0",
        "UPDATE fleet_admin_import_manifest_tables SET row_count='18446744073709551616' WHERE ordinal=0",
        "UPDATE fleet_admin_import_manifest_tables SET table_name='' WHERE ordinal=0",
        "UPDATE fleet_admin_import_manifest_tables SET table_name=X'616263' WHERE ordinal=0",
        "UPDATE fleet_admin_import_manifest_tables SET table_digest='not-a-digest' WHERE ordinal=0",
        "UPDATE fleet_admin_import_manifest_tables SET row_id='550e8400-e29b-41d4-a716-4466554400a9' WHERE ordinal=0",
        "UPDATE fleet_admin_import_manifest_tables SET created_at=0.5 WHERE ordinal=0",
        "UPDATE fleet_admin_import_manifest_tables SET updated_at=0.5 WHERE ordinal=0",
    ] {
        let fixture = prepare().await?;
        let offered = header(false)?;
        command(
            &fixture,
            crate::admin_import_store::AdminImportCommand::Reserve {
                header: Box::new(offered.clone()),
                candidate: id(0xdf01)?,
            },
            NOW.unix_millis(),
        )
        .await?
        .map_err(|refusal| format!("reserve failed: {refusal:?}"))?;
        fixture
            .object
            .transaction(move |scope| {
                Box::pin(async move {
                    query::<sqlx_turso::Turso>(sql)
                        .execute(scope.executor())
                        .await
                        .map_err(StoreError::backend)?;
                    Ok(())
                })
            })
            .await?;
        let result = read(&fixture, offered).await;
        assert!(
            matches!(result, Err(StoreError::Corrupt { .. })),
            "{sql}: {result:?}"
        );
        fixture.close().await?;
    }
    Ok(())
}
