use super::{SnapshotTransportError, reframe_import_manifest, reframe_import_page};
use fleet_protocol::{
    CommitSequence, DatabaseId, Epoch, PageDigest, ReplicationPosition, SchemaIdentity,
    SnapshotCursor, SnapshotManifest, SnapshotPage, SqlValue, TableDigestBuilder, TableManifest,
    TableName,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn source() -> Result<(SnapshotManifest, SnapshotPage), Box<dyn std::error::Error>> {
    let database = DatabaseId::new(uuid::Uuid::now_v7())?;
    let position = ReplicationPosition::new(Epoch::new(9)?, CommitSequence::new(123));
    let schema = SchemaIdentity::new(1, "source-schema".to_owned())?;
    let rows = vec![vec![
        SqlValue::Integer(9_007_199_254_740_993),
        SqlValue::Integer(i64::MIN),
        SqlValue::Real(-0.0),
        SqlValue::Blob(vec![0, 255, 1]),
        SqlValue::Text("unchanged\nrow\0".to_owned()),
        SqlValue::Null,
    ]];
    let mut digest = TableDigestBuilder::new();
    for row in &rows {
        digest.push_row(row)?;
    }
    let manifest = SnapshotManifest {
        database,
        position,
        schema: schema.clone(),
        tables: vec![TableManifest {
            table: TableName::new("project_feed")?,
            row_count: 1,
            digest: digest.finish(),
        }],
    };
    let page = SnapshotPage {
        database,
        position,
        schema,
        schema_steps: vec!["CREATE TABLE fixture (value TEXT)".to_owned()],
        table: "project_feed".to_owned(),
        rows,
        next: Some(SnapshotCursor {
            position,
            table: "request_answers".to_owned(),
            after_key: vec![SqlValue::Integer(42)],
        }),
    };
    Ok((manifest, page))
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions pin the A0 transport contract."
)]
fn manifest_reframes_only_the_epoch_and_preserves_table_hashes() -> TestResult {
    // Arrange: physical transport epoch 9 is distinct from the application L bound.
    let (source, _) = source()?;
    let mut expected = source.clone();
    expected.position = ReplicationPosition::new(Epoch::new(1)?, CommitSequence::new(123));
    // Act.
    let reframed = reframe_import_manifest(source)?;
    // Assert: database, sequence, schema, table order, row counts and hashes are exact.
    assert_eq!(reframed, expected);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions pin source and outgoing cursor separation."
)]
fn page_preserves_row_bytes_and_source_cursor_while_resealing_outgoing_headers() -> TestResult {
    // Arrange: values include integers outside JavaScript precision, binary and signed zero.
    let (manifest, page) = source()?;
    let source_cursor = page.next.clone();
    let source_rows = serde_json::to_vec(&page.rows)?;
    let old_digest = PageDigest::of(&page)?;
    let mut expected = page.clone();
    expected.position = ReplicationPosition::new(Epoch::new(1)?, CommitSequence::new(123));
    if let Some(next) = &mut expected.next {
        next.position = expected.position;
    }
    let expected_digest = PageDigest::of(&expected)?;
    // Act.
    let prepared = reframe_import_page(page, &manifest)?;
    // Assert: source paging continues at e9; every outgoing header is e1, with a new seal.
    assert_eq!(prepared.source_next, source_cursor);
    assert_eq!(prepared.contents, expected);
    assert_eq!(serde_json::to_vec(&prepared.contents.rows)?, source_rows);
    assert_eq!(prepared.digest, expected_digest);
    assert_ne!(prepared.digest, old_digest);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A different pinned read must be refused."
)]
fn pages_and_cursors_from_another_snapshot_are_refused() -> TestResult {
    // Arrange: each mismatch is independent of the source's valid position.
    let (manifest, page) = source()?;
    let other = ReplicationPosition::new(Epoch::new(9)?, CommitSequence::new(124));
    let mut wrong_page = page.clone();
    wrong_page.position = other;
    let mut wrong_cursor = page;
    if let Some(next) = &mut wrong_cursor.next {
        next.position = other;
    }
    // Act / Assert: neither wrong header can enter the prepared import.
    assert!(matches!(
        reframe_import_page(wrong_page, &manifest),
        Err(SnapshotTransportError::PageHeaderMismatch)
    ));
    assert!(matches!(
        reframe_import_page(wrong_cursor, &manifest),
        Err(SnapshotTransportError::CursorPositionMismatch)
    ));
    Ok(())
}
