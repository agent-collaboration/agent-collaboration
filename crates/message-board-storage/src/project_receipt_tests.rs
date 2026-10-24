//! Native receipt history survives current-row changes and request-scoped paging.
use crate::{ProjectStorageError, ProjectStore, project_store_tests::project_record};
use message_board::*;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn ledger(
    base: &Record,
    request: RequestId,
    decision: StoredDecision,
) -> TestResult<RequestLedgerEntry> {
    Ok(RequestLedgerEntry {
        envelope: RecordEnvelope {
            id: request.clone().into(),
            request: Some(request.clone()),
            ..base.envelope().clone()
        },
        request_id: request,
        canonical_hash: CanonicalHash::try_from("a".repeat(64))?,
        accepted_in: matches!(decision, StoredDecision::Accepted { .. })
            .then_some(EpochNumber::INITIAL),
        decision,
    })
}

#[tokio::test]
async fn project_receipt_pages_keep_every_historical_version_after_update_and_reopen() -> TestResult
{
    // Arrange: the same record has 513 effects under one request, interleaved
    // with another request's rows so positions cannot be treated as ordinals.
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("project.sqlite");
    let project = ProjectId::generate();
    let store = ProjectStore::open(&path, project.clone()).await?;
    let base = project_record(project.clone());
    let request = RequestId::generate();
    let other = RequestId::generate();
    let mut origin = crate::project_test_support::origin();
    origin.machine = MachineLabel::try_from("\"".repeat(256))?;
    let mut write = store.begin_admission(origin.clone()).await?;
    let mut expected = Vec::new();
    let mut foreign_position = None;
    for index in 0..513 {
        let mut version = base.clone();
        version.envelope_mut().request = Some(request.clone());
        let cursor = write.append_record(version).await?;
        expected.push((base.reference(), write.record_placement(cursor)));
        if index % 17 == 0 {
            let mut unrelated = base.clone();
            unrelated.envelope_mut().request = Some(other.clone());
            foreign_position = Some(write.append_record(unrelated).await?.position);
        }
    }
    let original = ledger(
        &base,
        request.clone(),
        StoredDecision::accepted(expected.clone(), ChangeOutcome::Changed),
    )?;
    let first = write.request_decision(&original).await?;
    let ledger_position = write
        .append_record(Record::Request(original.clone()))
        .await?;
    write.commit().await?;

    // Act: current content changes under a new request; the original request
    // cannot acquire a new effect after its ledger decision has committed.
    let mut write = store.begin_admission(origin).await?;
    let mut later = base.clone();
    later.envelope_mut().request = Some(RequestId::generate());
    write.append_record(later).await?;
    write.commit().await?;
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    let mut forbidden = base.clone();
    forbidden.envelope_mut().request = Some(request.clone());
    assert!(matches!(
        write.append_record(forbidden).await,
        Err(ProjectStorageError::InvalidRecord {
            field: "decidedRequest",
            ..
        })
    ));
    write.rollback().await?;
    store.close().await;
    let store = ProjectStore::open(&path, project.clone()).await?;
    let mut read = store.snapshot().await?;
    assert_eq!(read.request_decision(&original).await?, first);
    let mut query = RequestEffectsRequest {
        scope: RequestEffectsScope::Project {
            project_id: project.clone(),
        },
        request: request.clone(),
        after: None,
    };
    let mut collected = Vec::new();
    let mut page_sizes = Vec::new();
    loop {
        let RequestEffectsRead::Decision {
            decision: RequestDecision::Accepted { effects, change },
        } = read.request_effects(&query).await?
        else {
            panic!("accepted receipt page")
        };
        assert_eq!(change, ChangeOutcome::Changed);
        assert_eq!(effects.total_count(), 513);
        assert_eq!(
            effects.digest(),
            &DecisionEffectsHeader::from_full(&expected).digest
        );
        page_sizes.push(effects.entries().len());
        collected.extend_from_slice(effects.entries());
        query.after = effects.next().cloned();
        if query.after.is_none() {
            break;
        }
    }
    // Assert: repeated IDs remain distinct effects, in original order and with
    // original enrolled origin metadata; the ledger itself is never an effect.
    assert_eq!(page_sizes, [256, 256, 1]);
    assert!(
        collected == expected,
        "receipt history lost, duplicated or reordered an effect"
    );
    for after in [
        RequestEffectsCursor::Project {
            position: foreign_position.ok_or("foreign row")?,
        },
        RequestEffectsCursor::Project {
            position: ledger_position.position,
        },
        RequestEffectsCursor::Fleet { ordinal: 0 },
    ] {
        query.after = Some(after);
        assert_eq!(
            read.request_effects(&query).await?,
            RequestEffectsRead::Refused {
                reason: RequestEffectsRefusal::CursorInvalid,
            }
        );
    }
    query.after = None;
    query.request = RequestId::generate();
    assert_eq!(
        read.request_effects(&query).await?,
        RequestEffectsRead::Refused {
            reason: RequestEffectsRefusal::UnknownRequest,
        }
    );
    query.request = request;
    query.scope = RequestEffectsScope::Project {
        project_id: ProjectId::generate(),
    };
    assert_eq!(
        read.request_effects(&query).await?,
        RequestEffectsRead::Refused {
            reason: RequestEffectsRefusal::RequestNotInScope,
        }
    );
    read.finish().await?;
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn empty_changed_already_so_and_rejected_receipts_remain_distinct() -> TestResult {
    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let store =
        ProjectStore::open(&directory.path().join("project.sqlite"), project.clone()).await?;
    let base = project_record(project.clone());
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    write.append_record(base.clone()).await?;
    let mut requests = Vec::new();
    for decision in [
        StoredDecision::accepted(Vec::new(), ChangeOutcome::Changed),
        StoredDecision::accepted(Vec::new(), ChangeOutcome::AlreadySo),
        StoredDecision::Rejected {
            reasons: vec![Refusal::Archived],
        },
    ] {
        let row = ledger(&base, RequestId::generate(), decision.clone())?;
        write.append_record(Record::Request(row.clone())).await?;
        requests.push((row.request_id, decision.into_reply(Vec::new(), None)?));
    }
    write.commit().await?;
    let mut read = store.snapshot().await?;
    for (request, expected) in requests {
        let mut query = RequestEffectsRequest {
            scope: RequestEffectsScope::Project {
                project_id: project.clone(),
            },
            request,
            after: None,
        };
        assert_eq!(
            read.request_effects(&query).await?,
            RequestEffectsRead::Decision { decision: expected }
        );
        query.after = Some(RequestEffectsCursor::Project {
            position: FeedPosition::ZERO,
        });
        assert_eq!(
            read.request_effects(&query).await?,
            RequestEffectsRead::Refused {
                reason: RequestEffectsRefusal::CursorInvalid,
            }
        );
    }
    read.finish().await?;
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn mismatched_commitment_and_noncanonical_receipt_metadata_are_refused() -> TestResult {
    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let store =
        ProjectStore::open(&directory.path().join("project.sqlite"), project.clone()).await?;
    let mut base = project_record(project.clone());
    let request = RequestId::generate();
    base.envelope_mut().request = Some(request.clone());
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    let cursor = write.append_record(base.clone()).await?;
    let incorrect = ledger(
        &base,
        request.clone(),
        StoredDecision::accepted(Vec::new(), ChangeOutcome::Changed),
    )?;
    assert!(matches!(
        write.append_record(Record::Request(incorrect)).await,
        Err(ProjectStorageError::InvalidRecord {
            field: "effects",
            ..
        })
    ));
    let correct = ledger(
        &base,
        request.clone(),
        StoredDecision::accepted(
            vec![(base.reference(), write.record_placement(cursor))],
            ChangeOutcome::Changed,
        ),
    )?;
    write.append_record(Record::Request(correct)).await?;
    write.commit().await?;
    let mut corrupt = store.database.begin_write().await?;
    assert_eq!(sqlx::query("UPDATE project_feed SET origin_machine=? WHERE request_id=? AND record_kind <> 'request'")
        .bind(" padded enrollment ").bind(request.as_str()).execute(&mut corrupt).await?.rows_affected(), 1);
    corrupt.commit().await?;
    let mut read = store.snapshot().await?;
    assert!(matches!(
        read.request_effects(&RequestEffectsRequest {
            scope: RequestEffectsScope::Project {
                project_id: project
            },
            request,
            after: None,
        })
        .await,
        Err(ProjectStorageError::InvalidFeed)
    ));
    read.finish().await?;
    store.close().await;
    Ok(())
}
