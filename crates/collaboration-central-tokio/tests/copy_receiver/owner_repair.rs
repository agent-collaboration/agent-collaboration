//! Repair must discharge actual alarm work, not only return a success body.
use super::{
    schema_piece,
    support::{Fixture, TestResult, position},
};
use board_sync_protocol::BoardAnnotation;
use durable_steps::{AtomicStore, StoreError};
use fleet_sync_protocol::{
    Piece, PieceContents, PieceDigest, ReplicatedTransaction, ReplicationAnswer, TransactionKey,
    TransactionPart,
};
use serde_json::json;
use std::sync::atomic::Ordering;

async fn inspection(fixture: &Fixture) -> TestResult<fleet_sync_receiver::CopyInspection> {
    Ok(fixture
        .copy
        .transaction(|scope| {
            Box::pin(
                async move { Ok::<_, StoreError>(fleet_sync_receiver::inspect_copy(scope).await) },
            )
        })
        .await?)
}

#[tokio::test]
async fn owner_repair_discharges_real_directory_debt_and_expired_staged_parts() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof=async {
        fixture.create(false).await?;
        let schema=schema_piece(fixture.database)?;
        if fixture.apply(schema).await? != (ReplicationAnswer::Applied {through:position(1)?}) {
            return Err("Repair setup never committed the real annotated piece".into());
        }
        let expected=fixture.summary(false).await?.ok_or("Repair setup has no Copy summary")?;
        // This existing fixture records wake intents and forwards to the real
        // Fleet runtime/store. It never drives background alarms by itself.
        // Therefore only the owner operation below can discharge this debt.
        if fixture.summary(true).await?.is_some() || !fixture.io.calls.lock().map_err(|_|"calls poisoned")?.is_empty() {
            return Err("Repair setup already published its pending DirectoryUpdate".into());
        }
        let statements=["CREATE TABLE repair_partial(id TEXT NOT NULL PRIMARY KEY)","CREATE INDEX repair_partial_index ON repair_partial(id)"];
        let whole=ReplicatedTransaction::SchemaStep {
            commit_id:position(2)?.sequence,migration_version:2,
            statements:statements.iter().map(|statement|(*statement).to_owned()).collect(),
        };
        let first=Piece::<BoardAnnotation>::new(PieceContents {
            database:fixture.database,epoch:position(1)?.epoch,
            continues_from:position(1)?,through:position(2)?,
            transactions:vec![ReplicatedTransaction::SchemaStep {
                commit_id:position(2)?.sequence,migration_version:2,
                statements:vec![statements[0].to_owned()],
            }],
            part:Some(TransactionPart::new(TransactionKey {epoch:position(1)?.epoch,commit_id:position(2)?.sequence},0,2,PieceDigest::of_transaction(&whole)?)?),
            annotation:None,
        })?;
        if !matches!(fixture.apply(first).await?,ReplicationAnswer::PartStaged {index:0,acked,..} if acked==position(1)?) {
            return Err("Repair setup did not stage the actual incomplete transaction".into());
        }
        let fleet_sync_receiver::CopyInspection::Live {staged_parts,..}=inspection(&fixture).await? else {
            return Err("Repair setup has no public live Copy inspection".into());
        };
        if staged_parts.iter().map(|transaction|transaction.parts.len()).sum::<usize>()!=1 {
            return Err("Repair setup did not retain exactly one real staged part".into());
        }
        fixture.io.time.fetch_add(2*86_400_000,Ordering::SeqCst);
        let request=json!({
            "source":{"kind":"registry"},"sourceFace":"edge","authority":{"kind":"owner","value":"fixture@example.test"},
            "destination":{"kind":"copy","value":fixture.database},"destinationFace":"copy",
            "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"owner_repair_owed_duties"},
            "kind":"central.copy-operations","target":{"database":fixture.database},"request":{"kind":"repair"}
        });
        let reply=fixture.runtime.on_call(serde_json::to_vec(&request)?).await.map_err(|error|format!("real owner Repair failed: {error:?}"))?;
        if serde_json::from_slice::<serde_json::Value>(&reply)?!=json!({"kind":"repaired"}) {
            return Err("owner Repair lost its typed answer".into());
        }
        let fleet_sync_receiver::CopyInspection::Live {status,staged_parts,..}=inspection(&fixture).await? else {
            return Err("Repair damaged the actual Copy".into());
        };
        let directory=fixture.summary(true).await?;
        let calls=fixture.io.calls.lock().map_err(|_|"calls poisoned")?.len();
        let mut missing=Vec::new();
        if !staged_parts.is_empty() {missing.push("Repair left its expired staged part");}
        if directory.as_ref()!=Some(&expected) || calls!=1 {missing.push("Repair did not resend the pending DirectoryUpdate to the real Fleet owner");}
        if status.acked!=position(1)? || status.schema.version!=1 {missing.push("Repair changed the committed project prefix");}
        if fixture.summary(false).await?.as_ref()!=Some(&expected) {missing.push("Repair rewrote the original summary/time");}
        if !missing.is_empty() {return Err(format!("Repair skipped owed duties: {missing:?}").into());}
        // Move beyond any ordinary retry delay. An acknowledged follow-up is
        // discharged: another real owner Repair must cause no second RPC.
        fixture.io.time.fetch_add(60_000,Ordering::SeqCst);
        let mut repeated=request;
        repeated["identity"]["operation"]=json!(durable_steps::Uuid::now_v7());
        fixture.runtime.on_call(serde_json::to_vec(&repeated)?).await.map_err(|error|format!("repeated Repair failed: {error:?}"))?;
        if fixture.io.calls.lock().map_err(|_|"calls poisoned")?.len()!=1 || fixture.summary(true).await?.as_ref()!=Some(&expected) {
            return Err("Repair did not durably acknowledge its directory resend".into());
        }
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    fixture.objects.close().await?;
    proof
}
