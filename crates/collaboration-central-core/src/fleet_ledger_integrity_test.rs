//! Corrupt physical columns must never become a replay authority.
use super::{
    ledger_columns_tests::{entry, read, refusals, write},
    workspace_columns_tests::prepare,
};
use crate::fleet_storage_fixture::{FixtureIo, FleetStorageFixture, NOW, TestError};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use message_board::{ChangeOutcome, Refusal, StoredDecision};
use sqlx_core::{query::query, row::Row};

async fn execute(fixture: &FleetStorageFixture, sql: &'static str) -> Result<(), StoreError> {
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
        .await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Replay must retain the original decision, timestamps, actor, scope and independent envelope request."
)]
async fn ledger_replay_preserves_first_authority_and_ingestion_time_after_reopen()
-> Result<(), TestError> {
    let fixture = prepare().await?;
    let mut expected = Vec::new();
    for (index, change) in [ChangeOutcome::Changed, ChangeOutcome::AlreadySo]
        .into_iter()
        .enumerate()
    {
        let value = entry(index, StoredDecision::accepted(Vec::new(), change))?;
        let answer = write(&fixture, value.clone(), NOW.unix_millis()).await?;
        let mut changed = value.clone();
        changed.envelope.scope.board = None;
        changed.envelope.request = None;
        changed.envelope.time = message_board::RecordTime::Router("2030-01-01T00:00:00Z".parse()?);
        changed.decision = StoredDecision::Rejected {
            reasons: vec![Refusal::Archived],
        };
        assert_eq!(
            write(&fixture, changed.clone(), NOW.unix_millis() + 17).await?,
            answer
        );
        changed.canonical_hash = "cd".repeat(32).try_into()?;
        assert_eq!(
            write(&fixture, changed, NOW.unix_millis() + 29).await?,
            fleet_admission::FleetRequestResult::ConflictingRequest
        );
        assert_eq!(
            read(&fixture, value.request_id.clone()).await?,
            Some(value.clone())
        );
        expected.push(value);
    }
    let timestamps = fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                query::<sqlx_turso::Turso>(
                    "SELECT created_at,updated_at FROM fleet_request_decisions ORDER BY request_id",
                )
                .fetch_all(scope.executor())
                .await
                .map_err(StoreError::backend)?
                .into_iter()
                .map(|row| {
                    Ok((
                        row.try_get::<i64, _>("created_at")
                            .map_err(StoreError::backend)?,
                        row.try_get::<i64, _>("updated_at")
                            .map_err(StoreError::backend)?,
                    ))
                })
                .collect::<Result<Vec<_>, StoreError>>()
            })
        })
        .await?;
    assert_eq!(timestamps, vec![(NOW.unix_millis(), NOW.unix_millis()); 2]);
    let fixture = fixture.reopen().await?;
    for value in expected {
        assert_eq!(read(&fixture, value.request_id.clone()).await?, Some(value));
    }
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Full-width unsigned commitments are TEXT and must not truncate through SQLite INTEGER or floating point."
)]
async fn ledger_retains_full_unsigned_effect_count_in_immutable_header() -> Result<(), TestError> {
    let fixture = prepare().await?;
    let value = entry(
        0,
        StoredDecision::Accepted {
            change: ChangeOutcome::Changed,
            effects: message_board::DecisionEffectsHeader {
                count: u64::MAX,
                digest: "ab".repeat(32).try_into()?,
            },
        },
    )?;
    let offered = value.clone();
    fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move { crate::fleet_ledger_codec::insert(scope, &offered, NOW).await })
        })
        .await?;
    assert_eq!(
        read(&fixture, value.request_id.clone()).await?,
        Some(value.clone())
    );
    let fixture = fixture.reopen().await?;
    assert_eq!(read(&fixture, value.request_id.clone()).await?, Some(value));
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Unknown tags, inactive fields, malformed composites and physical storage classes must fail the production decoder."
)]
async fn ledger_rejects_corrupt_headers_reason_graphs_and_evidence() -> Result<(), TestError> {
    // Each fixture starts with a valid nested, committed two-task path. Mutations
    // are physical SQL, never a second serializer or a mocked owner read.
    let task: message_board::TaskId = "019f0000-0000-7000-8000-00000000ac02"
        .to_owned()
        .try_into()?;
    let reason = Refusal::ProjectRejected {
        reason: Box::new(Refusal::Task(message_board::TaskRefusal::DependencyCycle {
            path: message_board::AbridgedEvidence::from_full(vec![task.clone(), task]),
        })),
    };
    for sql in [
        "UPDATE fleet_request_decisions SET decision_kind='future'",
        "UPDATE fleet_request_decisions SET change_kind='changed'",
        "UPDATE fleet_request_decisions SET reason_count=0",
        "UPDATE fleet_request_decisions SET reason_count=1.5",
        "UPDATE fleet_request_decisions SET fleet_position=1.5",
        "UPDATE fleet_request_decisions SET origin_machine=' padded '",
        "UPDATE fleet_request_decisions SET router_time=NULL,time_basis='legacyUnknown'",
        "UPDATE fleet_request_decisions SET actor_human_id='inactive' WHERE actor_kind='router'",
        "UPDATE fleet_request_decisions SET created_at=1.5",
        "UPDATE fleet_decision_reasons SET refusal_group='future' WHERE parent_reason_id IS NOT NULL",
        "UPDATE fleet_decision_reasons SET refusal_kind='future' WHERE parent_reason_id IS NOT NULL",
        "UPDATE fleet_decision_reasons SET field='inactive' WHERE parent_reason_id IS NOT NULL",
        "UPDATE fleet_decision_reasons SET ordinal=2 WHERE parent_reason_id IS NOT NULL",
        "UPDATE fleet_decision_reasons SET ordinal=0.5 WHERE parent_reason_id IS NOT NULL",
        "UPDATE fleet_decision_reasons SET parent_reason_id=(SELECT reason_id FROM fleet_decision_reasons WHERE parent_reason_id IS NOT NULL) WHERE parent_reason_id IS NULL",
        "UPDATE fleet_decision_reasons SET parent_reason_id=reason_id WHERE parent_reason_id IS NOT NULL",
        "UPDATE fleet_decision_reasons SET refusal_kind='projectRejected',refusal_group='root' WHERE parent_reason_id IS NOT NULL",
        "UPDATE fleet_decision_reasons SET actor_kind='session' WHERE parent_reason_id IS NOT NULL",
        "UPDATE fleet_decision_reasons SET record_kind='task' WHERE parent_reason_id IS NOT NULL",
        "UPDATE fleet_decision_reasons SET requirement_kind='holds' WHERE parent_reason_id IS NOT NULL",
        "UPDATE fleet_decision_reasons SET role_subject_kind='project' WHERE parent_reason_id IS NOT NULL",
        "UPDATE fleet_decision_reasons SET last_seen_epoch='1' WHERE parent_reason_id IS NOT NULL",
        "UPDATE fleet_decision_reasons SET evidence_returned_count=3 WHERE parent_reason_id IS NOT NULL",
        "UPDATE fleet_decision_reasons SET evidence_returned_count=2.5 WHERE parent_reason_id IS NOT NULL",
        "UPDATE fleet_decision_reasons SET evidence_total_count='02' WHERE parent_reason_id IS NOT NULL",
        "UPDATE fleet_decision_reasons SET evidence_total_count='18446744073709551616' WHERE parent_reason_id IS NOT NULL",
        "UPDATE fleet_decision_reasons SET evidence_digest='abababababababababababababababababababababababababababababababab' WHERE parent_reason_id IS NOT NULL",
        "UPDATE fleet_decision_reasons SET evidence_abridged=2 WHERE parent_reason_id IS NOT NULL",
        "UPDATE fleet_decision_reasons SET evidence_abridged=1 WHERE parent_reason_id IS NOT NULL",
        "UPDATE fleet_decision_reasons SET evidence_kind='assignment' WHERE parent_reason_id IS NOT NULL",
        "UPDATE fleet_decision_reasons SET evidence_digest=NULL WHERE parent_reason_id IS NOT NULL",
        "DELETE FROM fleet_decision_reason_evidence WHERE ordinal=0",
        "UPDATE fleet_decision_reason_evidence SET ordinal=2 WHERE ordinal=1",
        "UPDATE fleet_decision_reason_evidence SET entry_kind='future'",
        "UPDATE fleet_decision_reason_evidence SET task_id='not-a-uuid'",
        "UPDATE fleet_decision_reason_evidence SET assignment_kind='lead'",
        "UPDATE fleet_decision_reason_evidence SET updated_at=0.5",
        "UPDATE fleet_decision_reason_evidence SET reason_id=(SELECT reason_id FROM fleet_decision_reasons WHERE parent_reason_id IS NULL)",
    ] {
        let fixture = prepare().await?;
        let value = entry(
            0,
            StoredDecision::Rejected {
                reasons: vec![reason.clone()],
            },
        )?;
        write(&fixture, value.clone(), NOW.unix_millis()).await?;
        execute(&fixture, sql).await?;
        let result = read(&fixture, value.request_id).await;
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
    reason = "A late real SQL fault must roll back every typed parent, reason and evidence row before reopen."
)]
async fn ledger_late_uniqueness_failure_rolls_back_entire_reason_tree() -> Result<(), TestError> {
    let fixture = prepare().await?;
    let reason = refusals()?
        .into_iter()
        .find(|value| {
            matches!(
                value,
                Refusal::Trail(message_board::TrailRefusal::SubjectRequired { .. })
            )
        })
        .ok_or("missing evidence fixture")?;
    let value = entry(
        0,
        StoredDecision::Rejected {
            reasons: vec![reason],
        },
    )?;
    let original = value.clone();
    let result=fixture.object.transaction(move |scope|Box::pin(async move {
        crate::fleet_decision_step::persist_decision(scope,crate::fleet_decision_step::FleetDecisionStep {
            entry:value, effects:Vec::new(),receipt_ids:Vec::new(),now:NOW,
            row_ids:crate::fleet_row_ids::FleetRowIds::new(std::sync::Arc::new(FixtureIo)),
            #[cfg(feature="conformance")]
            fault:crate::fleet_decision_step::FleetDecisionFault::None,
        }).await?;
        query::<sqlx_turso::Turso>("INSERT INTO fleet_decision_reason_evidence SELECT * FROM fleet_decision_reason_evidence LIMIT 1")
            .execute(scope.executor()).await.map_err(StoreError::backend)?;
        Ok(())
    })).await;
    assert!(result.is_err(), "late primary-key violation was accepted");
    assert_eq!(read(&fixture, original.request_id.clone()).await?, None);
    let fixture = fixture.reopen().await?;
    assert_eq!(read(&fixture, original.request_id.clone()).await?, None);
    let counts=fixture.object.transaction(|scope|Box::pin(async move {
        let row=query::<sqlx_turso::Turso>("SELECT (SELECT count(*) FROM fleet_request_decisions) AS parents,(SELECT count(*) FROM fleet_decision_reasons) AS reasons,(SELECT count(*) FROM fleet_decision_reason_evidence) AS evidence").fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        Ok((row.try_get::<i64,_>("parents").map_err(StoreError::backend)?,row.try_get::<i64,_>("reasons").map_err(StoreError::backend)?,row.try_get::<i64,_>("evidence").map_err(StoreError::backend)?))
    })).await?;
    assert_eq!(counts, (0, 0, 0));
    write(&fixture, original.clone(), NOW.unix_millis() + 17).await?;
    assert_eq!(
        read(&fixture, original.request_id.clone()).await?,
        Some(original)
    );
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Relationally valid cross-request parents and invalid stored stale-seat coupling must fail the same production tree decoder."
)]
async fn ledger_rejects_cross_request_parent_and_stored_stale_seat_pair() -> Result<(), TestError> {
    let fixture = prepare().await?;
    let value = entry(
        0,
        StoredDecision::Rejected {
            reasons: vec![Refusal::ProjectRejected {
                reason: Box::new(Refusal::Archived),
            }],
        },
    )?;
    let other = entry(
        1,
        StoredDecision::Rejected {
            reasons: vec![Refusal::Archived],
        },
    )?;
    write(&fixture, value.clone(), NOW.unix_millis()).await?;
    write(&fixture, other.clone(), NOW.unix_millis()).await?;
    execute(&fixture,"UPDATE fleet_decision_reasons SET parent_reason_id=(SELECT reason_id FROM fleet_decision_reasons WHERE request_id='019f0000-0000-7000-8000-000000000001') WHERE parent_reason_id IS NOT NULL").await?;
    assert!(matches!(
        read(&fixture, value.request_id).await,
        Err(StoreError::Corrupt { .. })
    ));
    assert_eq!(
        read(&fixture, other.request_id.clone()).await?,
        Some(other),
        "an unrelated request still owns only its own reason rows"
    );
    fixture.close().await?;
    for kind in ["staleOrchestrator", "staleImplementer"] {
        let fixture = prepare().await?;
        let reason = refusals()?
            .into_iter()
            .find(|reason| match reason {
                Refusal::Thread(thread) => match thread.as_ref() {
                    message_board::ThreadRefusal::StaleOrchestrator {
                        holder: Some(_), ..
                    } => kind == "staleOrchestrator",
                    message_board::ThreadRefusal::StaleImplementer {
                        holder: Some(_), ..
                    } => kind == "staleImplementer",
                    _ => false,
                },
                _ => false,
            })
            .ok_or("missing stale-seat fixture")?;
        let value = entry(
            0,
            StoredDecision::Rejected {
                reasons: vec![reason],
            },
        )?;
        write(&fixture, value.clone(), NOW.unix_millis()).await?;
        execute(
            &fixture,
            "UPDATE fleet_decision_reasons SET last_seen_epoch=NULL,last_seen_position=NULL",
        )
        .await?;
        assert!(
            matches!(
                read(&fixture, value.request_id).await,
                Err(StoreError::Corrupt { .. })
            ),
            "{kind} must reuse shared holder/cursor validation"
        );
        fixture.close().await?;
    }
    Ok(())
}
