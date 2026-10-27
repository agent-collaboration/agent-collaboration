//! A participant page and its holders never decode the rest of a thread's history.
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
#[path = "support/board_read_setup.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
mod board_read_setup;
#[path = "support/read_cost_counter.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
#[expect(
    unused_imports,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
mod read_cost_counter;
use board_fixture::{BoardFixture, TestError, accepted};
use board_read_setup::{human_reader, keyed_call};
use collaboration_client::CollaborationClient;
use message_board::*;
use message_board_storage::{CursorStore, encode_board_cursor};
use read_cost_counter::{ReadCost, current_cost};
use serde_json::{Value, json};
use sqlx::{ConnectOptions, Connection};

fn page_request(
    thread: &ThreadRef,
    cursor: Option<String>,
) -> Result<ThreadParticipantListRequest, TestError> {
    Ok(ThreadParticipantListRequest {
        thread: thread.clone(),
        page: PageRequest {
            limit: PageLimit::try_from(2)?,
            cursor,
        },
    })
}

async fn participant_call(
    fixture: &BoardFixture,
    tool: &str,
    fields: Value,
) -> Result<Value, TestError> {
    let mut request = fixture.header();
    let object = request.as_object_mut().ok_or("participant request")?;
    object.remove("actingFor");
    object.extend(fields.as_object().ok_or("participant fields")?.clone());
    Ok(fixture.api.call(tool, request).await?)
}

async fn populate(fixture: &BoardFixture, size: usize) -> Result<ThreadRef, TestError> {
    let (project, _) = fixture.project().await?;
    let post = PostId::generate();
    accepted(&keyed_call(fixture, "thread_create", json!({"projectId":project,"postId":post,"actor":human_reader("00-holder")?,"text":"interval paging","about":fixture.discussion_subject(&project).await?,"references":[],"addressees":[],"role":"orchestrator","watch":false})).await?);
    let thread = ThreadRef {
        thread: ThreadId::from(post),
        project: Some(project),
    };
    accepted(&participant_call(fixture,"thread_join",json!({"thread":thread,"actor":human_reader("01-implementer")?,"role":"implementer","watch":false,"replace":null,"note":null})).await?);
    for index in 0..size - 2 {
        // Several intervals for the same reader cross both page and decimal-position boundaries.
        let mut join = participant_call(fixture,"thread_join",json!({"thread":thread,"actor":human_reader("zz-history")?,"role":"participant","watch":false,"replace":null,"note":format!("closed interval {index}")})).await?;
        accepted(&join);
        join = participant_call(
            fixture,
            "thread_leave",
            json!({"thread":thread,"actor":human_reader("zz-history")?,"afterLeave":null}),
        )
        .await?;
        accepted(&join);
    }
    Ok(thread)
}

async fn reject_old_cursor(
    fixture: &BoardFixture,
    client: &CollaborationClient,
    thread: &ThreadRef,
    first: &Participant,
) -> Result<(), TestError> {
    let request = page_request(thread, None)?;
    let mut filter = serde_json::to_value(&request)?;
    filter.as_object_mut().ok_or("filter")?.remove("page");
    let cursors =
        CursorStore::open(&fixture._directory.path().join("board-cursors.sqlite")).await?;
    let key = cursors.key().await?.ok_or("installed signing key")?;
    let cursor = encode_board_cursor(
        &key,
        &json!({"operation":"thread_participant_list","filter":serde_json::to_string(&filter)?,"lastKey":format!("{:?}:{}", first.identity, first.joined_at_position.position.get())}),
    )?;
    // The MAC and operation/filter/thread are valid; only the retired Debug-key shape differs.
    let error = client
        .thread_participant_list(page_request(thread, Some(cursor))?)
        .await
        .err()
        .ok_or("old cursor accepted")?;
    assert!(
        matches!(error, collaboration_client::BoardClientError::Rejected(error) if error.kind == BoardFailureKind::InvalidCursor)
    );
    cursors.close().await;
    Ok(())
}

