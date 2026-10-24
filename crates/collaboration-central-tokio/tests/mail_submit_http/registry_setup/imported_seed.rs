//! Publish a real seeded Copy for the imported-Claim session scenario.
use super::{ConformanceResult, Copy};
use fleet_sync_protocol::*;
use serde_json::{Value, json};

pub(super) async fn publish(
    copy: &Copy,
    receiver: ReceiverId,
    database: DatabaseId,
) -> ConformanceResult<()> {
    let sql = "CREATE TABLE claim_rows(id TEXT NOT NULL PRIMARY KEY,value TEXT NOT NULL)";
    let schema = SchemaIdentity::new(
        1,
        schema_fingerprint(&[SchemaEntry {
            kind: SchemaObjectKind::Table,
            name: "claim_rows".into(),
            sql: sql.into(),
        }])?,
    )?;
    let row = vec![
        SqlValue::Text("019f0000-0000-7000-8000-000000005510".into()),
        SqlValue::Text("imported Claim row".into()),
    ];
    let position = ReplicationPosition::new(Epoch::new(1)?, CommitSequence::new(1));
    let contents = SnapshotPage {
        database,
        position,
        schema: schema.clone(),
        schema_steps: vec![sql.into()],
        table: "claim_rows".into(),
        rows: vec![row],
        next: None,
    };
    publish_snapshot(copy, receiver, contents).await
}

pub(super) async fn publish_normal_rows(
    copy: &Copy,
    receiver: ReceiverId,
    database: DatabaseId,
) -> ConformanceResult<()> {
    let sql = "CREATE TABLE records(record_key TEXT NOT NULL PRIMARY KEY, id INTEGER NOT NULL UNIQUE, label TEXT NOT NULL UNIQUE)";
    let schema = SchemaIdentity::new(
        1,
        schema_fingerprint(&[SchemaEntry {
            kind: SchemaObjectKind::Table,
            name: "records".into(),
            sql: sql.into(),
        }])?,
    )?;
    let contents = SnapshotPage {
        database,
        position: ReplicationPosition::new(Epoch::new(1)?, CommitSequence::new(3)),
        schema,
        schema_steps: vec![sql.into()],
        table: "records".into(),
        rows: (1..=3)
            .map(|id| {
                Ok(vec![
                    SqlValue::Text(super::normal_record_key(id)?.into()),
                    SqlValue::Integer(id),
                    SqlValue::Text(format!("imported-{id}")),
                ])
            })
            .collect::<ConformanceResult<_>>()?,
        next: None,
    };
    publish_snapshot(copy, receiver, contents).await
}

async fn publish_snapshot(
    copy: &Copy,
    receiver: ReceiverId,
    contents: SnapshotPage,
) -> ConformanceResult<()> {
    let database = contents.database;
    let position = contents.position;
    let schema = contents.schema.clone();
    let table = TableName::new(contents.table.clone())?;
    let mut table_digest = TableDigestBuilder::new();
    for row in &contents.rows {
        table_digest.push_row(row)?;
    }
    let attempt = SeedAttemptId::new(durable_steps::Uuid::now_v7())?;
    let ReplicationAnswer::SeedStarted { seed } = invoke(
        copy,
        receiver,
        database,
        json!({"kind":"begin","value":{"database":database,"attempt":attempt}}),
    )
    .await?
    else {
        return Err("imported Claim seed did not start".into());
    };
    let page = SeedPage {
        seed,
        page: 0,
        digest: PageDigest::of(&contents)?,
        contents,
    };
    if invoke(
        copy,
        receiver,
        database,
        json!({"kind":"page","value":page}),
    )
    .await?
        != ReplicationAnswer::SeedPage(SeedPageAnswer::Staged)
    {
        return Err("imported Claim seed page was not staged".into());
    }
    let done = SeedComplete {
        database,
        seed,
        position,
        pages: 1,
        schema,
        table_digests: vec![(table, table_digest.finish())],
    };
    let answer = invoke(
        copy,
        receiver,
        database,
        json!({"kind":"complete","value":{"done":done,"summary":null}}),
    )
    .await?;
    if answer != ReplicationAnswer::SeedComplete(SeedAnswer::Published { position }) {
        return Err(format!("imported Claim seed was not published: {answer:?}").into());
    }
    Ok(())
}

async fn invoke(
    copy: &Copy,
    receiver: ReceiverId,
    database: DatabaseId,
    request: Value,
) -> ConformanceResult<ReplicationAnswer> {
    // This is the existing verified-edge Copy call seam, not HTTP import orchestration.
    let answer = copy.on_call(serde_json::to_vec(&json!({
        "source":{"kind":"registry"},"sourceFace":"edge",
        "authority":{"kind":"owner","value":"fixture@example.test"},
        "destination":{"kind":"copy","value":database},"destinationFace":"copy",
        "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"imported_claim_seed"},
        "kind":"central.copy-admin-seed","target":{"copy":receiver,"database":database},"request":request
    }))?).await.map_err(|error| format!("imported Claim seed: {error:?}"))?;
    Ok(serde_json::from_slice(&answer)?)
}
