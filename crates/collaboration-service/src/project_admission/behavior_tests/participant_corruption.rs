use super::participant_intervals::create_as;
use super::*;
use sqlx::{ConnectOptions, Connection};

async fn foreign_join(fixture: &SubscriptionFixture) -> Result<FeedCursor, TestError> {
    let second = ThreadId::generate();
    fixture
        .create_root(&second, human("other-owner")?, "second root")
        .await?;
    assert_accepted(
        &fixture
            .submit(
                session("other-thread")?,
                BoardOperation::ThreadJoin {
                    thread_id: second.clone(),
                    role: ParticipantRole::Participant,
                    watch: false,
                    mode: None,
                    when_idle: None,
                    replace: None,
                    note: None,
                },
                fixture.now,
            )
            .await?,
    );
    let mut read = fixture.project_store().await?.snapshot().await?;
    let participants = read.participants(&second).await?;
    let position = participants
        .first()
        .ok_or("foreign participant")?
        .joined_at_position;
    read.finish().await?;
    Ok(position)
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Cursor binding and corrupt native rows must fail at real public readers.
async fn participant_cursor_is_thread_bound_and_unknown_stored_role_is_rejected()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::start().await?;
    let foreign = foreign_join(&fixture).await?;
    for name in ["one", "two"] {
        fixture
            .admit(
                session(name)?,
                BoardOperation::ThreadJoin {
                    thread_id: fixture.root.clone(),
                    role: ParticipantRole::Participant,
                    watch: false,
                    mode: None,
                    when_idle: None,
                    replace: None,
                    note: None,
                },
                fixture.now,
            )
            .await?;
    }
    let first = fixture
        .application
        .board()
        .thread_participant_list(ThreadParticipantListRequest {
            thread: (fixture.root.clone()).into(),
            page: PageRequest {
                limit: PageLimit::try_from(1)?,
                cursor: None,
            },
        })
        .await?;
    let mut read = fixture.project_store().await?.snapshot().await?;
    let other = read
        .records(RecordKind::Thread)
        .await?
        .into_iter()
        .find_map(|record| match record {
            Record::Thread(thread) if thread.thread_id != fixture.root => Some(thread.thread_id),
            _ => None,
        })
        .ok_or("second root")?;
    read.finish().await?;
    let error = fixture
        .application
        .board()
        .thread_participant_list(ThreadParticipantListRequest {
            thread: (other).into(),
            page: PageRequest {
                limit: PageLimit::try_from(1)?,
                cursor: Some(first.page.next_cursor.ok_or("cursor")?),
            },
        })
        .await
        .expect_err("foreign thread accepted cursor");
    assert_eq!(error.kind, BoardFailureKind::InvalidCursor);
    let mut raw = sqlx_turso::TursoConnectOptions::new()
        .filename(fixture.project_store().await?.path())
        .connect()
        .await?;
    sqlx::query("UPDATE thread_participants SET role='driver' WHERE thread_id=?")
        .bind(fixture.root.as_str())
        .execute(&mut raw)
        .await?;
    let error = fixture
        .application
        .board()
        .thread_participant_list(ThreadParticipantListRequest {
            thread: (fixture.root.clone()).into(),
            page: PageRequest::default(),
        })
        .await
        .expect_err("unknown role was accepted");
    assert_eq!(error.kind, BoardFailureKind::InvalidRecord);
    // Keep both the index and payload mutually consistent. Only the immutable
    // feed fact's thread differs, so a structural decoder alone cannot pass.
    let encoded = serde_json::to_string(&foreign)?;
    sqlx::query("UPDATE thread_participants SET role='participant',joined_epoch=?,joined_position=?,payload=json_set(payload,'$.joinedAtPosition',json(?),'$.lastSeenPosition',json(?)) WHERE thread_id=?").bind(i64::try_from(foreign.epoch.get())?).bind(i64::try_from(foreign.position.get())?).bind(encoded.clone()).bind(encoded).bind(fixture.root.as_str()).execute(&mut raw).await?;
    raw.close().await?;
    let error = fixture
        .application
        .board()
        .thread_participant_list(ThreadParticipantListRequest {
            thread: (fixture.root.clone()).into(),
            page: PageRequest::default(),
        })
        .await
        .expect_err("foreign feed fact was accepted for a participant interval");
    assert_eq!(error.kind, BoardFailureKind::InvalidRecord);
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // The list must validate and project holders from the same native seat truth as show.
async fn thread_list_rejects_corrupt_projected_orchestrator_records() -> Result<(), TestError> {
    let fixture = SubscriptionFixture::empty().await?;
    assert_accepted(
        &create_as(
            &fixture,
            session("first-orchestrator")?,
            Some(ParticipantRole::Orchestrator),
            false,
        )
        .await?,
    );
    let foreign = foreign_join(&fixture).await?;
    let mut read = fixture.project_store().await?.snapshot().await?;
    let workspace = read
        .record(&fixture.project.clone().into())
        .await?
        .ok_or("project")?
        .envelope()
        .scope
        .workspace
        .clone();
    read.finish().await?;
    let request = ThreadListRequest {
        workspace_id: workspace,
        reader: human("inspector")?,
        watched_only: false,
        page: PageRequest::default(),
    };
    let mut raw = sqlx_turso::TursoConnectOptions::new()
        .filename(fixture.project_store().await?.path())
        .connect()
        .await?;
    let payload: String =
        sqlx::query_scalar("SELECT payload FROM thread_participants WHERE thread_id=?")
            .bind(fixture.root.as_str())
            .fetch_one(&mut raw)
            .await?;
    for fault in ["role", "closedReason", "foreignSeen"] {
        sqlx::query(
            "UPDATE thread_participants SET role='orchestrator',payload=? WHERE thread_id=?",
        )
        .bind(payload.clone())
        .bind(fixture.root.as_str())
        .execute(&mut raw)
        .await?;
        match fault {
            "role" => {
                sqlx::query("UPDATE thread_participants SET role='driver' WHERE thread_id=?")
                    .bind(fixture.root.as_str())
                    .execute(&mut raw)
                    .await?;
            }
            "closedReason" => {
                sqlx::query("UPDATE thread_participants SET payload=json_set(payload,'$.closedReason','left') WHERE thread_id=?").bind(fixture.root.as_str()).execute(&mut raw).await?;
            }
            _ => {
                sqlx::query("UPDATE thread_participants SET payload=json_set(payload,'$.lastSeenPosition',json(?)) WHERE thread_id=?").bind(serde_json::to_string(&foreign)?).bind(fixture.root.as_str()).execute(&mut raw).await?;
            }
        }
        let error = fixture
            .application
            .board()
            .thread_list(request.clone())
            .await
            .expect_err("thread list accepted a corrupt projected holder");
        assert_eq!(
            error.kind,
            BoardFailureKind::InvalidRecord,
            "fault: {fault}"
        );
    }
    raw.close().await?;
    fixture.stop().await
}