async fn selected_corruption(
    fixture: &BoardFixture,
    client: &CollaborationClient,
    thread: &ThreadRef,
    first: &[Participant],
    last_cursor: String,
    last: &Participant,
) -> Result<(), TestError> {
    let store = fixture
        ._registry
        .existing_project(thread.project.as_ref().ok_or("qualified thread")?)
        .await?
        .ok_or("store")?;
    let mut raw = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    let original: Option<String> = sqlx::query_scalar("SELECT note FROM thread_participation WHERE thread_id=? AND actor_id=(SELECT actor_id FROM actors WHERE canonical_identity=?) AND joined_epoch=? AND joined_position=?")
        .bind(thread.thread.as_str()).bind(serde_json::to_string(&RecordedActor::declared(last.identity.clone()))?).bind(i64::try_from(last.joined_at_position.epoch.get())?).bind(i64::try_from(last.joined_at_position.position.get())?).fetch_one(&mut raw).await?;
    let changed = sqlx::query("UPDATE thread_participation SET note='' WHERE thread_id=? AND actor_id=(SELECT actor_id FROM actors WHERE canonical_identity=?) AND joined_epoch=? AND joined_position=?")
        .bind(thread.thread.as_str()).bind(serde_json::to_string(&RecordedActor::declared(last.identity.clone()))?).bind(i64::try_from(last.joined_at_position.epoch.get())?).bind(i64::try_from(last.joined_at_position.position.get())?).execute(&mut raw).await?;
    assert_eq!(changed.rows_affected(), 1);
    raw.close().await?;
    let healthy = client
        .thread_participant_list(page_request(thread, None)?)
        .await?;
    assert_eq!(healthy.page.records, first);
    let error = client
        .thread_participant_list(page_request(thread, Some(last_cursor.clone()))?)
        .await
        .err()
        .ok_or("selected closed interval corruption accepted")?;
    assert!(
        matches!(error, collaboration_client::BoardClientError::Rejected(error) if error.kind == BoardFailureKind::InvalidRecord)
    );
    let mut read = store.snapshot().await?;
    assert!(
        read.participants(&thread.thread).await.is_err(),
        "direct history must still reject the corrupt interval"
    );
    let high_water = read.high_water();
    read.finish().await?;
    let mut raw = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    assert_eq!(sqlx::query("UPDATE thread_participation SET note=? WHERE thread_id=? AND actor_id=(SELECT actor_id FROM actors WHERE canonical_identity=?) AND joined_epoch=? AND joined_position=?")
        .bind(original).bind(thread.thread.as_str()).bind(serde_json::to_string(&RecordedActor::declared(last.identity.clone()))?).bind(i64::try_from(last.joined_at_position.epoch.get())?).bind(i64::try_from(last.joined_at_position.position.get())?).execute(&mut raw).await?.rows_affected(),1);
    let holder = serde_json::to_string(&RecordedActor::declared(human_reader("01-implementer")?))?;
    let original: (String, i64, i64) = sqlx::query_as(
        "SELECT role,last_seen_epoch,last_seen_position FROM thread_participation WHERE thread_id=? AND actor_id=(SELECT actor_id FROM actors WHERE canonical_identity=?) AND closed_epoch IS NULL",
    )
    .bind(thread.thread.as_str())
    .bind(&holder)
    .fetch_one(&mut raw)
    .await?;
    for fault in ["role", "anchor"] {
        let (role, epoch, position) = if fault == "role" {
            ("driver", original.1, original.2)
        } else {
            (
                original.0.as_str(),
                i64::try_from(high_water.epoch.get())?,
                i64::try_from(high_water.position.get() + 1)?,
            )
        };
        assert_eq!(sqlx::query("UPDATE thread_participation SET role=?,last_seen_epoch=?,last_seen_position=? WHERE thread_id=? AND actor_id=(SELECT actor_id FROM actors WHERE canonical_identity=?) AND closed_epoch IS NULL").bind(role).bind(epoch).bind(position).bind(thread.thread.as_str()).bind(&holder).execute(&mut raw).await?.rows_affected(),1);
        let error = client
            .thread_participant_list(page_request(thread, Some(last_cursor.clone()))?)
            .await
            .err()
            .ok_or("corrupt off-page holder was accepted")?;
        assert!(
            matches!(error,collaboration_client::BoardClientError::Rejected(error) if error.kind==BoardFailureKind::InvalidRecord),
            "holder fault: {fault}"
        );
    }
    assert_eq!(
        sqlx::query(
            "UPDATE thread_participation SET role=?,last_seen_epoch=?,last_seen_position=? WHERE thread_id=? AND actor_id=(SELECT actor_id FROM actors WHERE canonical_identity=?) AND closed_epoch IS NULL"
        )
        .bind(original.0).bind(original.1).bind(original.2)
        .bind(thread.thread.as_str())
        .bind(&holder)
        .execute(&mut raw)
        .await?
        .rows_affected(),
        1
    );
    raw.close().await?;
    Ok(())
}

