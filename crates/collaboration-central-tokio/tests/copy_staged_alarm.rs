//! Staged-part alarms preserve recent data and derive their deadline after commit.
#![cfg(feature = "conformance")]
#![allow(clippy::panic_in_result_fn)]
#[path = "copy_receiver/mod.rs"]
#[allow(dead_code)]
mod support;
use board_sync_protocol::BoardAnnotation;
use durable_steps::{AtomicStore, StoreError};
use fleet_sync_protocol::*;
use fleet_sync_receiver::{CopyInspection, StagedTransaction};
use runtime_primitives::SyncTime;
use serde_json::json;
use std::sync::atomic::Ordering;
use support::{Fixture, TestResult, position};

fn part(fixture: &Fixture, index: u32) -> TestResult<Piece<BoardAnnotation>> {
    let statements = [
        "CREATE TABLE alarm_records(id TEXT NOT NULL PRIMARY KEY)",
        "CREATE INDEX alarm_records_index ON alarm_records(id)",
        "CREATE TABLE other_alarm_records(id TEXT NOT NULL PRIMARY KEY)",
    ];
    let whole = ReplicatedTransaction::SchemaStep {
        commit_id: position(1)?.sequence,
        migration_version: 1,
        statements: statements.iter().map(|value| (*value).to_owned()).collect(),
    };
    let statement = statements
        .get(usize::try_from(index)?)
        .ok_or("fixture part index")?;
    Ok(Piece::new(PieceContents {
        database: fixture.database,
        epoch: position(0)?.epoch,
        continues_from: position(0)?,
        through: position(1)?,
        transactions: vec![ReplicatedTransaction::SchemaStep {
            commit_id: position(1)?.sequence,
            migration_version: 1,
            statements: vec![(*statement).to_owned()],
        }],
        part: Some(TransactionPart::new(
            TransactionKey {
                epoch: position(0)?.epoch,
                commit_id: position(1)?.sequence,
            },
            index,
            3,
            PieceDigest::of_transaction(&whole)?,
        )?),
        annotation: None,
    })?)
}
async fn staged(fixture: &Fixture) -> TestResult<Vec<StagedTransaction>> {
    let inspection = fixture
        .copy
        .transaction(|scope| {
            Box::pin(
                async move { Ok::<_, StoreError>(fleet_sync_receiver::inspect_copy(scope).await) },
            )
        })
        .await?;
    match inspection {
        CopyInspection::Live { staged_parts, .. } => Ok(staged_parts),
        other => Err(format!("copy inspection did not remain Live: {other:?}").into()),
    }
}

#[tokio::test]
async fn a_part_younger_than_one_day_survives_and_rearms_its_original_deadline() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof = async {
        fixture.create(false).await?;
        let start = fixture.io.time.load(Ordering::SeqCst);
        assert!(matches!(
            fixture.apply(part(&fixture, 0)?).await?,
            ReplicationAnswer::PartStaged { .. }
        ));
        let before = staged(&fixture).await?;
        fixture.io.time.fetch_add(43_200_000, Ordering::SeqCst);
        fixture
            .io
            .wakes
            .lock()
            .map_err(|_| "wake observation poisoned")?
            .clear();

        fixture.runtime.on_alarm().await?;

        assert_eq!(staged(&fixture).await?, before);
        assert_eq!(
            *fixture
                .io
                .wakes
                .lock()
                .map_err(|_| "wake observation poisoned")?,
            vec![SyncTime::from_unix_millis(start + 86_400_001)]
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    fixture.objects.close().await?;
    proof
}

#[tokio::test]
async fn each_committed_part_arms_the_earliest_staged_transaction_deadline() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof = async {
        fixture.create(false).await?;
        let start = fixture.io.time.load(Ordering::SeqCst);
        fixture.io.wakes.lock().map_err(|_|"wake observation poisoned")?.clear();
        for index in 0..2 {
            assert!(matches!(fixture.apply(part(&fixture,index)?).await?,ReplicationAnswer::PartStaged {index:actual,..} if actual==index));
            fixture.io.time.fetch_add(3_600_000,Ordering::SeqCst);
        }
        let transactions = staged(&fixture).await?;
        let transaction = transactions.first().ok_or("staged transaction missing")?;
        assert_eq!(transaction.parts.len(),2);
        let deadline = SyncTime::from_unix_millis(start+86_400_001);
        assert_eq!(*fixture.io.wakes.lock().map_err(|_|"wake observation poisoned")?,vec![deadline,deadline]);
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    fixture.objects.close().await?;
    proof
}

#[tokio::test]
async fn an_alarm_discards_a_younger_transaction_below_the_actual_copy_fence() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof =
        async {
            fixture.create(false).await?;
            assert!(matches!(
                fixture.apply(part(&fixture, 0)?).await?,
                ReplicationAnswer::PartStaged { .. }
            ));
            let request = FenceRequest {
                database: fixture.database,
                transfer_id: TransferId::new(durable_steps::Uuid::now_v7())?,
                new_epoch: Epoch::new(2)?,
                expect: FenceExpectation {
                    epoch: Epoch::new(1)?,
                    acked: position(0)?,
                },
                target_schema: Some(SchemaSupport { max_version: 1 }),
                holder: PeerId::try_from("019f0000-0000-7000-8000-000000000902".to_owned())?,
            };
            let answer = fixture.runtime.on_call(serde_json::to_vec(&json!({
            "source":{"kind":"registry"},"sourceFace":"registry","authority":{"kind":"object"},
            "destination":{"kind":"copy","value":fixture.database},"destinationFace":"copy",
            "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"fence"},
            "kind":"fleet-sync.copy-fence","target":{"copy":fixture.receiver},"request":request
        }))?).await.map_err(|error|format!("real Copy fence: {error:?}"))?;
            assert!(matches!(
                serde_json::from_slice::<FenceResult>(&answer)?,
                FenceResult::Fenced { .. }
            ));
            assert_eq!(
                staged(&fixture).await?.len(),
                1,
                "the library does not discard at fence time"
            );

            fixture.runtime.on_alarm().await?;

            assert!(staged(&fixture).await?.is_empty());
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        }
        .await;
    fixture.objects.close().await?;
    proof
}
