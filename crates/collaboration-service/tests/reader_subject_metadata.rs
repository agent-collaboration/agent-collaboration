//! Saved decisions survive unrelated body corruption while subject metadata stays validated.
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
use serde_json::{Value, json};
use sqlx::{ConnectOptions, Connection};

#[derive(Clone, Copy)]
enum ReplaySubjectCase {
    Subtask,
    Trail,
}
struct ReplayOperand {
    tool: &'static str,
    request: Value,
    reference: RecordRef,
}

async fn replay_after_corruption(case: ReplaySubjectCase) -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let task = fixture.task(&project, &workstream).await?;
    let operand = match case {
        ReplaySubjectCase::Subtask => {
            let subtask = SubtaskId::generate();
            accepted(&fixture.api.call("subtask_add",fixture.request(&project,json!({"kind":"subtaskAdd","subtaskId":subtask,"taskId":task,"text":"saved child","place":"80"}))).await?);
            ReplayOperand {
                tool: "subtask_check",
                request: fixture.request(
                    &project,
                    json!({"kind":"subtaskCheck","subtaskId":subtask,"checked":true}),
                ),
                reference: subtask.into(),
            }
        }
        ReplaySubjectCase::Trail => {
            let row = WorkTrailRowId::generate();
            ReplayOperand {
                tool:"work_trail_add",request:fixture.request(&project,json!({"kind":"workTrailAdd","rowId":row,"subject":{"kind":"task","subject":task},"text":"saved evidence","tags":[],"links":[]})),
                reference:row.into(),
            }
        }
    };
    let saved = fixture
        .api
        .call(operand.tool, operand.request.clone())
        .await?;
    accepted(&saved);
    let saved_count = saved
        .pointer("/result/reader/unreadCount")
        .and_then(Value::as_u64)
        .ok_or("saved count")?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let selected = read
        .record(&operand.reference)
        .await?
        .ok_or("selected record")?;
    let selected_position = selected
        .envelope()
        .position
        .project_cursor()
        .ok_or("selected position")?;
    read.finish().await?;
    // The replay must calculate fresh state, rather than returning a cached ReaderState.
    accepted(&fixture.api.call("subtask_add",fixture.request(&project,json!({"kind":"subtaskAdd","subtaskId":SubtaskId::generate(),"taskId":task,"text":"later activity","place":"90"}))).await?);
    let kind = serde_json::to_value(operand.reference.kind)?
        .as_str()
        .ok_or("kind")?
        .to_owned();
    let mut observer = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .foreign_keys(false)
        .connect()
        .await?;
    let body_statement = match case {
        ReplaySubjectCase::Subtask => "UPDATE subtasks SET text='' WHERE subtask_id=?",
        ReplaySubjectCase::Trail => "UPDATE work_trail_rows SET text='' WHERE row_id=?",
    };
    let changed = sqlx::query(body_statement)
        .bind(operand.reference.id.as_str())
        .execute(&mut observer)
        .await?;
    assert_eq!(changed.rows_affected(), 1);
    // The immutable selected body is corrupt too; its subject metadata stays intact.
    let changed=sqlx::query("UPDATE project_feed SET content=json_set(content,'$.record.record.text','') WHERE record_kind=? AND record_id=?")
        .bind(&kind).bind(operand.reference.id.as_str()).execute(&mut observer).await?;
    assert!(changed.rows_affected() > 0);
    let actor: Identity = serde_json::from_value(fixture.actor.clone())?;
    let mut read = store.snapshot().await?;
    let head = read.high_water();
    let mark = read
        .read_mark(
            &actor,
            &ReadScope::Project {
                project_id: project.clone(),
            },
        )
        .await?
        .map_or(
            FeedCursor {
                epoch: head.epoch,
                position: FeedPosition::ZERO,
            },
            |mark| mark.through,
        );
    let expected = read.unread_state_after(&task.clone().into(), mark).await?;
    assert!(expected.unread_count > saved_count);
    read.finish().await?;
    let replay = fixture
        .api
        .call(operand.tool, operand.request.clone())
        .await?;
    accepted(&replay);
    assert_eq!(
        replay.pointer("/result/result"),
        saved.pointer("/result/result"),
        "saved terminal decision changed"
    );
    assert_eq!(
        replay.pointer("/result/reader"),
        Some(&serde_json::to_value(expected)?),
        "fresh metadata state: {replay}"
    );
    let mut read = store.snapshot().await?;
    assert_eq!(read.high_water(), head, "replay repeated effects");
    assert!(
        matches!(
            read.record(&operand.reference).await,
            Err(message_board_storage::ProjectStorageError::InvalidRecord { .. })
        ),
        "direct full body read must still refuse corruption"
    );
    let before_selected = FeedCursor {
        epoch: selected_position.epoch,
        position: FeedPosition::try_from(selected_position.position.get() - 1)?,
    };
    assert!(
        matches!(
            read.feed_after(before_selected, 1, true).await,
            Err(
                message_board_storage::ProjectStorageError::InvalidRecord { .. }
                    | message_board_storage::ProjectStorageError::InvalidFeed
            )
        ),
        "content feed must still validate its selected body"
    );
    read.finish().await?;
    // Duplicate blob identities/scope and malformed Boolean cells no longer exist.
    // project_native_integrity_tests exercises their native key/FK/CHECK/scope guards.
    let (statement, field, metadata_faults) = match case {
        ReplaySubjectCase::Subtask => (
            "UPDATE subtasks SET task_id=? WHERE subtask_id=?",
            "scope",
            vec![
                "invalid-task".to_owned(),
                TaskId::generate().as_str().to_owned(),
                String::new(),
                "7".to_owned(),
            ],
        ),
        ReplaySubjectCase::Trail => (
            "UPDATE work_trail_rows SET subject=? WHERE row_id=?",
            "subject",
            vec![
                json!({"kind":"unknownSubject","subject":task}),
                Value::Null,
                json!(7),
                json!(serde_json::to_string(&TrailSubject::Task(task.clone()))?),
                json!({"kind":"task","subject":"invalid-task"}),
            ]
            .into_iter()
            .map(|value| value.to_string())
            .collect(),
        ),
    };
    for fault in metadata_faults {
        assert_eq!(
            sqlx::query(statement)
                .bind(&fault)
                .bind(operand.reference.id.as_str())
                .execute(&mut observer)
                .await?
                .rows_affected(),
            1
        );
        let refused = fixture
            .api
            .call(operand.tool, operand.request.clone())
            .await?;
        assert_eq!(
            refused.pointer("/error/data/kind"),
            Some(&json!("invalidRecord")),
            "{fault}: {refused}"
        );
        assert_eq!(
            refused.pointer("/error/data/details/field"),
            Some(&json!(field)),
            "{fault}: {refused}"
        );
    }
    let read = store.snapshot().await?;
    assert_eq!(
        read.high_water(),
        head,
        "metadata refusals repeated effects"
    );
    read.finish().await?;
    observer.close().await?;
    fixture.stop().await
}

#[tokio::test]
async fn subtask_saved_decision_replays_after_text_body_corruption() -> Result<(), TestError> {
    Box::pin(replay_after_corruption(ReplaySubjectCase::Subtask)).await
}
#[tokio::test]
async fn trail_saved_decision_replays_after_text_body_corruption() -> Result<(), TestError> {
    Box::pin(replay_after_corruption(ReplaySubjectCase::Trail)).await
}
