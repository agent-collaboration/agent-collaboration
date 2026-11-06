//! Real SQL statements and exact returned records bound page/bundle hydration cost.
use crate::fleet_storage_fixture::{FleetStorageFixture, NOW, TestError};
use crate::object_bootstrap::workspace_columns_tests::{prepare, workspace, write};
use durable_steps::{AtomicStore, StoreError};
use fleet_admission::{FleetNameScope, FleetReadKey, FleetReads};
use message_board::{Record, RecordRef};
#[path = "fleet_statement_count.rs"]
mod statement_count;
use statement_count::StatementCountScope;

async fn seeded_mixed(count: usize) -> Result<(FleetStorageFixture, Vec<Record>), TestError> {
    use crate::object_bootstrap::{adr_columns_tests, ledger_columns_tests};
    use message_board::{Refusal, StoredDecision};
    let (fixture, mut records) = seeded(count).await?;
    for index in 0..count {
        let Record::Workspace(parent) = &records[index] else {
            return Err("wrong fixture kind".into());
        };
        let Record::Adr(mut adr) = adr_columns_tests::adr(index as u64)? else {
            return Err("wrong fixture kind".into());
        };
        adr.adr_id = format!("019f0000-0000-7000-8000-{:012x}", 0xc100 + index).try_into()?;
        adr.envelope.id = adr.adr_id.clone().into();
        adr.workspace = parent.workspace_id.clone();
        adr.envelope.scope.workspace = parent.workspace_id.clone();
        let record = Record::Adr(adr);
        write(&fixture, record.clone(), NOW.unix_millis()).await?;
        let mut entry = ledger_columns_tests::entry(
            0xc200 + index,
            StoredDecision::Rejected {
                reasons: vec![Refusal::ProjectRejected {
                    reason: Box::new(Refusal::InvalidShape {
                        field: format!("original-{index}"),
                    }),
                }],
            },
        )?;
        entry.envelope.scope.workspace = parent.workspace_id.clone();
        ledger_columns_tests::write(&fixture, entry.clone(), NOW.unix_millis()).await?;
        records.push(record);
        records.push(Record::Request(entry));
    }
    records.sort_by_key(|record| (record.reference().id, record.reference().kind));
    Ok((fixture, records))
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Exact mixed bodies, child ordering, reason trees and SQL budgets must fail loudly."
)]
async fn mixed_typed_children_and_reason_trees_use_fixed_statement_ranges() -> Result<(), TestError>
{
    let mut counts = Vec::new();
    for size in [2, 8] {
        let (fixture, expected) = seeded_mixed(size).await?;
        let keys = expected
            .iter()
            .flat_map(|record| {
                [
                    FleetReadKey::Record {
                        record: record.reference(),
                    },
                    FleetReadKey::ImportDigest {
                        record: record.reference(),
                    },
                ]
            })
            .collect();
        let record_count = expected.len();
        let (page, bundle, statement_counts) = fixture
            .object
            .transaction(move |scope| {
                Box::pin(async move {
                    let mut snapshot_scope = StatementCountScope::new(scope);
                    let page = super::records(&mut snapshot_scope, None, record_count).await?;
                    let snapshot_count = snapshot_scope.statement_count();
                    let mut bundle_scope = StatementCountScope::new(scope);
                    let bundle =
                        crate::fleet_scope_snapshot::read(&mut bundle_scope, FleetReads { keys })
                            .await?;
                    Ok((
                        page,
                        bundle,
                        (snapshot_count, bundle_scope.statement_count()),
                    ))
                })
            })
            .await?;
        assert_eq!(page, (expected.clone(), None));
        assert_eq!(bundle.records, expected);
        assert_eq!(bundle.imports.len(), size * 2);
        counts.push(statement_counts);
        // The cursor still selects a global identity prefix across all three owners.
        let expected_first = expected[..size].to_vec();
        let after = expected_first.last().ok_or("empty prefix")?.reference();
        let expected_rest = expected[size..].to_vec();
        let (first, rest) = fixture
            .object
            .transaction(move |scope| {
                Box::pin(async move {
                    let first = super::records(scope, None, size).await?;
                    let rest = super::records(scope, Some(after), record_count).await?;
                    Ok((first, rest))
                })
            })
            .await?;
        assert_eq!(first.0, expected_first);
        assert_eq!(first.1, first.0.last().map(Record::reference));
        assert_eq!(rest, (expected_rest, None));
        fixture.close().await?;
    }
    eprintln!(
        "Q2/Q4 mixed Workspace+ADR+rejected Request statement counts at [2,8] per kind: {counts:?}"
    );
    assert_eq!(counts, [(12, 7), (12, 7)]);
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Corrupt children and reason trees must be refused by both real read surfaces."
)]
async fn batch_pages_and_import_inputs_keep_strict_child_and_reason_validation()
-> Result<(), TestError> {
    use durable_steps::storage::TransactionScope;
    for sql in [
        "UPDATE fleet_workspace_repository_refs SET reference_kind='invalid' WHERE ordinal=0",
        "UPDATE fleet_adr_repository_files SET path=CAST(x'00' AS BLOB) WHERE ordinal=0",
        "UPDATE fleet_decision_reasons SET ordinal=3 WHERE parent_reason_id IS NOT NULL",
    ] {
        let (fixture, records) = seeded_mixed(2).await?;
        let keys = records
            .iter()
            .map(|record| FleetReadKey::Record {
                record: record.reference(),
            })
            .collect();
        let (page, bundle) = fixture
            .object
            .transaction(move |scope| {
                Box::pin(async move {
                    sqlx_core::query::query::<sqlx_turso::Turso>(sql)
                        .execute(scope.executor())
                        .await
                        .map_err(StoreError::backend)?;
                    let page = super::records(scope, None, 20).await;
                    let bundle =
                        crate::fleet_scope_snapshot::read(scope, FleetReads { keys }).await;
                    Ok((page, bundle))
                })
            })
            .await?;
        assert!(
            matches!(page, Err(StoreError::Corrupt { .. })),
            "page accepted corruption: {sql}"
        );
        assert!(
            matches!(bundle, Err(StoreError::Corrupt { .. })),
            "bundle accepted corruption: {sql}"
        );
        fixture.close().await?;
    }
    Ok(())
}

