//! Ledger rows are insert-once; terminal voiding is a read projection, never stored state.
use crate::{ProjectStorageError, ProjectStore};
use message_board::*;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
async fn rejection_shape_is_checked_at_native_admission_and_decode() -> TestResult {
    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let store =
        ProjectStore::open(&directory.path().join("project.sqlite"), project.clone()).await?;
    let base = crate::project_store_tests::project_record(project);
    let id = RequestId::generate();
    let ledger = RequestLedgerEntry {
        envelope: RecordEnvelope {
            id: id.clone().into(),
            request: Some(id.clone()),
            ..base.envelope().clone()
        },
        request_id: id,
        canonical_hash: CanonicalHash::try_from("a".repeat(64))?,
        accepted_in: None,
        decision: StoredDecision::Rejected {
            reasons: vec![Refusal::Archived],
        },
    };
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    write.append_record(base).await?;
    write.append_record(Record::Request(ledger.clone())).await?;
    write.commit().await?;
    let mut invalid = ledger.clone();
    invalid.request_id = RequestId::generate();
    invalid.envelope.id = invalid.request_id.clone().into();
    invalid.envelope.request = Some(invalid.request_id.clone());
    invalid.decision = StoredDecision::Rejected {
        reasons: vec![
            Refusal::Archived,
            Refusal::InvalidShape {
                field: "fixture".into(),
            },
        ],
    };
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    assert!(
        matches!(
            write.append_record(Record::Request(invalid.clone())).await,
            Err(ProjectStorageError::InvalidRecord {
                field: "reasons",
                ..
            })
        ),
        "native admission accepted two non-move reasons"
    );
    write.rollback().await?;
    let mut corrupt = store.database.begin_write().await?;
    assert_eq!(
        sqlx::query("UPDATE request_decisions SET decision=? WHERE request_id=?")
            .bind(serde_json::to_string(&invalid.decision)?)
            .bind(ledger.request_id.as_str())
            .execute(&mut corrupt)
            .await?
            .rows_affected(),
        1
    );
    corrupt.commit().await?;
    let mut read = store.snapshot().await?;
    assert!(
        matches!(
            read.record(&ledger.request_id.into()).await,
            Err(ProjectStorageError::InvalidRecord {
                field: "reasons",
                ..
            })
        ),
        "native decoding accepted two non-move reasons"
    );
    read.finish().await?;
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn request_rows_are_insert_once_and_a_persisted_voided_decision_is_refused() -> TestResult {
    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let store =
        ProjectStore::open(&directory.path().join("project.sqlite"), project.clone()).await?;
    let base = crate::project_store_tests::project_record(project);
    let id = RequestId::generate();
    let ledger = RequestLedgerEntry {
        envelope: RecordEnvelope {
            id: id.clone().into(),
            request: Some(id.clone()),
            ..base.envelope().clone()
        },
        request_id: id.clone(),
        canonical_hash: CanonicalHash::try_from("a".repeat(64))?,
        accepted_in: Some(EpochNumber::INITIAL),
        decision: StoredDecision::accepted(Vec::new(), ChangeOutcome::AlreadySo),
    };
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    write.append_record(base).await?;
    let accepted = write.append_record(Record::Request(ledger.clone())).await?;
    write.commit().await?;

    let encoded = serde_json::to_value(&ledger)?;
    assert_eq!(
        encoded.pointer("/decision/effects/count"),
        Some(&serde_json::json!(0)),
        "ledger must keep a bounded effects header"
    );
    assert!(encoded.pointer("/decision/records").is_none());
    assert!(encoded.pointer("/decision/positions").is_none());

    // Neither a replay nor a derived terminal result may rewrite the original row.
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    assert!(matches!(
        write.append_record(Record::Request(ledger.clone())).await,
        Err(ProjectStorageError::InvalidRecord {
            field: "immutableContent",
            ..
        })
    ));
    write.rollback().await?;

    // A native stored-data counterexample must fail decoding, even though its JSON is valid.
    let mut corrupt = store.database.begin_write().await?;
    let changed = sqlx::query("UPDATE request_decisions SET decision=? WHERE request_id=?")
        .bind(
            serde_json::json!({"kind":"voided","cause":{"kind":"takeover","epoch":2}}).to_string(),
        )
        .bind(id.as_str())
        .execute(&mut corrupt)
        .await?;
    assert_eq!(changed.rows_affected(), 1);
    corrupt.commit().await?;
    let mut read = store.snapshot().await?;
    assert_eq!(read.high_water(), accepted);
    assert!(matches!(
        read.record(&id.into()).await,
        Err(ProjectStorageError::InvalidRecord {
            field: "decision",
            ..
        })
    ));
    read.finish().await?;
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn a_rejected_decision_cannot_commit_record_effects() -> TestResult {
    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let store =
        ProjectStore::open(&directory.path().join("project.sqlite"), project.clone()).await?;
    let base = crate::project_store_tests::project_record(project);
    let request = RequestId::generate();
    let fact = FactId::generate();
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    write.append_record(base.clone()).await?;
    write
        .append_record(Record::Fact(Fact {
            envelope: RecordEnvelope {
                id: fact.clone().into(),
                request: Some(request.clone()),
                ..base.envelope().clone()
            },
            fact_id: fact,
            kind: FactKind::Lifecycle(LifecycleFact::PhaseCompleted {
                phase: "fixture".into(),
            }),
            links: vec![],
            producer_time: chrono::Utc::now(),
            anchor: None,
            late: false,
        }))
        .await?;
    let rejected = Record::Request(RequestLedgerEntry {
        envelope: RecordEnvelope {
            id: request.clone().into(),
            request: Some(request.clone()),
            ..base.envelope().clone()
        },
        request_id: request,
        canonical_hash: CanonicalHash::try_from("b".repeat(64))?,
        accepted_in: None,
        decision: StoredDecision::Rejected {
            reasons: vec![Refusal::Archived],
        },
    });
    let result = write.append_record(rejected).await;
    assert!(
        matches!(
            result,
            Err(ProjectStorageError::InvalidRecord {
                field: "effects",
                ..
            })
        ),
        "rejected decision accepted effects: {result:?}"
    );
    write.rollback().await?;
    store.close().await;
    Ok(())
}
