#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! Every distinct join retains its interval, including role changes without posts or leaves.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "support/board_read_setup.rs"]
mod board_read_setup;
use board_fixture::{BoardFixture, TestError, accepted};
use board_read_setup::{human_reader, keyed_call};
use message_board::*;
use serde_json::{Value, json};

fn join_request(
    fixture: &BoardFixture,
    thread: &ThreadRef,
    actor: &Identity,
    role: &str,
    note: Option<&str>,
) -> Result<Value, TestError> {
    let mut request = fixture.header();
    let fields = request.as_object_mut().ok_or("join request")?;
    fields.remove("actingFor");
    fields.extend(
        json!({"thread":thread,"actor":actor,"role":role,"watch":false,"replace":null,"note":note})
            .as_object()
            .ok_or("join fields")?
            .clone(),
    );
    Ok(request)
}

async fn actor_history(
    fixture: &BoardFixture,
    thread: &ThreadRef,
    actor: &Identity,
) -> Result<Vec<Participant>, TestError> {
    let store = fixture
        ._registry
        .existing_project(thread.project.as_ref().ok_or("qualified thread")?)
        .await?
        .ok_or("native project")?;
    let mut read = store.snapshot().await?;
    let mut rows: Vec<_> = read
        .participants(&thread.thread)
        .await?
        .into_iter()
        .filter(|row| row.identity == *actor)
        .collect();
    read.finish().await?;
    rows.sort_by_key(|row| row.joined_at_position.position);
    Ok(rows)
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn role_change_without_a_leave_or_post_retains_each_join_interval() -> Result<(), TestError> {
    // Arrange: the first interval's note and position must remain historical.
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let post = PostId::generate();
    accepted(
        &keyed_call(
            &fixture,
            "thread_create",
            json!({"projectId":project,"postId":post,
        "text":"seat interval history","about":fixture.discussion_subject(&project).await?,
        "references":[],"addressees":[],"role":"orchestrator","watch":false}),
        )
        .await?,
    );
    let thread = ThreadRef {
        thread: post.into(),
        project: Some(project),
    };
    let actor = human_reader("history reader")?;
    let first_request = join_request(
        &fixture,
        &thread,
        &actor,
        "advisor",
        Some("Retained advisory note"),
    )?;
    accepted(&fixture.api.call("thread_join", first_request).await?);
    let first = actor_history(&fixture, &thread, &actor)
        .await?
        .into_iter()
        .next()
        .ok_or("first join interval")?;

    // Act: change only the role, with neither a leave nor a post in between.
    let change = join_request(&fixture, &thread, &actor, "reviewer", None)?;
    let reply = fixture.api.call("thread_join", change.clone()).await?;
    accepted(&reply);
    let intervals = actor_history(&fixture, &thread, &actor).await?;

    // Assert: an in-place role update loses the first row and fails here.
    assert_eq!(
        intervals.len(),
        2,
        "role change overwrote the advisory interval"
    );
    let old = intervals.first().ok_or("old interval")?;
    let current = intervals.get(1).ok_or("current interval")?;
    assert_eq!(
        old.role,
        RecordedParticipantRole::Recorded(ParticipantRole::Advisor)
    );
    assert_eq!(old.joined_at_position, first.joined_at_position);
    assert_eq!(old.note, first.note);
    assert_eq!(old.closed_at_position, Some(current.joined_at_position));
    assert_eq!(old.last_seen_position, current.joined_at_position);
    assert_eq!(serde_json::to_value(old.closed_reason)?, json!("rejoined"));
    assert_eq!(old.replaced_by, None);
    assert!(current.joined_at_position.position > old.joined_at_position.position);
    assert_eq!(
        current.role,
        RecordedParticipantRole::Recorded(ParticipantRole::Reviewer)
    );
    assert_eq!(current.note, first.note);
    assert_eq!(current.last_seen_position, current.joined_at_position);
    assert!(current.is_open());
    let client = fixture.api.client("seat interval history").await?;
    let listed = client
        .thread_participant_list(ThreadParticipantListRequest {
            thread: thread.clone(),
            page: PageRequest::default(),
        })
        .await?;
    let listed: Vec<_> = listed
        .page
        .records
        .into_iter()
        .filter(|row| row.identity == actor)
        .collect();
    assert_eq!(
        listed, intervals,
        "public interval history differs from the native owner"
    );

    // Replaying one request is still idempotent; a new join opens another interval.
    let replay = fixture.api.call("thread_join", change).await?;
    assert_eq!(
        replay.pointer("/result/result"),
        reply.pointer("/result/result")
    );
    assert_eq!(actor_history(&fixture, &thread, &actor).await?, intervals);
    accepted(
        &fixture
            .api
            .call(
                "thread_join",
                join_request(
                    &fixture,
                    &thread,
                    &actor,
                    "reviewer",
                    Some("Current review note"),
                )?,
            )
            .await?,
    );
    let latest = actor_history(&fixture, &thread, &actor).await?;
    assert_eq!(latest.len(), 3);
    assert_eq!(latest.iter().filter(|row| row.is_open()).count(), 1);
    assert_eq!(
        latest
            .last()
            .ok_or("latest interval")?
            .note
            .as_ref()
            .map(ParticipantNote::as_str),
        Some("Current review note")
    );
    let fixture = fixture.restart_read_fixture().await?;
    assert_eq!(
        actor_history(&fixture, &thread, &actor).await?,
        latest,
        "join history changed after native reopen"
    );
    fixture.stop().await?;
    Ok(())
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn handover_without_a_post_retains_the_advisor_and_promoted_intervals()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let post = PostId::generate();
    let outgoing = human_reader("outgoing holder")?;
    let target = human_reader("promoted advisor")?;
    accepted(&keyed_call(&fixture, "thread_create", json!({"projectId":project,"postId":post,
        "actor":outgoing,"text":"handover interval history","about":fixture.discussion_subject(&project).await?,
        "references":[],"addressees":[],"role":"orchestrator","watch":false})).await?);
    let thread = ThreadRef {
        thread: post.into(),
        project: Some(project),
    };
    accepted(
        &fixture
            .api
            .call(
                "thread_join",
                join_request(
                    &fixture,
                    &thread,
                    &target,
                    "advisor",
                    Some("Retained promotion note"),
                )?,
            )
            .await?,
    );
    let first = actor_history(&fixture, &thread, &target)
        .await?
        .into_iter()
        .next()
        .ok_or("advisor interval")?;

    // Act: hand over directly, without an intervening leave or post from B.
    let mut leave = fixture.header();
    let fields = leave.as_object_mut().ok_or("handover request")?;
    fields.remove("actingFor");
    fields.extend(
        json!({"thread":thread,"actor":outgoing,"afterLeave":{"kind":"handTo","to":target}})
            .as_object()
            .ok_or("handover fields")?
            .clone(),
    );
    let reply = fixture.api.call("thread_leave", leave.clone()).await?;
    accepted(&reply);

    let intervals = actor_history(&fixture, &thread, &target).await?;
    assert_eq!(
        intervals.len(),
        2,
        "handover overwrote the advisory interval"
    );
    let old = intervals.first().ok_or("old advisor interval")?;
    let promoted = intervals.get(1).ok_or("promoted interval")?;
    assert_eq!(
        old.role,
        RecordedParticipantRole::Recorded(ParticipantRole::Advisor)
    );
    assert_eq!(old.joined_at_position, first.joined_at_position);
    assert_eq!(old.note, first.note);
    assert_eq!(old.closed_at_position, Some(promoted.joined_at_position));
    assert_eq!(old.last_seen_position, promoted.joined_at_position);
    assert_eq!(serde_json::to_value(old.closed_reason)?, json!("promoted"));
    assert_eq!(old.replaced_by, None);
    assert_eq!(
        promoted.role,
        RecordedParticipantRole::Recorded(ParticipantRole::Orchestrator)
    );
    assert_eq!(promoted.note, first.note);
    assert!(promoted.joined_at_position.position > old.joined_at_position.position);
    assert_eq!(promoted.last_seen_position, promoted.joined_at_position);
    assert!(promoted.is_open());
    let former = actor_history(&fixture, &thread, &outgoing).await?;
    let former = former.first().ok_or("outgoing interval")?;
    assert_eq!(
        former.role,
        RecordedParticipantRole::Recorded(ParticipantRole::Orchestrator)
    );
    assert_eq!(former.closed_at_position, Some(promoted.joined_at_position));
    assert_eq!(
        former.closed_reason,
        Some(ParticipantClosedReason::Replaced)
    );
    assert_eq!(former.replaced_by, Some(target.clone()));
    let client = fixture.api.client("handover interval history").await?;
    let listed = client
        .thread_participant_list(ThreadParticipantListRequest {
            thread: thread.clone(),
            page: PageRequest::default(),
        })
        .await?;
    assert_eq!(
        listed.orchestrator.as_ref().map(|holder| &holder.identity),
        Some(&target)
    );
    assert_eq!(
        listed
            .orchestrator
            .as_ref()
            .map(|holder| holder.last_seen_position),
        Some(promoted.joined_at_position)
    );
    let listed: Vec<_> = listed
        .page
        .records
        .into_iter()
        .filter(|row| row.identity == target)
        .collect();
    assert_eq!(listed, intervals);
    let replay = fixture.api.call("thread_leave", leave).await?;
    assert_eq!(
        replay.pointer("/result/result"),
        reply.pointer("/result/result")
    );
    assert_eq!(actor_history(&fixture, &thread, &target).await?, intervals);
    let fixture = fixture.restart_read_fixture().await?;
    assert_eq!(actor_history(&fixture, &thread, &target).await?, intervals);
    fixture.stop().await?;
    Ok(())
}
