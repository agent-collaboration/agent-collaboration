pub mod actual_cli;
use board_legacy_upgrade::PreparedUpgrade;
use sqlx_turso::sqlx::{ConnectOptions, Connection, Executor, Row};
use std::path::Path;
type TestResult<T> = Result<T, Box<dyn std::error::Error>>;
pub async fn projection(root: &Path, projects: usize) -> TestResult<Vec<String>> {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let summaries = read(root, "fleet", "SELECT copy_summary AS value FROM project_directory WHERE copy_summary IS NOT NULL ORDER BY project_id").await?;
            if summaries.len() == projects {
                for value in &summaries {
                    let summary: collaboration_central_core::CopyDirectorySummary = serde_json::from_str(value)?;
                    let copy = format!("copy-{}", summary.database);
                    assert_eq!(read(root, &copy, "SELECT record AS value FROM _copy_summary").await?, vec![value.clone()]);
                }
                return Ok::<_,Box<dyn std::error::Error>>(summaries);
            }
            tokio::task::yield_now().await;
        }
    }).await?
}
pub const COPY_SNAPSHOT: &str = "SELECT json_object('copy',(SELECT record FROM _fleet_sync_copy),'seed',(SELECT record FROM _fleet_sync_seed),'summary',(SELECT record FROM _copy_summary),'posts',(SELECT count(*) FROM posts),'staged',(SELECT count(*) FROM _fleet_sync_seed_pages)) AS value";

pub async fn header(
    prepared: &PreparedUpgrade,
) -> TestResult<collaboration_central_core::AdminImportHeader> {
    let project = prepared.projects.first().ok_or("project missing")?;
    let report = prepared.report.projects.first().ok_or("report missing")?;
    let mut snapshot = project.database.snapshot().await?;
    let mut cursor = None;
    let mut pages = 0;
    loop {
        let page = snapshot.page(cursor).await?;
        pages += 1;
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    snapshot.close().await?;
    Ok(collaboration_central_core::AdminImportHeader {
        digest: report.digest.clone(),
        provenance: prepared.report.fleet.provenance.clone(),
        declared_owner: prepared.report.owner.clone(),
        manifest: report.manifest.clone(),
        pages,
        summary: board_sync_protocol::BoardAnnotation::ProjectSummary {
            latest_record_at: message_board::RecordTime::LegacyUnknown,
        },
    })
}

pub async fn execute(root: &Path, object: &str, statement: &'static str) -> TestResult<()> {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join(format!("{object}.db")))
        .create_if_missing(false)
        .connect()
        .await?;
    let result = connection.execute(statement).await;
    connection.close().await?;
    result?;
    Ok(())
}

pub async fn read(root: &Path, object: &str, statement: &'static str) -> TestResult<Vec<String>> {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join(format!("{object}.db")))
        .read_only(true)
        .connect()
        .await?;
    let rows = sqlx_turso::sqlx::query::<sqlx_turso::Turso>(statement)
        .fetch_all(&mut connection)
        .await?;
    let result = rows
        .iter()
        .map(|row| row.try_get("value"))
        .collect::<Result<Vec<String>, _>>()?;
    connection.close().await?;
    Ok(result)
}

pub async fn published_copies(
    root: &Path,
    prepared: &PreparedUpgrade,
    first_header: Option<&collaboration_central_core::AdminImportHeader>,
) -> TestResult<Vec<Vec<String>>> {
    let mut all = Vec::new();
    for project in &prepared.report.projects {
        let expected = first_header
            .filter(|header| header.database() == project.manifest.database)
            .map_or(&project.manifest, |header| &header.manifest);
        let copy = format!("copy-{}", project.project.as_str());
        let snapshot = read(root, &copy, COPY_SNAPSHOT).await?;
        assert_eq!(snapshot.len(), 1);
        let row: serde_json::Value =
            serde_json::from_str(snapshot.first().ok_or("copy snapshot missing")?)?;
        assert_eq!(
            row.get("posts").and_then(serde_json::Value::as_u64),
            Some(project.posts)
        );
        assert_eq!(
            row.get("staged").and_then(serde_json::Value::as_u64),
            Some(0)
        );
        let state: serde_json::Value = serde_json::from_str(
            row.get("copy")
                .and_then(serde_json::Value::as_str)
                .ok_or("copy marker missing")?,
        )?;
        assert_eq!(
            state.get("state").and_then(serde_json::Value::as_str),
            Some("following")
        );
        let status: fleet_sync_protocol::CopyStatus =
            serde_json::from_value(state.get("copy").cloned().ok_or("copy authority missing")?)?;
        assert_eq!(status.holder, None);
        assert_eq!(status.fence.value(), 1);
        assert_eq!(status.acked, expected.position);
        assert_eq!(status.schema, expected.schema);
        let summary: collaboration_central_core::CopyDirectorySummary = serde_json::from_str(
            row.get("summary")
                .and_then(serde_json::Value::as_str)
                .ok_or("summary missing")?,
        )?;
        assert_eq!(summary.position, expected.position);
        assert_eq!(
            summary.summary,
            board_sync_protocol::BoardAnnotation::ProjectSummary {
                latest_record_at: message_board::RecordTime::LegacyUnknown,
            }
        );
        all.push(snapshot);
    }
    Ok(all)
}
