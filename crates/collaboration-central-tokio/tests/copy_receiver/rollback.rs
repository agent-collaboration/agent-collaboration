use super::*;
use fleet_sync_protocol::CopyStatusAnswer;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_failure_after_hook_write_rolls_back_rows_ack_summary_and_admitted_follow_ups()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof=async {
        fixture.create(false).await?;
        let piece=schema_piece(fixture.database)?;
        fixture.copy_sql("CREATE TRIGGER reject_piece_receipt BEFORE INSERT ON _fleet_sync_receipts BEGIN SELECT RAISE(ABORT,'late receipt failure'); END").await?;
        let failed=fixture.apply(piece.clone()).await?;
        assert!(!matches!(failed,ReplicationAnswer::Applied{..}|ReplicationAnswer::AlreadyApplied{..}));
        assert_eq!(fixture.counts().await?,(0,0,0));
        assert_eq!(fixture.runtime.copy_summary().await?,None);
        let CopyStatusAnswer::Status(status)=fixture.runtime.read_copy_status().await.map_err(|e|format!("{e:?}"))? else {return Err("copy disappeared".into());};
        assert_eq!(status.acked,position(0)?);
        assert!(fixture.io.calls.lock().map_err(|_|"calls poisoned")?.is_empty());
        fixture.copy_sql("DROP TRIGGER reject_piece_receipt").await?;
        assert_eq!(fixture.apply(piece.clone()).await?,ReplicationAnswer::Applied{through:position(1)?});
        assert_eq!(fixture.counts().await?,(1,1,1));
        let before=fixture.runtime.copy_summary().await?;
        assert_eq!(fixture.apply(piece).await?,ReplicationAnswer::AlreadyApplied{through:position(1)?});
        assert_eq!(fixture.runtime.copy_summary().await?,before);
        assert_eq!(fixture.counts().await?,(1,1,1));
        fixture.runtime.on_alarm().await?;
        assert_eq!(fixture.summary(true).await?,fixture.summary(false).await?);
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    fixture.objects.close().await?;
    proof
}