async fn fixture_cost(size: usize) -> Result<(ReadCost, Vec<Participant>), TestError> {
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("bounded participant history").await?;
    let thread = populate(&fixture, size).await?;
    let store = fixture
        ._registry
        .existing_project(thread.project.as_ref().ok_or("qualified thread")?)
        .await?
        .ok_or("store")?;
    let mut read = store.snapshot().await?;
    // The independent, unchanged native full-history API supplies the expected intervals.
    let mut expected = read.participants(&thread.thread).await?;
    read.finish().await?;
    let mut keyed = expected
        .into_iter()
        .map(|row| {
            Ok((
                (
                    serde_json::to_string(&row.identity)?,
                    row.joined_at_position.epoch,
                    row.joined_at_position.position,
                ),
                row,
            ))
        })
        .collect::<Result<Vec<_>, TestError>>()?;
    keyed.sort_by(|first, second| first.0.cmp(&second.0));
    expected = keyed.into_iter().map(|(_, row)| row).collect();
    assert_eq!(expected.len(), size);
    let before = current_cost();
    let first = client
        .thread_participant_list(page_request(&thread, None)?)
        .await?;
    let cost = current_cost().since(before);
    assert_eq!(first.page.records, expected.get(..2).ok_or("first two")?);
    assert_eq!(
        first.orchestrator.as_ref().map(|holder| &holder.identity),
        Some(&human_reader("00-holder")?)
    );
    assert_eq!(
        first.implementer.as_ref().map(|holder| &holder.identity),
        Some(&human_reader("01-implementer")?)
    );
    let first_records = first.page.records.clone();
    let mut observed = first.page.records;
    let mut next = first.page.next_cursor;
    let mut final_input = None;
    for _ in 0..size {
        let Some(cursor) = next.take() else {
            break;
        };
        final_input = Some(cursor.clone());
        let before = current_cost();
        let page = client
            .thread_participant_list(page_request(&thread, Some(cursor))?)
            .await?;
        let page_cost = current_cost().since(before);
        assert!(
            page_cost.participants <= 4,
            "page decoded unreturned history: {page_cost:?}"
        );
        assert_eq!(page.orchestrator, first.orchestrator);
        assert_eq!(page.implementer, first.implementer);
        observed.extend(page.page.records);
        next = page.page.next_cursor;
    }
    assert!(next.is_none(), "participant traversal did not terminate");
    assert_eq!(
        observed, expected,
        "interval traversal skipped, repeated or misordered a row"
    );
    reject_old_cursor(
        &fixture,
        &client,
        &thread,
        first_records.first().ok_or("first")?,
    )
    .await?;
    selected_corruption(
        &fixture,
        &client,
        &thread,
        &first_records,
        final_input.ok_or("last cursor")?,
        expected.last().ok_or("last interval")?,
    )
    .await?;
    fixture.stop().await?;
    Ok((cost, first_records))
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn participant_page_work_stays_bounded_with_same_reader_history() -> Result<(), TestError> {
    read_cost_counter::install();
    let small = fixture_cost(8).await?;
    let large = fixture_cost(80).await?;
    eprintln!(
        "participant page: eight={:?}; eighty={:?}",
        small.0, large.0
    );
    assert_eq!(
        small, large,
        "first page/body/anchor work grew with thread history"
    );
    assert_eq!(
        large.0.participants, 2,
        "holders already on the page are decoded only once"
    );
    assert!(large.0.feed_entries > 0 && large.0.feed_entries <= 6);
    Ok(())
}
