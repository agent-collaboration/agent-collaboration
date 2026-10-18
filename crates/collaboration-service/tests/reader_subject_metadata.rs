//! Saved decisions survive unrelated body corruption while subject metadata stays validated.
#[path = "support/board_fixture.rs"]
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
    identity_path: &'static str,
    body_path: &'static str,
    body_fault: Value,
    subject_path: &'static str,
    subject_fault: Value,
    subject_field: &'static str,
}

#[allow(clippy::panic_in_result_fn)]
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
                identity_path: "/record/subtaskId",
                body_path: "/record/checked",
                body_fault: json!("notBoolean"),
                subject_path: "/record/taskId",
                subject_fault: json!("invalid-task"),
                subject_field: "taskId",
            }
        }
        ReplaySubjectCase::Trail => {
            let row = WorkTrailRowId::generate();
            ReplayOperand {
                tool:"work_trail_add",request:fixture.request(&project,json!({"kind":"workTrailAdd","rowId":row,"subject":{"kind":"task","subject":task},"text":"saved evidence","tags":[],"links":[]})),
                reference:row.into(),identity_path:"/record/rowId",body_path:"/record/text",body_fault:json!(42),
                subject_path:"/record/subject",subject_fault:json!({"kind":"unknownSubject","subject":task}),subject_field:"subject",
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
    let RecordPosition::Project(selected_position) = selected.envelope().position else {
        return Err("selected position".into());
    };
    read.finish().await?;
    // The replay must calculate fresh state, rather than returning a cached ReaderState.
    accepted(&fixture.api.call("subtask_add",fixture.request(&project,json!({"kind":"subtaskAdd","subtaskId":SubtaskId::generate(),"taskId":task,"text":"later activity","place":"90"}))).await?);
    let mut corrupted = serde_json::to_value(selected)?;
    *corrupted
        .pointer_mut(operand.body_path)
        .ok_or("body field")? = operand.body_fault;
    let kind = serde_json::to_value(operand.reference.kind)?
        .as_str()
        .ok_or("kind")?
        .to_owned();
    let mut observer = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    let changed = sqlx::query("UPDATE record_identities SET payload=? WHERE kind=? AND id=?")
        .bind(corrupted.to_string())
        .bind(&kind)
        .bind(operand.reference.id.as_str())
        .execute(&mut observer)
        .await?;
    assert_eq!(changed.rows_affected(), 1);
    // The immutable selected body is corrupt too; its subject metadata stays intact.
    let feed_path = format!("$.content.record{}", operand.body_path.replace('/', "."));
    let changed=sqlx::query("UPDATE project_feed SET entry=json_set(entry,?,json(?)) WHERE record_kind=? AND record_id=?")
        .bind(feed_path).bind(corrupted.pointer(operand.body_path).ok_or("fault")?.to_string()).bind(&kind).bind(operand.reference.id.as_str()).execute(&mut observer).await?;
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
    let mut metadata_faults = vec![
        (
            operand.subject_path,
            operand.subject_fault,
            operand.subject_field,
        ),
        (operand.subject_path, Value::Null, operand.subject_field),
        (operand.subject_path, json!(7), operand.subject_field),
        ("/kind", json!("unknownRecord"), "kind"),
        (operand.identity_path, json!(RecordId::generate()), "id"),
        ("/record/envelope/id", json!(RecordId::generate()), "id"),
        (
            "/record/envelope/scope/project",
            json!(ProjectId::generate()),
            "scope",
        ),
    ];
    if let ReplaySubjectCase::Trail = case {
        metadata_faults.push(("/record/projectId", json!(ProjectId::generate()), "project"));
        metadata_faults.push((
            operand.subject_path,
            json!(serde_json::to_string(&TrailSubject::Task(task.clone()))?),
            "subject",
        ));
        metadata_faults.push((
            operand.subject_path,
            json!({"kind":"task","subject":"invalid-task"}),
            "subject",
        ));
    }
    for (path, fault, field) in metadata_faults {
        let mut invalid = corrupted.clone();
        *invalid.pointer_mut(path).ok_or("metadata field")? = fault;
        assert_eq!(
            sqlx::query("UPDATE record_identities SET payload=? WHERE kind=? AND id=?")
                .bind(invalid.to_string())
                .bind(&kind)
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
            "{path}: {refused}"
        );
        assert_eq!(
            refused.pointer("/error/data/details/field"),
            Some(&json!(field)),
            "{path}: {refused}"
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
async fn subtask_saved_decision_replays_after_checked_body_corruption() -> Result<(), TestError> {
    replay_after_corruption(ReplaySubjectCase::Subtask).await
}
#[tokio::test]
async fn trail_saved_decision_replays_after_text_body_corruption() -> Result<(), TestError> {
    replay_after_corruption(ReplaySubjectCase::Trail).await
}
