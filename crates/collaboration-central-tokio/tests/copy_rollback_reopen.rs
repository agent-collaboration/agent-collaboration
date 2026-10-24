//! Late row failure cannot publish an acknowledgement or survive as a partial copy.
#![cfg(feature = "conformance")]
#![allow(clippy::panic_in_result_fn)]

#[path = "copy_receiver/mod.rs"]
#[allow(
    dead_code,
    reason = "Only the real copy fixture is reused for this reopen case."
)]
mod support;

use board_sync_protocol::BoardAnnotation;
use fleet_sync_protocol::{
    CopyStatusAnswer, CopyUnavailableReason, Piece, PieceContents, ReplicatedTransaction,
    ReplicationAnswer, RowChange, SnapshotRead, SqlValue,
};
use serde_json::json;
use support::{Fixture, TestResult, position};

#[tokio::test]
async fn a_late_constraint_error_rolls_back_rows_and_acknowledgement_across_reopen() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof = async {
        fixture.create(false).await?;
        let schema = Piece::<BoardAnnotation>::new(PieceContents {
            database: fixture.database,
            epoch: position(0)?.epoch,
            continues_from: position(0)?,
            through: position(1)?,
            transactions: vec![ReplicatedTransaction::SchemaStep {
                commit_id: position(1)?.sequence,
                migration_version: 1,
                statements: vec![
                    "CREATE TABLE records(id TEXT NOT NULL PRIMARY KEY,label TEXT NOT NULL UNIQUE)"
                        .into(),
                ],
            }],
            part: None,
            annotation: None,
        })?;
        let initialized = fixture.apply(schema).await?;
        if initialized
            != (ReplicationAnswer::Applied {
                through: position(1)?,
            })
        {
            return Err(format!("schema was not applied: {initialized:?}").into());
        }
        let changes = [
            "019f0000-0000-7000-8000-000000000b31",
            "019f0000-0000-7000-8000-000000000b32",
        ]
        .into_iter()
        .map(|identity| {
            RowChange::upsert(
                "records",
                vec![SqlValue::Text(identity.into())],
                vec![
                    SqlValue::Text(identity.into()),
                    SqlValue::Text("duplicate".into()),
                ],
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
        let piece = Piece::<BoardAnnotation>::new(PieceContents {
            database: fixture.database,
            epoch: position(0)?.epoch,
            continues_from: position(1)?,
            through: position(2)?,
            transactions: vec![ReplicatedTransaction::Rows {
                commit_id: position(2)?.sequence,
                rows: changes,
            }],
            part: None,
            annotation: None,
        })?;
        let failed = fixture.apply(piece).await?;
        if !matches!(failed, ReplicationAnswer::CopyUnavailable {
            reason: CopyUnavailableReason::Damaged { ref detail }
        } if detail.to_ascii_lowercase().contains("unique"))
        {
            return Err(
                format!("the real late uniqueness failure was not reached: {failed:?}").into(),
            );
        }
        unchanged_copy(&fixture).await
    }
    .await;
    fixture.objects.close().await?;
    proof?;
    drop(fixture);

    let reopened = Fixture::open(root.path()).await?;
    let proof = unchanged_copy(&reopened).await;
    reopened.objects.close().await?;
    proof
}

async fn unchanged_copy(fixture: &Fixture) -> TestResult {
    let CopyStatusAnswer::Status(status) = fixture
        .runtime
        .read_copy_status()
        .await
        .map_err(|error| format!("copy status did not answer: {error:?}"))?
    else {
        return Err("the initialized copy disappeared".into());
    };
    assert_eq!(
        status.acked,
        position(1)?,
        "a failed piece advanced its acknowledgement"
    );
    let response = fixture
        .runtime
        .on_call(serde_json::to_vec(&json!({
            "source":{"kind":"connection","value":"550e8400-e29b-41d4-a716-4466554400a8"},
            "sourceFace":"session","authority":{"kind":"object"},
            "destination":{"kind":"copy","value":fixture.database},"destinationFace":"copy",
            "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"read_rollback"},
            "kind":"central.copy-session-read",
            "target":{"copy":fixture.receiver,"database":fixture.database},
            "request":{"kind":"page","cursor":null}
        }))?)
        .await
        .map_err(|error| format!("snapshot did not answer: {error:?}"))?;
    let ReplicationAnswer::SnapshotPage(SnapshotRead::Page(page)) =
        serde_json::from_slice(&response)?
    else {
        return Err("copy did not return its committed snapshot".into());
    };
    assert_eq!(page.position, position(1)?);
    assert_eq!(page.table, "records");
    assert!(
        page.rows.is_empty(),
        "the first row survived a later constraint failure"
    );
    Ok(())
}
