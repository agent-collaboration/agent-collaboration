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
#[expect(
    clippy::panic_in_result_fn,
    reason = "Cursor binding and corrupt native rows must fail at real public readers."
)]
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
    let actor = serde_json::to_string(&RecordedActor::declared(session("one")?))?;
    assert_eq!(sqlx::query("UPDATE thread_participation SET role='driver' WHERE thread_id=? AND actor_id=(SELECT actor_id FROM actors WHERE canonical_identity=?)")
        .bind(fixture.root.as_str())
        .bind(&actor)
        .execute(&mut raw)
        .await?.rows_affected(), 1);
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
    // The non-holder's normalized row is structurally valid. Only its immutable
    // feed anchor belongs to another thread, so a structural decoder cannot pass.
    assert_eq!(sqlx::query("UPDATE thread_participation SET role='participant',joined_epoch=?,joined_position=?,last_seen_epoch=?,last_seen_position=? WHERE thread_id=? AND actor_id=(SELECT actor_id FROM actors WHERE canonical_identity=?)")
        .bind(i64::try_from(foreign.epoch.get())?).bind(i64::try_from(foreign.position.get())?)
        .bind(i64::try_from(foreign.epoch.get())?).bind(i64::try_from(foreign.position.get())?)
        .bind(fixture.root.as_str()).bind(&actor).execute(&mut raw).await?.rows_affected(),1);
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
#[expect(
    clippy::panic_in_result_fn,
    reason = "The list must validate and project holders from the same native seat truth as show."
)]
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
    let holder = read
        .participants(&fixture.root)
        .await?
        .into_iter()
        .find(|row| {
            row.is_open()
                && row.role == RecordedParticipantRole::Recorded(ParticipantRole::Orchestrator)
        })
        .ok_or("open orchestrator")?;
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
    for fault in ["role", "closedReason", "foreignSeen"] {
        assert_eq!(sqlx::query(
            "UPDATE thread_participation SET role='orchestrator',last_seen_epoch=?,last_seen_position=?,closed_reason=NULL WHERE thread_id=? AND closed_epoch IS NULL",
        )
        .bind(i64::try_from(holder.last_seen_position.epoch.get())?)
        .bind(i64::try_from(holder.last_seen_position.position.get())?)
        .bind(fixture.root.as_str())
        .execute(&mut raw)
        .await?.rows_affected(), 1);
        match fault {
            "role" => {
                assert_eq!(sqlx::query("UPDATE thread_participation SET role='driver' WHERE thread_id=? AND closed_epoch IS NULL")
                    .bind(fixture.root.as_str())
                    .execute(&mut raw)
                    .await?.rows_affected(), 1);
            }
            "closedReason" => {
                assert_eq!(sqlx::query("UPDATE thread_participation SET closed_reason='left' WHERE thread_id=? AND closed_epoch IS NULL").bind(fixture.root.as_str()).execute(&mut raw).await?.rows_affected(),1);
            }
            _ => {
                assert_eq!(sqlx::query("UPDATE thread_participation SET last_seen_epoch=?,last_seen_position=? WHERE thread_id=? AND closed_epoch IS NULL").bind(i64::try_from(foreign.epoch.get())?).bind(i64::try_from(foreign.position.get())?).bind(fixture.root.as_str()).execute(&mut raw).await?.rows_affected(),1);
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
