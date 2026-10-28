//! Reader marks use their actual unsequenced table, never a fabricated feed row.
use super::{
    RECORD_BYTE_BUDGET, SCHEMA_BYTE_BUDGET, TestResult, envelope, maximum_numeric_width_row,
    project_state,
};
use crate::{ProjectStore, project_test_support::origin};
use fleet_sync_protocol::SqlValue;
use message_board::*;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn maximum_reader_metadata_fits_native_rows_without_acquiring_a_feed_position() -> TestResult
{
    // Arrange: the maximum declared actor, request and timestamp travel through
    // the existing reader-state owner. Its through cursor names actual history.
    let directory = tempfile::tempdir()?;
    let state = project_state()?;
    let store = ProjectStore::open(
        &directory.path().join("project.sqlite"),
        state.project_id.clone(),
    )
    .await?;
    let reader = state
        .envelope
        .actor
        .declared_identity()
        .ok_or("reader fixture")?;
    let mut write = store.begin_admission(origin()).await?;
    let through = write.append_record(Record::Project(state.clone())).await?;
    let mut envelope = envelope(&state, RecordId::generate());
    envelope.position = RecordPlacement::Unsequenced;
    let mark = ReadMark {
        envelope,
        reader: reader.clone(),
        scope: ReadScope::Project {
            project_id: state.project_id.clone(),
        },
        through,
    };

    // Act: write and decode through the public storage owner, then inspect the
    // real native snapshot's read_marks and interned actors physical rows.
    write.write_reader_mark(mark.clone()).await?;
    assert_eq!(write.high_water().await?, through);
    write.commit().await?;
    let mut read = store.snapshot().await?;
    assert_eq!(
        read.read_mark(&reader, &mark.scope).await?,
        Some(mark.clone())
    );
    assert_eq!(
        read.records(RecordKind::ReadMark).await?,
        vec![Record::ReadMark(mark.clone())]
    );
    let feed = read
        .feed_after(
            FeedCursor {
                epoch: through.epoch,
                position: FeedPosition::ZERO,
            },
            10,
            false,
        )
        .await?;
    let FeedRead::Entries { entries, .. } = feed else {
        return Err("accepted feed missing".into());
    };
    assert_eq!(entries.len(), 1, "a reader mark must not add a feed row");
    assert_eq!(entries[0].record.kind, RecordKind::Project);
    read.finish().await?;
    let mut snapshot = store.database.snapshot().await?;
    let mut cursor = None;
    let mut schema = None;
    let mut measured = Vec::new();
    let mut marks = 0;
    let mut actors = 0;
    loop {
        let page = snapshot.page(cursor).await?;
        match &schema {
            Some(previous) => assert_eq!(previous, &page.schema_steps),
            None => schema = Some(page.schema_steps.clone()),
        }
        if matches!(page.table.as_str(), "read_marks" | "actors") {
            for row in &page.rows {
                let encoded = serde_json::to_vec(row)?;
                let decoded: Vec<SqlValue> = serde_json::from_slice(&encoded)?;
                assert_eq!(&decoded, row);
                let conservative = maximum_numeric_width_row(row)?;
                assert!(conservative <= RECORD_BYTE_BUDGET);
                measured.push(serde_json::json!({
                    "table":page.table,"nativeRowBytes":encoded.len(),
                    "conservativeNumericRowBytes":conservative
                }));
                if page.table == "read_marks" {
                    marks += 1;
                    assert!(row.iter().any(|cell| {
                        matches!(cell, SqlValue::Text(id) if id == mark.envelope.id.as_str())
                    }));
                } else {
                    actors += 1;
                }
            }
        }
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    // Assert: both physical homes were measured, and the common reconstructed
    // schema fits beside a row. No feed origin or position is invented.
    assert_eq!(marks, 1);
    assert_eq!(actors, 1);
    let schema = schema.ok_or("native snapshot schema missing")?;
    assert!(!schema.is_empty());
    let schema_bytes = serde_json::to_vec(&schema)?.len();
    assert!(schema_bytes <= SCHEMA_BYTE_BUDGET);
    eprintln!(
        "BOARD_MAXIMUM_READ_MARK_SIZES {}",
        serde_json::json!({"rows":measured,"schemaBytes":schema_bytes})
    );
    snapshot.close().await?;
    store.close().await;
    Ok(())
}
