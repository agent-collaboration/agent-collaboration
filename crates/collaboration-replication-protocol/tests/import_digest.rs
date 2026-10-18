#![allow(clippy::expect_used)]
use collaboration_replication_protocol::*;
fn header() -> ImportSnapshotHeader {
    ImportSnapshotHeader {
        project: ProjectId::new("019f0000-0000-7000-8000-000000000002").expect("project"),
        head: ReplicationPosition::new(Epoch::new(1).expect("epoch"), 7.into()),
        schema: SchemaIdentity::new(2, "schema".into()).expect("schema"),
        schema_steps: vec!["CREATE TABLE records(id INTEGER PRIMARY KEY)".into()],
    }
}
fn summary() -> ProjectSummary {
    ProjectSummary {
        status: "active".into(),
        latest_record_at: chrono::DateTime::parse_from_rfc3339("2026-10-15T00:00:00Z")
            .expect("time")
            .to_utc(),
    }
}
#[test]
fn import_identity_is_exact_logical_content_and_ignores_transport_pages() {
    let first = vec![SqlValue::Integer(i64::MAX)];
    let second = vec![SqlValue::Real(1.0)];
    let mut direct = ImportDigestBuilder::new(&header()).expect("header");
    direct.append_row("records", &first).expect("row");
    direct.append_row("records", &second).expect("row");
    let digest = direct.finish(&summary()).expect("digest");
    // Independent Python hashlib/JSON fixture, including i64::MAX and REAL 1.0.
    assert_eq!(
        digest.as_str(),
        "9a9ed6b68347e8f9a7ba9c4754c4fe418809e2b75e7c257b568441b35162cc47"
    );
    let mut paged = ImportDigestBuilder::new(&header()).expect("header");
    for rows in [vec![first.clone()], vec![second]] {
        for row in rows {
            paged.append_row("records", &row).expect("row");
        }
    }
    assert_eq!(digest, paged.finish(&summary()).expect("paged"));
    let mut changed = ImportDigestBuilder::new(&header()).expect("header");
    changed
        .append_row("records", &[SqlValue::Integer(i64::MAX - 1)])
        .expect("row");
    changed
        .append_row("records", &[SqlValue::Real(1.0)])
        .expect("same second row");
    assert_ne!(digest, changed.finish(&summary()).expect("changed"));
    let mut changed_summary = summary();
    changed_summary.status = "closed".into();
    let mut builder = ImportDigestBuilder::new(&header()).expect("header");
    builder.append_row("records", &first).expect("row");
    builder
        .append_row("records", &[SqlValue::Real(1.0)])
        .expect("same second row");
    assert_ne!(digest, builder.finish(&changed_summary).expect("summary"));
    assert_eq!(
        serde_json::from_str::<ImportDigest>(&serde_json::to_string(&digest).expect("encode"))
            .expect("decode"),
        digest
    );
    assert!(ImportDigest::new("fake".into()).is_err());
    let mut builder = ImportDigestBuilder::new(&header()).expect("header");
    assert!(
        builder
            .append_row("records", &[SqlValue::Real(f64::NAN)])
            .is_err()
    );
}
