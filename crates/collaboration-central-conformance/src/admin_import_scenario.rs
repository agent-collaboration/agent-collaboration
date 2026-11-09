//! Normal owner HTTP drives the real Fleet continuation and SDK-owned snapshot publication.
use crate::{ConformanceClient, ConformanceResult, owner_assertion};
use collaboration_central_core::{
    AdminImportHeader, AdminImportPhase, AdminImportRecord, AdminProjectImportAnswer,
    AdminProjectImportRequest,
};
use fleet_protocol::{
    CommitSequence, DatabaseId, Epoch, ImportDigest, PageDigest, ReplicationPosition, RequestId,
    SchemaEntry, SchemaIdentity, SchemaObjectKind, SnapshotManifest, SnapshotPage, SqlValue,
    TableDigestBuilder, TableManifest, TableName, schema_fingerprint,
};
use std::time::Duration;

const PROJECT: &str = "019f0000-0000-7000-8000-00000000ed03";
const SQL: &str = "CREATE TABLE typed_import_rows(id TEXT NOT NULL PRIMARY KEY,value TEXT NOT NULL,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL)";

pub struct AdminImportScenario {
    header: AdminImportHeader,
    import_id: RequestId,
}
impl AdminImportScenario {
    pub async fn prepare(base: &str) -> ConformanceResult<Self> {
        let (header, page) = fixture()?;
        let graph = serde_json::from_value(serde_json::json!({
            "provenance":header.provenance,"declaredOwner":header.declared_owner,
            "records":[
                {"kind":"workspace","id":"019f0000-0000-7000-8000-00000000ed01","name":"typed import workspace","description":"normal owner import proof","repositories":[],"time":{"kind":"legacyUnknown"}},
                {"kind":"board","id":"019f0000-0000-7000-8000-00000000ed02","workspace":"019f0000-0000-7000-8000-00000000ed01","name":"typed import board","description":"normal owner import proof","state":"active","time":{"kind":"legacyUnknown"}},
                {"kind":"directory","project":PROJECT,"board":"019f0000-0000-7000-8000-00000000ed02","title":"typed import project","time":{"kind":"legacyUnknown"}}
            ]
        }))?;
        let imported = ConformanceClient::new(base)?.import_fleet(&graph).await?;
        if !matches!(
            imported,
            fleet_admission::FleetImportResult::Imported {
                counts: fleet_admission::FleetImportCounts {
                    workspaces: 1,
                    boards: 1,
                    directory_entries: 1
                }
            }
        ) {
            return Err(format!("parent graph import differed: {imported:?}").into());
        }
        let begin = AdminProjectImportRequest::Begin {
            header: header.clone(),
        };
        let AdminProjectImportAnswer::Ready { import_id, seed } = settled(base, &begin).await?
        else {
            return Err("normal import did not reach Ready".into());
        };
        if settled(base, &begin).await? != (AdminProjectImportAnswer::Ready { import_id, seed }) {
            return Err("repeat Begin changed import/seed identity".into());
        }
        let expected = AdminImportRecord {
            import_id,
            header: header.clone(),
            phase: AdminImportPhase::Reserved,
        };
        require_status(base, &header, expected).await?;
        let contents = AdminProjectImportRequest::Page {
            digest: header.digest.clone(),
            import_id,
            page: 0,
            page_digest: PageDigest::of(&page)?,
            contents: page,
        };
        for _ in 0..2 {
            let staged = settled(base, &contents).await?;
            if staged != (AdminProjectImportAnswer::Staged { import_id, page: 0 }) {
                return Err(format!("normal page did not stage once: {staged:?}").into());
            }
        }
        let done = AdminProjectImportRequest::Complete {
            digest: header.digest.clone(),
            import_id,
        };
        let complete = settled(base, &done).await?;
        if complete
            != (AdminProjectImportAnswer::Imported {
                head: header.manifest.position,
            })
        {
            return Err(
                format!("normal receiver publication did not complete: {complete:?}").into(),
            );
        }
        let saved = Self { header, import_id };
        saved.require_complete(base).await?;
        if settled(base, &done).await? != AdminProjectImportAnswer::AlreadyImported {
            return Err("repeat Complete lost retained completion".into());
        }
        saved.require_complete(base).await?;
        println!(
            "ADMIN_IMPORT_COLUMNS_CASE normal_begin_page_complete_preserves_original_header_and_identity passed"
        );
        Ok(saved)
    }
    pub async fn verify_reopen(&self, base: &str) -> ConformanceResult<()> {
        self.require_complete(base).await?;
        let begin = AdminProjectImportRequest::Begin {
            header: self.header.clone(),
        };
        if settled(base, &begin).await? != AdminProjectImportAnswer::AlreadyImported {
            return Err("owning reopen lost completed Begin replay".into());
        }
        let done = AdminProjectImportRequest::Complete {
            digest: self.header.digest.clone(),
            import_id: self.import_id,
        };
        if settled(base, &done).await? != AdminProjectImportAnswer::AlreadyImported {
            return Err("owning reopen lost completed import identity".into());
        }
        self.require_complete(base).await?;
        println!(
            "ADMIN_IMPORT_COLUMNS_CASE owning_reopen_keeps_exact_completed_header_and_publication passed"
        );
        Ok(())
    }
    async fn require_complete(&self, base: &str) -> ConformanceResult<()> {
        require_status(
            base,
            &self.header,
            AdminImportRecord {
                import_id: self.import_id,
                header: self.header.clone(),
                phase: AdminImportPhase::Complete {
                    position: self.header.manifest.position,
                },
            },
        )
        .await
    }
}
fn fixture() -> ConformanceResult<(AdminImportHeader, SnapshotPage)> {
    let database = DatabaseId::try_from(PROJECT.to_owned())?;
    let position = ReplicationPosition::new(Epoch::new(1)?, CommitSequence::new(1));
    let schema = SchemaIdentity::new(
        1,
        schema_fingerprint(&[SchemaEntry {
            kind: SchemaObjectKind::Table,
            name: "typed_import_rows".into(),
            sql: SQL.into(),
        }])?,
    )?;
    let row = vec![
        SqlValue::Text("019f0000-0000-7000-8000-00000000ed04".into()),
        SqlValue::Text("literal imported content λ".into()),
        SqlValue::Integer(1_792_281_600_000),
        SqlValue::Integer(1_792_281_600_000),
    ];
    let mut table_digest = TableDigestBuilder::new();
    table_digest.push_row(&row)?;
    let page = SnapshotPage {
        database,
        position,
        schema: schema.clone(),
        schema_steps: vec![SQL.into()],
        table: "typed_import_rows".into(),
        rows: vec![row],
        next: None,
    };
    let header = AdminImportHeader {
        digest: ImportDigest::of_canonical(&page)?,
        provenance: fleet_admission::ImportProvenance {
            installation: "550e8400-e29b-41d4-a716-4466554400a9"
                .to_owned()
                .try_into()?,
            machine: "original typed import machine".to_owned().try_into()?,
        },
        declared_owner: "original typed import owner".to_owned().try_into()?,
        manifest: SnapshotManifest {
            database,
            position,
            schema,
            tables: vec![TableManifest {
                table: TableName::new("typed_import_rows")?,
                row_count: 1,
                digest: table_digest.finish(),
            }],
        },
        pages: 1,
        summary: board_sync_protocol::BoardAnnotation::ProjectSummary {
            latest_record_at: message_board::RecordTime::Router(
                "2026-11-04T10:11:12.123456789Z".parse()?,
            ),
        },
    };
    Ok((header, page))
}
async fn require_status(
    base: &str,
    header: &AdminImportHeader,
    expected: AdminImportRecord,
) -> ConformanceResult<()> {
    let actual = request(
        base,
        &AdminProjectImportRequest::Status {
            digest: header.digest.clone(),
        },
    )
    .await?;
    if actual != AdminProjectImportAnswer::Progress(expected) {
        return Err(
            format!("normal import status changed immutable header or phase: {actual:?}").into(),
        );
    }
    Ok(())
}
async fn settled(
    base: &str,
    command: &AdminProjectImportRequest,
) -> ConformanceResult<AdminProjectImportAnswer> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let answer = request(base, command).await?;
            if answer != AdminProjectImportAnswer::Pending {
                return Ok(answer);
            }
            tokio::task::yield_now().await;
        }
    })
    .await?
}
async fn request(
    base: &str,
    command: &AdminProjectImportRequest,
) -> ConformanceResult<AdminProjectImportAnswer> {
    let response = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?
        .post(format!("{base}/admin/v1/import/projects/{PROJECT}"))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(command)
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        return Err(format!("normal import HTTP {status}: {body}").into());
    }
    Ok(serde_json::from_str(&body)?)
}
