use super::*;
use std::sync::atomic::Ordering;

#[tokio::test]
async fn lost_directory_ack_reopens_with_same_call_and_never_overwrites_a_newer_summary()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    fixture.create(false).await?;
    assert_eq!(
        fixture.apply(schema_piece(fixture.database)?).await?,
        ReplicationAnswer::Applied {
            through: position(1)?
        }
    );
    fixture.io.lose_answer.store(true, Ordering::SeqCst);
    fixture.runtime.on_alarm().await?;
    let first = fixture
        .io
        .calls
        .lock()
        .map_err(|_| "calls poisoned")?
        .first()
        .cloned()
        .ok_or("no actual Fleet call")?;
    assert!(fixture.summary(true).await?.is_some());
    fixture.objects.close().await?;
    drop(fixture);

    let fixture = Fixture::open(root.path()).await?;
    let proof = async {
        let annotation = BoardAnnotation::ProjectSummary {
            latest_record_at: RecordTime::LegacyUnknown,
        };
        let piece = Piece::new(PieceContents {
            database: fixture.database,
            epoch: position(0)?.epoch,
            continues_from: position(1)?,
            through: position(2)?,
            transactions: vec![ReplicatedTransaction::Rows {
                commit_id: position(2)?.sequence,
                rows: vec![],
            }],
            part: None,
            annotation: Some(annotation.clone()),
        })?;
        assert_eq!(
            fixture.apply(piece).await?,
            ReplicationAnswer::Applied {
                through: position(2)?
            }
        );
        fixture.runtime.on_alarm().await?;
        let latest = fixture
            .summary(true)
            .await?
            .ok_or("newer summary not published")?;
        assert_eq!(
            latest.get("position"),
            Some(&serde_json::to_value(position(2)?)?)
        );
        assert_eq!(
            latest.get("summary"),
            Some(&serde_json::to_value(annotation)?)
        );
        fixture.io.time.fetch_add(60_000, Ordering::SeqCst);
        fixture.runtime.on_alarm().await?;
        let calls = fixture
            .io
            .calls
            .lock()
            .map_err(|_| "calls poisoned")?
            .clone();
        assert_eq!(calls.last(), Some(&first));
        assert_eq!(fixture.summary(true).await?, Some(latest.clone()));
        assert_eq!(fixture.summary(false).await?, Some(latest));
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    fixture.objects.close().await?;
    proof
}
