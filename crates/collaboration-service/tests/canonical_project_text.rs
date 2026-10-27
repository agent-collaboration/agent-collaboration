#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! Incoming normalization stays useful; persisted current/feed values must already be canonical.
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
#[expect(
    clippy::indexing_slicing,
    reason = "This test consumes a subset of the shared fixture and retains its fixed JSON construction."
)]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted};
use message_board::*;
use message_board_storage::ProjectStorageError;
use serde_json::{Value, json};
use sqlx::ConnectOptions;

async fn setup() -> Result<
    (
        BoardFixture,
        ProjectId,
        Vec<(RecordRef, &'static str, &'static str)>,
    ),
    TestError,
> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let workstream = WorkstreamId::generate();
    let task = TaskId::generate();
    let row = WorkTrailRowId::generate();
    for (tool, operation) in [
        (
            "workstream_create",
            json!({"kind":"workstreamCreate","workstreamId":workstream,"name":" canonical work ","goal":"codec proof"}),
        ),
        (
            "task_create",
            json!({"kind":"taskCreate","taskId":task,"workstreamId":workstream,"title":" canonical task ","body":"","blockedBy":[],"relatedTo":[]}),
        ),
        (
            "work_trail_add",
            json!({"kind":"workTrailAdd","rowId":row,"subject":{"kind":"project"},"text":"canonical decision","tags":[" decision "],"links":[]}),
        ),
    ] {
        accepted(
            &fixture
                .api
                .call(tool, fixture.request(&project, operation))
                .await?,
        );
    }
    Ok((
        fixture,
        project,
        vec![
            (workstream.into(), "name", "canonical work"),
            (task.into(), "title", "canonical task"),
            (row.into(), "tags", "decision"),
        ],
    ))
}
#[expect(
    clippy::panic_in_result_fn,
    reason = "Literal native diagnostic assertions are the regression oracle."
)]
fn assert_invalid<TValue>(
    result: Result<TValue, ProjectStorageError>,
    reference: &RecordRef,
    field: &str,
) -> Result<(), TestError> {
    match result {
        Err(ProjectStorageError::InvalidRecord {
            record,
            field: observed,
        }) => {
            assert_eq!(&record, reference);
            assert_eq!(observed, field);
        }
        Err(other) => return Err(format!("canonical text lost field attribution: {other}").into()),
        Ok(_) => return Err("noncanonical stored text was silently normalized".into()),
    }
    Ok(())
}
#[expect(
    clippy::panic_in_result_fn,
    reason = "Verify the independent fixture's original canonical literal."
)]
fn corrupt_text(
    value: &mut Value,
    prefix: &str,
    field: &str,
    expected: &str,
) -> Result<(), TestError> {
    let suffix = if field == "tags" { "/tags/0" } else { field };
    let pointer = if field == "tags" {
        format!("{prefix}{suffix}")
    } else {
        format!("{prefix}/{suffix}")
    };
    let text = value.pointer_mut(&pointer).ok_or("stored field")?;
    assert_eq!(
        text,
        &json!(expected),
        "incoming text was not canonicalized"
    );
    *text = json!(format!(" {expected} "));
    Ok(())
}
#[tokio::test]

