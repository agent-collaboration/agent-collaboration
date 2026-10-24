//! Ledger rows are insert-once; terminal voiding is a read projection, never stored state.
use crate::{ProjectStorageError, ProjectStore};
use message_board::*;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

fn rejected_move(base: &Record, reasons: Vec<RejectReason>) -> Result<Record, TextValidationError> {
    let move_id = MoveId::generate();
    let actor = base.envelope().actor.clone();
    MoveRecord::new(MoveRecordProps {
        envelope: RecordEnvelope {
            id: move_id.clone().into(),
            ..base.envelope().clone()
        },
        move_id,
        project: base
            .envelope()
            .scope
            .project
            .clone()
            .ok_or(TextValidationError {
                field: "project",
                requirement: "fixture must own a project",
            })?,
        move_name: MoveName::try_from("attempt".to_owned())?,
        endpoints: None,
        evidence: Vec::new(),
        reason: String::new(),
        requester: actor,
        outcome: MoveOutcome::Rejected { reasons },
    })
    .map(Record::Move)
}

#[tokio::test]
async fn move_rejection_shape_is_checked_at_native_admission_and_decode() -> TestResult {
    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let store =
        ProjectStore::open(&directory.path().join("project.sqlite"), project.clone()).await?;
    let base = crate::project_store_tests::project_record(project);
    let mut reasons = vec![
        RejectReason::Archived {},
        RejectReason::Closed {},
        RejectReason::StaleRevision {
            current: SubjectRevision::INITIAL,
        },
        RejectReason::MoveNotAllowed {
            edge: "attempt".to_owned().try_into()?,
            required_role: Some(AuthorityRole::Lead),
        },
    ];
    let evaluator = Identity::Human {
        human_id: "verifier".to_owned().try_into()?,
    };
    reasons.extend(
        (0..MAX_EDGE_GUARDS).map(|_| RejectReason::GuardUnavailable {
            guard: "guard".to_owned(),
            evaluator: evaluator.clone(),
        }),
    );
    assert_eq!(reasons.len(), 12);
    let valid = rejected_move(&base, reasons.clone())?;
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    write.append_record(base.clone()).await?;
    write.append_record(valid.clone()).await?;
    write.commit().await?;
    reasons.push(RejectReason::Closed {});
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    assert!(
        matches!(
            write
                .append_record(rejected_move(&base, reasons.clone())?)
                .await,
            Err(ProjectStorageError::InvalidRecord {
                field: "reasons",
                ..
            })
        ),
        "native admission accepted a 13-reason movement"
    );
    write.rollback().await?;
    let mut corrupt = store.database.begin_write().await?;
    assert_eq!(
        sqlx::query("UPDATE moves SET outcome=? WHERE move_id=?")
            .bind(serde_json::to_string(&MoveOutcome::Rejected { reasons })?)
            .bind(valid.reference().id.as_str())
            .execute(&mut corrupt)
            .await?
            .rows_affected(),
        1
    );
    corrupt.commit().await?;
    let mut read = store.snapshot().await?;
    assert!(
        matches!(
            read.record(&valid.reference()).await,
            Err(ProjectStorageError::InvalidRecord {
                field: "reasons",
                ..
            })
        ),
        "native decoding accepted a 13-reason movement"
    );
    read.finish().await?;
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn non_move_rejection_shape_is_checked_at_native_admission_and_decode() -> TestResult {
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
        reasons: vec![Refusal::Archived, Refusal::DefinitionConflict],
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
