use super::*;
use fleet_sync_protocol::{CopyStatusAnswer, CopyUnavailableReason, SeedAnswer};

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn ordinary_seed_keeps_summary_absent_until_an_annotated_piece() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof = async {
        let done = fixture.prepare_seed(false).await?;
        assert_eq!(fixture.runtime.copy_summary().await?, None);
        assert_eq!(
            fixture.complete_seed(false, &done, None).await?,
            ReplicationAnswer::SeedComplete(SeedAnswer::Published {
                position: position(1)?
            })
        );
        fixture.runtime.on_alarm().await?;
        assert_eq!(fixture.runtime.copy_summary().await?, None);
        assert_eq!(fixture.summary(true).await?, None);
        let CopyStatusAnswer::Status(status) = fixture
            .runtime
            .read_copy_status()
            .await
            .map_err(|e| format!("{e:?}"))?
        else {
            return Err("published seed is not readable".into());
        };
        assert_eq!(status.acked, position(1)?);
        assert_eq!(fixture.counts().await?, (1, 0, 0));
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
            annotation: Some(BoardAnnotation::ProjectSummary {
                latest_record_at: RecordTime::LegacyUnknown,
            }),
        })?;
        assert_eq!(
            fixture.apply(piece).await?,
            ReplicationAnswer::Applied {
                through: position(2)?
            }
        );
        fixture.runtime.on_alarm().await?;
        assert!(fixture.runtime.copy_summary().await?.is_some());
        assert_eq!(fixture.summary(true).await?, fixture.summary(false).await?);
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    fixture.objects.close().await?;
    proof
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn admin_seed_summary_and_follow_up_share_publication_and_never_rewrite_on_replay()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof=async {
        let done=fixture.prepare_seed(true).await?;
        let summary=BoardAnnotation::ProjectSummary{ latest_record_at:RecordTime::LegacyUnknown};
        fixture.copy_sql("CREATE TRIGGER reject_snapshot_summary BEFORE INSERT ON _copy_summary BEGIN SELECT RAISE(ABORT,'publication summary unavailable'); END").await?;
        let failed=fixture.complete_seed(true,&done,Some(summary.clone())).await?;
        assert!(matches!(failed,ReplicationAnswer::CopyUnavailable {reason:CopyUnavailableReason::Damaged {..}}),"snapshot SQL failure lost its storage class: {failed:?}");
        assert_eq!(fixture.counts().await?,(0,0,0));
        assert_eq!(fixture.runtime.read_copy_status().await.map_err(|e|format!("{e:?}"))?,CopyStatusAnswer::NotInitialized);
        assert!(fixture.io.calls.lock().map_err(|_|"calls poisoned")?.is_empty());
        fixture.copy_sql("DROP TRIGGER reject_snapshot_summary").await?;
        assert_eq!(fixture.complete_seed(true,&done,Some(summary.clone())).await?,ReplicationAnswer::SeedComplete(SeedAnswer::Published{position:position(1)?}));
        assert_eq!(fixture.counts().await?,(1,1,1));
        let before=fixture.runtime.copy_summary().await?.ok_or("admin summary missing")?;
        assert_eq!(before.summary,summary);
        assert_eq!(before.position,position(1)?);
        assert_eq!(fixture.complete_seed(true,&done,None).await?,ReplicationAnswer::SeedComplete(SeedAnswer::Published{position:position(1)?}));
        assert_eq!(fixture.runtime.copy_summary().await?,Some(before));
        assert_eq!(fixture.counts().await?,(1,1,1));
        fixture.runtime.on_alarm().await?;
        assert_eq!(fixture.summary(true).await?,fixture.summary(false).await?);
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    fixture.objects.close().await?;
    proof
}