async fn seeded(count: usize) -> Result<(FleetStorageFixture, Vec<Record>), TestError> {
    let fixture = prepare().await?;
    let mut records = Vec::new();
    for index in 0..count {
        let Record::Workspace(mut value) = workspace(index as u64 + 1)? else {
            return Err("wrong fixture kind".into());
        };
        value.workspace_id =
            format!("019f0000-0000-7000-8000-{:012x}", 0xc000 + index).try_into()?;
        value.envelope.id = value.workspace_id.clone().into();
        value.envelope.scope.workspace = value.workspace_id.clone();
        value.name = format!("batch-workspace-{index}").try_into()?;
        let record = Record::Workspace(value);
        write(&fixture, record.clone(), NOW.unix_millis()).await?;
        records.push(record);
    }
    Ok((fixture, records))
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The exact page and fixed SQL count are the regression oracle."
)]
async fn snapshot_hydration_statement_count_is_independent_of_page_size() -> Result<(), TestError> {
    // Arrange: separate real stores, each with nonempty ordered repository children.
    let mut counts = Vec::new();
    for size in [2, 8] {
        let (fixture, expected) = seeded(size).await?;
        // Act: observe only the actual page read inside its owning transaction.
        let ((records, next), statement_count) = fixture
            .object
            .transaction(move |scope| {
                Box::pin(async move {
                    let mut counted = StatementCountScope::new(scope);
                    let result = super::records(&mut counted, None, size).await?;
                    Ok((result, counted.statement_count()))
                })
            })
            .await?;
        // Assert: exact original bodies, children and end cursor, plus fixed SQL budget.
        assert_eq!(records, expected);
        assert_eq!(next, None);
        counts.push(statement_count);
        fixture.close().await?;
    }
    eprintln!("Q2 actual statements for page sizes [2,8]: {counts:?}");
    assert_eq!(counts, [7, 7]); // Five key ranges, one parent range, one child range.
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The exact bundle contents, absence and fixed SQL count are the regression oracle."
)]
async fn import_hydration_statement_count_is_independent_of_bundle_size() -> Result<(), TestError> {
    let mut counts = Vec::new();
    for size in [2, 8] {
        let (fixture, expected) = seeded(size).await?;
        let mut keys = Vec::new();
        for record in &expected {
            let Record::Workspace(value) = record else {
                return Err("wrong fixture kind".into());
            };
            keys.extend([
                FleetReadKey::Record {
                    record: record.reference(),
                },
                FleetReadKey::ImportDigest {
                    record: record.reference(),
                },
                FleetReadKey::Name {
                    scope: FleetNameScope::Workspaces {},
                    name: value.name.clone(),
                },
            ]);
        }
        // A missing reference must stay absent; repeated keys must not duplicate bodies.
        let absent: RecordRef = message_board::WorkspaceId::try_from(
            "019f0000-0000-7000-8000-00000000d000".to_owned(),
        )?
        .into();
        keys.push(FleetReadKey::Record {
            record: absent.clone(),
        });
        let (snapshot, statement_count) = fixture
            .object
            .transaction(move |scope| {
                Box::pin(async move {
                    let mut counted = StatementCountScope::new(scope);
                    let snapshot =
                        crate::fleet_scope_snapshot::read(&mut counted, FleetReads { keys })
                            .await?;
                    Ok((snapshot, counted.statement_count()))
                })
            })
            .await?;
        assert_eq!(snapshot.records, expected);
        assert!(snapshot.record(&absent).is_none());
        assert_eq!(snapshot.names.len(), size);
        assert_eq!(snapshot.imports.len(), size);
        for record in &snapshot.records {
            assert!(
                snapshot
                    .names
                    .iter()
                    .any(|name| name.record == record.reference())
            );
            let imported = snapshot
                .imports
                .iter()
                .find(|import| import.record == record.reference())
                .ok_or_else(|| StoreError::backend("missing original import digest"))?;
            assert_eq!(imported.digest.as_str(), "ab".repeat(32));
        }
        counts.push(statement_count);
        fixture.close().await?;
    }
    eprintln!("Q4 actual statements for bundle sizes [2,8]: {counts:?}");
    assert_eq!(counts, [3, 3]); // One name range, one parent range, one child range.
    Ok(())
}
