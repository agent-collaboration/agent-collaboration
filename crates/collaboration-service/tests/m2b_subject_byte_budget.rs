//! Byte-truncated threads and subject bodies stay opaque until selected for output.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError};
use message_board::*;
use serde_json::{Value, json};
use sqlx::{ConnectOptions, Connection};

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn byte_truncated_threads_and_subjects_are_not_decoded_until_returned()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let template = read
        .record(&project.clone().into())
        .await?
        .ok_or("project body")?;
    read.finish().await?;
    let mut write = store.begin_admission(fixture.record_origin()?).await?;
    let mut last = None;
    for index in 1..=24 {
        let thread = ThreadId::try_from(format!("019aaa00-0000-7000-8000-{index:012x}"))?;
        let row = WorkTrailRowId::generate();
        let mut envelope = template.envelope().clone();
        envelope.request = None;
        envelope.id = row.clone().into();
        write
            .append_record(Record::WorkTrailRow(WorkTrailRow {
                envelope: envelope.clone(),
                row_id: row.clone(),
                project_id: project.clone(),
                subject: TrailSubject::Project,
                text: "x".repeat(65_000).try_into()?,
                tags: Vec::new(),
                links: Vec::new(),
                redacted: false,
            }))
            .await?;
        envelope.id = thread.clone().into();
        let record = Record::Thread(Thread {
            envelope,
            thread_id: thread.clone(),
            about: ThreadSubject::WorkTrailRow(row.clone()),
            imported: false,
            state: ThreadState::Open,
            orchestrator: None,
            implementer: None,
        });
        write.append_record(record.clone()).await?;
        last = Some((thread, row));
    }
    write.commit().await?;
    let (thread, row) = last.ok_or("last fixture")?;
    let mut observer = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    let request = json!({"workspaceId":fixture.workspace,"reader":fixture.actor,"watchedOnly":false,"page":{"limit":100,"cursor":null}});
    for (kind, id) in [("thread", thread.as_str()), ("workTrailRow", row.as_str())] {
        if kind == "workTrailRow" {
            sqlx::query("UPDATE threads SET state=? WHERE thread_id=?")
                .bind(serde_json::to_string(&ThreadState::Open)?)
                .bind(thread.as_str())
                .execute(&mut observer)
                .await?;
        }
        let statement = if kind == "thread" {
            "UPDATE threads SET state='unsupportedState' WHERE thread_id=?"
        } else {
            "UPDATE work_trail_rows SET text='' WHERE row_id=?"
        };
        let changed = sqlx::query(statement)
            .bind(id)
            .execute(&mut observer)
            .await?;
        assert_eq!(
            changed.rows_affected(),
            1,
            "fault must reach the off-page body"
        );
        let page = fixture.api.call("thread_list", request.clone()).await?;
        let records = page
            .pointer("/result/page/records")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("{kind} initial page: {page}"))?;
        assert!(
            !records.is_empty() && records.len() < 24,
            "byte budget must truncate the actual page"
        );
        assert!(
            records
                .iter()
                .all(|record| record.get("threadId") != Some(&json!(thread)))
        );
        let cursor = page
            .pointer("/result/page/nextCursor")
            .and_then(Value::as_str)
            .ok_or("continuation")?;
        let mut continuation = request.clone();
        *continuation
            .pointer_mut("/page/cursor")
            .ok_or("cursor field")? = json!(cursor);
        let next = fixture.api.call("thread_list", continuation).await?;
        assert_eq!(
            next.pointer("/error/data/kind"),
            Some(&json!("invalidRecord")),
            "returned body must be validated: {next}"
        );
        let mut read = store.snapshot().await?;
        let reference = if kind == "thread" {
            thread.clone().into()
        } else {
            row.clone().into()
        };
        assert!(
            read.record(&reference).await.is_err(),
            "direct reads stay strict"
        );
        read.finish().await?;
    }
    observer.close().await?;
    fixture.stop().await
}
