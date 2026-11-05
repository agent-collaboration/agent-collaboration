//! Immutable effect receipts retain original references and placements in typed columns.
use super::workspace_columns_tests::prepare;
use crate::fleet_storage_fixture::TestError;
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use sqlx_core::{query::query, row::Row};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The approved ten typed effect columns must replace serialized effect JSON."
)]
async fn receipts_schema_exposes_typed_reference_placement_and_owner_times() -> Result<(), TestError>
{
    let fixture = prepare().await?;
    let columns = fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                query::<sqlx_turso::Turso>("PRAGMA table_info(fleet_decision_receipts)")
                    .fetch_all(scope.executor())
                    .await
                    .map_err(StoreError::backend)?
                    .into_iter()
                    .map(|row| {
                        Ok((
                            row.try_get::<String, _>("name")
                                .map_err(StoreError::backend)?,
                            row.try_get::<String, _>("type")
                                .map_err(StoreError::backend)?,
                            row.try_get::<i64, _>("pk").map_err(StoreError::backend)?,
                        ))
                    })
                    .collect::<Result<Vec<_>, StoreError>>()
            })
        })
        .await?;
    let expected = [
        "receipt_id",
        "request_id",
        "ordinal",
        "record_kind",
        "record_id",
        "fleet_position",
        "origin_installation",
        "origin_machine",
        "created_at",
        "updated_at",
    ];
    assert_eq!(columns.len(), expected.len(), "{columns:?}");
    for (index, name) in expected.into_iter().enumerate() {
        let storage = if ["ordinal", "fleet_position", "created_at", "updated_at"].contains(&name) {
            "INTEGER"
        } else {
            "TEXT"
        };
        assert!(
            columns.contains(&(name.into(), storage.into(), i64::from(index == 0))),
            "{name}: {columns:?}"
        );
    }
    fixture.close().await?;
    Ok(())
}

