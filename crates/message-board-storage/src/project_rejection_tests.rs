use crate::{ProjectLedgerReader, ProjectStore};
use message_board::*;

#[tokio::test]
async fn rejection_voids_only_accepted_requests_and_orphan_reader_keeps_the_decisions()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let path = directory.path().join("project.orphaned.sqlite");
    let base = crate::project_store_tests::project_record(project.clone());
    let accepted = RequestId::generate();
    let rejected = RequestId::generate();
    let rejection = Refusal::Archived;
    let ledger = |id: RequestId, decision| {
        Record::Request(RequestLedgerEntry {
            envelope: RecordEnvelope {
                id: id.clone().into(),
                request: Some(id.clone()),
                ..base.envelope().clone()
            },
            request_id: id,
            canonical_hash: CanonicalHash::try_from("a".repeat(64)).unwrap(),
            observed_epoch: None,
            accepted_in: Some(EpochNumber::try_from(1).unwrap()),
            decision,
            resubmits: None,
        })
    };
    let store = ProjectStore::open(&path, project.clone())
        .await
        .map_err(|error| format!("open fixture: {error}"))?;
    let mut write = store.begin_admission().await?;
    write
        .append_record(base.clone())
        .await
        .map_err(|error| format!("append project: {error}"))?;
    write
        .append_record(ledger(
            accepted.clone(),
            RequestDecision::Accepted {
                records: vec![],
                positions: vec![],
                change: ChangeOutcome::AlreadySo,
            },
        ))
        .await
        .map_err(|error| format!("append accepted ledger: {error}"))?;
    write
        .append_record(ledger(
            rejected.clone(),
            RequestDecision::Rejected {
                reasons: vec![rejection.clone()],
            },
        ))
        .await
        .map_err(|error| format!("append rejected ledger: {error}"))?;
    write.commit().await?;
    let cause = VoidCause::ProjectRejected {
        reason: Box::new(rejection.clone()),
    };
    let mut write = store.begin_admission().await?;
    assert_eq!(
        write
            .void_accepted_requests(
                cause.clone(),
                base.envelope().actor.clone(),
                base.envelope().router_time
            )
            .await
            .map_err(|error| format!("void accepted ledger: {error}"))?
            .len(),
        1
    );
    write.commit().await?;
    let before = store.snapshot().await?.high_water();
    store.close().await;
    let reader = ProjectLedgerReader::open(&path, project)
        .await
        .map_err(|error| format!("open orphan reader: {error}"))?;
    assert_eq!(
        reader
            .ledger(&accepted)
            .await
            .map_err(|error| format!("read accepted ledger: {error}"))?
            .unwrap()
            .decision,
        RequestDecision::Voided { cause }
    );
    assert_eq!(
        reader
            .ledger(&rejected)
            .await
            .map_err(|error| format!("read rejected ledger: {error}"))?
            .unwrap()
            .decision,
        RequestDecision::Rejected {
            reasons: vec![rejection.clone()]
        }
    );
    assert_eq!(reader.rejection_reason().await?, Some(rejection));
    assert_eq!(reader.high_water().await?, before);
    reader.close().await;
    Ok(())
}

#[tokio::test]
async fn voiding_pages_accepted_operands_and_ignores_corrupt_rejected_history()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use sqlx::{ConnectOptions, Connection};
    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let path = directory.path().join("paged-rejection.sqlite");
    let base = crate::project_store_tests::project_record(project.clone());
    let cause = VoidCause::ProjectRejected {
        reason: Box::new(Refusal::Archived),
    };
    let ledger = |id: RequestId, decision| {
        Record::Request(RequestLedgerEntry {
            envelope: RecordEnvelope {
                id: id.clone().into(),
                request: Some(id.clone()),
                ..base.envelope().clone()
            },
            request_id: id,
            canonical_hash: CanonicalHash::try_from("a".repeat(64)).unwrap(),
            observed_epoch: None,
            accepted_in: Some(EpochNumber::try_from(1).unwrap()),
            decision,
            resubmits: None,
        })
    };
    let store = ProjectStore::open(&path, project.clone()).await?;
    let mut write = store.begin_admission().await?;
    write.append_record(base.clone()).await?;
    let mut accepted = Vec::new();
    for _ in 0..105 {
        let id = RequestId::generate();
        accepted.push(id.clone());
        write
            .append_record(ledger(
                id,
                RequestDecision::Accepted {
                    records: Vec::new(),
                    positions: Vec::new(),
                    change: ChangeOutcome::AlreadySo,
                },
            ))
            .await?;
    }
    let rejected = RequestId::generate();
    let corrupt = RequestId::generate();
    for id in [&rejected, &corrupt] {
        write
            .append_record(ledger(
                id.clone(),
                RequestDecision::Rejected {
                    reasons: vec![Refusal::Archived],
                },
            ))
            .await?;
    }
    write
        .append_record(ledger(
            RequestId::generate(),
            RequestDecision::Voided {
                cause: cause.clone(),
            },
        ))
        .await?;
    write.commit().await?;
    let mut observer = sqlx_turso::TursoConnectOptions::new()
        .filename(&path)
        .connect()
        .await?;
    let changed = sqlx::query("UPDATE record_identities SET payload = json_set(payload, '$.record.canonicalHash', '') WHERE kind = 'request' AND id = ?")
        .bind(corrupt.as_str()).execute(&mut observer).await?;
    assert_eq!(changed.rows_affected(), 1);
    observer.close().await?;
    let mut write = store.begin_admission().await?;
    assert_eq!(
        write
            .void_accepted_requests(
                cause.clone(),
                base.envelope().actor.clone(),
                base.envelope().router_time
            )
            .await?
            .len(),
        105
    );
    write.commit().await?;
    let mut read = store.snapshot().await?;
    for id in accepted {
        let Some(Record::Request(ledger)) = read.record(&id.into()).await? else {
            panic!("accepted ledger missing");
        };
        assert_eq!(
            ledger.decision,
            RequestDecision::Voided {
                cause: cause.clone()
            }
        );
    }
    let Some(Record::Request(ledger)) = read.record(&rejected.clone().into()).await? else {
        panic!("rejected ledger missing");
    };
    assert_eq!(
        ledger.decision,
        RequestDecision::Rejected {
            reasons: vec![Refusal::Archived]
        }
    );
    assert!(read.record(&corrupt.clone().into()).await.is_err());
    read.finish().await?;
    store.close().await;
    let reader = ProjectLedgerReader::open(&path, project).await?;
    assert_eq!(reader.rejection_reason().await?, Some(Refusal::Archived));
    assert_eq!(
        reader.ledger(&rejected).await?.unwrap().decision,
        RequestDecision::Rejected {
            reasons: vec![Refusal::Archived]
        }
    );
    assert!(reader.ledger(&corrupt).await.is_err());
    reader.close().await;
    Ok(())
}