async fn native_current_and_kind_reads_reject_padded_names_titles_and_tags() -> Result<(), TestError>
{
    let (fixture, project, cases) = setup().await?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut observer = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    for (reference, field, expected) in cases {
        let mut read = store.snapshot().await?;
        let original = serde_json::to_value(read.record(&reference).await?.ok_or("record")?)?;
        read.finish().await?;
        let mut corrupt = original.clone();
        corrupt_text(&mut corrupt, "/record", field, expected)?;
        let statement = match reference.kind {
            RecordKind::Workstream => "UPDATE workstreams SET name=? WHERE workstream_id=?",
            RecordKind::Task => "UPDATE tasks SET title=? WHERE task_id=?",
            RecordKind::WorkTrailRow => "UPDATE work_trail_tags SET tag=? WHERE row_id=?",
            _ => return Err("unexpected canonical text fixture".into()),
        };
        sqlx::query(statement)
            .bind(format!(" {expected} "))
            .bind(reference.id.as_str())
            .execute(&mut observer)
            .await?;
        let mut read = store.snapshot().await?;
        assert_invalid(read.record(&reference).await, &reference, field)?;
        assert_invalid(read.records(reference.kind).await, &reference, field)?;
        read.finish().await?;
        sqlx::query(statement)
            .bind(expected)
            .bind(reference.id.as_str())
            .execute(&mut observer)
            .await?;
    }
    sqlx::Connection::close(observer).await?;
    fixture.stop().await
}
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn native_immutable_feed_rejects_padded_names_titles_and_tags() -> Result<(), TestError> {
    let (fixture, project, cases) = setup().await?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut observer = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    for (reference, field, expected) in cases {
        let mut read = store.snapshot().await?;
        let original_revision = read
            .assignment_revision(&RoleSubject::Project(project.clone()))
            .await?;
        let zero = FeedCursor {
            epoch: read.high_water().epoch,
            position: FeedPosition::ZERO,
        };
        let FeedRead::Entries { entries, .. } = read.feed_after(zero, u64::MAX, true).await? else {
            return Err("feed".into());
        };
        let entry = entries
            .into_iter()
            .find(|entry| entry.record == reference)
            .ok_or("feed entry")?;
        read.finish().await?;
        let original = serde_json::to_value(entry)?;
        let mut corrupt = original.clone();
        corrupt_text(&mut corrupt, "/content/record/record", field, expected)?;
        sqlx::query("UPDATE project_feed SET content = ? WHERE record_id = ?")
            .bind(corrupt.get("content").ok_or("content")?.to_string())
            .bind(reference.id.as_str())
            .execute(&mut observer)
            .await?;
        let mut read = store.snapshot().await?;
        assert_invalid(
            read.feed_after(zero, u64::MAX, true).await,
            &reference,
            field,
        )?;
        assert_eq!(
            read.assignment_revision(&RoleSubject::Project(project.clone()))
                .await?,
            original_revision,
            "unrelated body corruption cannot change indexed assignment metadata"
        );
        read.finish().await?;
        sqlx::query("UPDATE project_feed SET content = ? WHERE record_id = ?")
            .bind(original.get("content").ok_or("content")?.to_string())
            .bind(reference.id.as_str())
            .execute(&mut observer)
            .await?;
    }
    sqlx::Connection::close(observer).await?;
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn participant_reads_reject_noncanonical_notes_and_joining_feed_metadata()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let post = PostId::generate();
    let thread = ThreadId::from(post.clone());
    let mut create = fixture.header();
    create.as_object_mut().ok_or("header")?.extend(json!({"projectId":project,"postId":post,"about":{"kind":"project","subject":{"line":"Fixture discussion"}},"role":null,"watch":false,"text":"root","references":[],"addressees":[]}).as_object().ok_or("fields")?.clone());
    accepted(&fixture.api.call("thread_create", create).await?);
    let mut join = fixture.header();
    join.as_object_mut().ok_or("header")?.remove("actingFor");
    join.as_object_mut().ok_or("header")?.extend(json!({"thread":{"thread":thread},"role":"participant","watch":false,"note":" canonical note ","replace":null}).as_object().ok_or("fields")?.clone());
    accepted(&fixture.api.call("thread_join", join).await?);
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let participants = read.participants(&thread).await?;
    let participant = participants.first().ok_or("participant")?;
    let original = serde_json::to_value(participant)?;
    assert_eq!(original.get("note"), Some(&json!("canonical note")));
    read.finish().await?;
    let mut observer = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    let mut corrupt = original.clone();
    *corrupt.get_mut("note").ok_or("note")? = json!(" padded note ");
    sqlx::query("UPDATE thread_participation SET note=? WHERE thread_id=?")
        .bind(corrupt.get("note").and_then(Value::as_str))
        .bind(thread.as_str())
        .execute(&mut observer)
        .await?;
    let mut read = store.snapshot().await?;
    assert_invalid(
        read.participants(&thread).await,
        &thread.clone().into(),
        "note",
    )?;
    read.finish().await?;
    sqlx::query("UPDATE thread_participation SET note=? WHERE thread_id=?")
        .bind(original.get("note").and_then(Value::as_str))
        .bind(thread.as_str())
        .execute(&mut observer)
        .await?;
    let mut read = store.snapshot().await?;
    let zero = FeedCursor {
        epoch: read.high_water().epoch,
        position: FeedPosition::ZERO,
    };
    let FeedRead::Entries { entries, .. } = read.feed_after(zero, u64::MAX, true).await? else {
        return Err("feed".into());
    };
    let entry = entries
        .into_iter()
        .find(|entry| entry.position == participant.joined_at_position.position)
        .ok_or("join feed")?;
    let reference = entry.record.clone();
    read.finish().await?;
    assert_eq!(entry.origin.machine.as_str(), "V1/V2 proof");
    sqlx::query("UPDATE project_feed SET origin_machine=? WHERE record_id=?")
        .bind(" V1/V2 proof ")
        .bind(reference.id.as_str())
        .execute(&mut observer)
        .await?;
    let mut read = store.snapshot().await?;
    assert_invalid(
        read.participants(&thread).await,
        &reference,
        "origin.machine",
    )?;
    read.finish().await?;
    sqlx::Connection::close(observer).await?;
    fixture.stop().await
}