pub(super) fn effects() -> Result<Vec<message_board::DecisionEffect>, TestError> {
    use message_board::*;
    [
        RecordKind::Project,
        RecordKind::Adr,
        RecordKind::Board,
        RecordKind::Workspace,
    ]
    .into_iter()
    .enumerate()
    .map(|(index, kind)| {
        Ok((
            RecordRef {
                kind,
                id: format!("019f0000-0000-7000-8000-00000000be{index:02x}").try_into()?,
            },
            RecordPlacement::Fleet(FleetFeedEntry {
                position: FleetPosition::try_from(
                    9_007_199_254_740_993_u64 + u64::try_from(index)?,
                )?,
                origin: ActorOrigin {
                    installation: "550e8400-e29b-41d4-a716-4466554400a9"
                        .to_owned()
                        .try_into()?,
                    machine: format!("literal original effect {index} λ").try_into()?,
                },
            }),
        ))
    })
    .collect()
}
async fn write_receipts(
    fixture: &crate::fleet_storage_fixture::FleetStorageFixture,
    entry: message_board::RequestLedgerEntry,
    effects: Vec<message_board::DecisionEffect>,
    at: i64,
    duplicate_after: bool,
) -> Result<fleet_admission::FleetRequestResult, StoreError> {
    fixture.object.transaction(move |scope|Box::pin(async move {
        let mut ids=crate::fleet_row_ids::FleetRowIds::new(std::sync::Arc::new(crate::fleet_storage_fixture::FixtureIo));
        let receipt_ids=(0..effects.len()).map(|_|ids.next()).collect::<Result<Vec<_>,_>>()?;
        let reply=crate::fleet_decision_step::persist_decision(scope,crate::fleet_decision_step::FleetDecisionStep {
            entry,effects,receipt_ids,row_ids:ids,now:runtime_primitives::SyncTime::from_unix_millis(at),
            #[cfg(feature="conformance")]
            fault:crate::fleet_decision_step::FleetDecisionFault::None,
        }).await?;
        if duplicate_after {
            query::<sqlx_turso::Turso>("INSERT INTO fleet_decision_receipts SELECT * FROM fleet_decision_receipts LIMIT 1")
                .execute(scope.executor()).await.map_err(StoreError::backend)?;
        }
        Ok(reply)
    })).await
}
async fn read_receipts(
    fixture: &crate::fleet_storage_fixture::FleetStorageFixture,
    request: message_board::RequestId,
) -> Result<Vec<message_board::DecisionEffect>, StoreError> {
    fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                let entry = crate::fleet_decision_step::read_ledger(scope, &request)
                    .await?
                    .ok_or_else(|| StoreError::backend("missing receipt header"))?;
                let effects = crate::fleet_decision_step::read_receipts(scope, &request).await?;
                crate::fleet_decision_step::checked_reply(&entry, effects.clone())?;
                Ok(effects)
            })
        })
        .await
}
async fn physical_rows(
    fixture: &crate::fleet_storage_fixture::FleetStorageFixture,
) -> Result<Vec<(String, i64, i64)>, StoreError> {
    fixture.object.transaction(|scope|Box::pin(async move {
        query::<sqlx_turso::Turso>("SELECT receipt_id,created_at,updated_at FROM fleet_decision_receipts ORDER BY ordinal")
            .fetch_all(scope.executor()).await.map_err(StoreError::backend)?.into_iter().map(|row|Ok((
                row.try_get::<String,_>("receipt_id").map_err(StoreError::backend)?,
                row.try_get::<i64,_>("created_at").map_err(StoreError::backend)?,
                row.try_get::<i64,_>("updated_at").map_err(StoreError::backend)?
            ))).collect()
    })).await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "All four historical Fleet reference kinds, exact large placements, original origin, count/digest and timestamps must survive replay and owning reopen."
)]
async fn receipts_keep_exact_order_original_placements_and_owner_times_after_replay_and_reopen()
-> Result<(), TestError> {
    use crate::fleet_storage_fixture::NOW;
    let fixture = prepare().await?;
    let effects = effects()?;
    let entry = super::ledger_columns_tests::entry(
        0,
        message_board::StoredDecision::accepted(
            effects.clone(),
            message_board::ChangeOutcome::Changed,
        ),
    )?;
    let answer = write_receipts(
        &fixture,
        entry.clone(),
        effects.clone(),
        NOW.unix_millis(),
        false,
    )
    .await?;
    assert_eq!(
        read_receipts(&fixture, entry.request_id.clone()).await?,
        effects
    );
    let before = physical_rows(&fixture).await?;
    assert_eq!(before.len(), 4);
    for (id, created, updated) in &before {
        assert!(runtime_primitives::uuid_v7(durable_steps::Uuid::parse_str(
            id
        )?));
        assert_eq!((*created, *updated), (NOW.unix_millis(), NOW.unix_millis()));
    }
    let mut changed = entry.clone();
    changed.decision = message_board::StoredDecision::Rejected {
        reasons: vec![message_board::Refusal::Archived],
    };
    assert_eq!(
        write_receipts(&fixture, changed, Vec::new(), NOW.unix_millis() + 17, false).await?,
        answer
    );
    assert_eq!(physical_rows(&fixture).await?, before);
    let fixture = fixture.reopen().await?;
    assert_eq!(
        read_receipts(&fixture, entry.request_id.clone()).await?,
        effects
    );
    assert_eq!(physical_rows(&fixture).await?, before);
    // These are self-contained historical receipts: no canonical resources were
    // ever inserted, and a new replay proposal cannot replace their placements.
    assert_eq!(
        write_receipts(&fixture, entry, Vec::new(), NOW.unix_millis() + 29, false).await?,
        answer
    );
    assert_eq!(physical_rows(&fixture).await?, before);
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Physical class/tag/origin corruption, ordinal gaps and a commitment-changing reference must fail the production receipt reader."
)]
async fn receipts_refuse_invalid_classes_tags_ordinals_and_changed_commitments()
-> Result<(), TestError> {
    for sql in [
        "UPDATE fleet_decision_receipts SET receipt_id='550e8400-e29b-41d4-a716-4466554400a9' WHERE ordinal=0",
        "UPDATE fleet_decision_receipts SET ordinal=5 WHERE ordinal=0",
        "UPDATE fleet_decision_receipts SET ordinal=0.5 WHERE ordinal=0",
        "UPDATE fleet_decision_receipts SET record_kind='post' WHERE ordinal=0",
        "UPDATE fleet_decision_receipts SET record_kind='future' WHERE ordinal=0",
        "UPDATE fleet_decision_receipts SET record_id='not-a-uuid' WHERE ordinal=0",
        "UPDATE fleet_decision_receipts SET record_id='019f0000-0000-7000-8000-00000000beff' WHERE ordinal=0",
        "UPDATE fleet_decision_receipts SET record_id=X'616263' WHERE ordinal=0",
        "UPDATE fleet_decision_receipts SET fleet_position=-1 WHERE ordinal=0",
        "UPDATE fleet_decision_receipts SET fleet_position=0.5 WHERE ordinal=0",
        "UPDATE fleet_decision_receipts SET origin_machine=' padded ' WHERE ordinal=0",
        "UPDATE fleet_decision_receipts SET origin_installation='not-a-service' WHERE ordinal=0",
        "UPDATE fleet_decision_receipts SET created_at=0.5 WHERE ordinal=0",
        "UPDATE fleet_decision_receipts SET updated_at=0.5 WHERE ordinal=0",
        "DELETE FROM fleet_decision_receipts WHERE ordinal=3",
    ] {
        let fixture = prepare().await?;
        let effects = effects()?;
        let entry = super::ledger_columns_tests::entry(
            0,
            message_board::StoredDecision::accepted(
                effects.clone(),
                message_board::ChangeOutcome::Changed,
            ),
        )?;
        write_receipts(
            &fixture,
            entry.clone(),
            effects,
            crate::fleet_storage_fixture::NOW.unix_millis(),
            false,
        )
        .await?;
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
        let result = read_receipts(&fixture, entry.request_id).await;
        assert!(
            matches!(result, Err(StoreError::Corrupt { .. })),
            "{sql}: {result:?}"
        );
        fixture.close().await?;
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A real final receipt-key conflict must roll the entire first decision and all four ordered effects back before owning reopen and retry."
)]
async fn receipts_late_sql_conflict_rolls_back_header_and_all_typed_effect_rows()
-> Result<(), TestError> {
    let fixture = prepare().await?;
    let effects = effects()?;
    let entry = super::ledger_columns_tests::entry(
        0,
        message_board::StoredDecision::accepted(
            effects.clone(),
            message_board::ChangeOutcome::AlreadySo,
        ),
    )?;
    let result = write_receipts(
        &fixture,
        entry.clone(),
        effects.clone(),
        crate::fleet_storage_fixture::NOW.unix_millis(),
        true,
    )
    .await;
    assert!(matches!(result, Err(StoreError::Backend { .. })));
    assert_eq!(
        super::ledger_columns_tests::read(&fixture, entry.request_id.clone()).await?,
        None
    );
    assert!(physical_rows(&fixture).await?.is_empty());
    let fixture = fixture.reopen().await?;
    assert_eq!(
        super::ledger_columns_tests::read(&fixture, entry.request_id.clone()).await?,
        None
    );
    assert!(physical_rows(&fixture).await?.is_empty());
    write_receipts(
        &fixture,
        entry.clone(),
        effects.clone(),
        crate::fleet_storage_fixture::NOW.unix_millis() + 17,
        false,
    )
    .await?;
    assert_eq!(read_receipts(&fixture, entry.request_id).await?, effects);
    fixture.close().await?;
    Ok(())
}
