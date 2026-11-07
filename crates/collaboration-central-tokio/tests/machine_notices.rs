//! The machine stream owns retention; library notices only carry positioned hints.
#![cfg(feature = "conformance")]

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "session_publication/support.rs"]
mod session_fixture;
#[path = "machine_notices/support.rs"]
mod support;
use board_sync_protocol::*;
use durable_steps::{AtomicStore, StoreError};
use fleet_sync_protocol::*;
use fleet_sync_session::SessionKey;
use session_fixture::{Fixture, TestError};
use std::sync::atomic::Ordering;
use support::*;

#[expect(
    clippy::panic_in_result_fn,
    reason = "The production-shaped prefix must prune exactly and preserve admitted work."
)]
#[tokio::test]
async fn production_floor_98_prunes_99_through_110_and_drives_the_pending_job()
-> Result<(), TestError> {
    // Arrange: production's numeric boundary, with a valid seven-day interval
    // on every row. Only 99..110 have expired at the fixture's fixed clock.
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let state_id = durable_steps::Uuid::now_v7().to_string();
    let mut seeded = Vec::new();
    for position in 99..=120 {
        let expires_at = if position <= 110 {
            1_792_281_599_999_i64
        } else {
            1_792_281_601_000_i64
        };
        seeded.push((
            durable_steps::Uuid::now_v7().to_string(),
            position,
            serde_json::to_string(&notice(position)?)?,
            expires_at - RETENTION,
            expires_at,
        ));
    }
    fixture.host.store.object.transaction(move |scope| Box::pin(async move {
        use durable_steps::storage::TransactionScope;
        use sqlx_core::query::query;
        query::<sqlx_turso::Turso>("INSERT INTO machine_notice_state(state_id,name,last_position,pruned_through,peer_id) VALUES(?,'machine',120,98,NULL)")
            .bind(state_id).execute(scope.executor()).await.map_err(StoreError::backend)?;
        for (id, position, kind, created, expires) in seeded {
            query::<sqlx_turso::Turso>("INSERT INTO machine_notices(notice_id,position,kind,created_at,expires_at) VALUES(?,?,?,?,?)")
                .bind(id).bind(i64::try_from(position).map_err(StoreError::backend)?).bind(kind).bind(created).bind(expires)
                .execute(scope.executor()).await.map_err(StoreError::backend)?;
        }
        Ok(())
    })).await?;
    let call = invocation(&notice(121)?)?;
    assert!(matches!(
        fixture.runtime.delivery()?.on_call(call.clone()).await,
        Err(durable_steps::CallFailure::AnswerLost)
    ));

    // Act: the same alarm performs retention and drives the admitted append.
    fixture.runtime.on_alarm().await?;

    // Assert: no lexical gap, exact floor and suffix, same completed job.
    assert_eq!(fixture.runtime.delivery()?.on_call(call).await?, b"null");
    assert_eq!(
        retained_notice_rows(&fixture)
            .await?
            .into_iter()
            .map(|row| row.0)
            .collect::<Vec<_>>(),
        (111..=121).collect::<Vec<i64>>()
    );
    let floor = fixture
        .host
        .store
        .object
        .transaction(|scope| {
            Box::pin(async move {
                use durable_steps::storage::TransactionScope;
                use sqlx_core::row::Row;
                sqlx_core::query::query::<sqlx_turso::Turso>(
                    "SELECT pruned_through FROM machine_notice_state",
                )
                .fetch_one(scope.executor())
                .await
                .map_err(StoreError::backend)?
                .try_get::<i64, _>("pruned_through")
                .map_err(StoreError::backend)
            })
        })
        .await?;
    assert_eq!(floor, 110);
    fixture.objects.close().await?;
    Ok(())
}

#[tokio::test]
async fn machine_notice_reads_keep_numeric_position_order_beyond_nine() -> Result<(), TestError> {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let mut expected = Vec::new();
    for position in 1..=12 {
        let kind = notice(position)?;
        deliver(&fixture, &invocation(&kind)?).await?;
        expected.push((position, kind));
    }
    let (binding, _, mut socket) =
        admit(&fixture, PeerId::new(durable_steps::Uuid::now_v7())?).await?;

    let actual = read(&fixture, binding, &mut socket, None).await?;

    assert_page(actual, &expected);
    fixture.objects.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Regression assertions reject lost journal progress and incorrect retention."
)]
#[tokio::test]
async fn inverted_notice_expiry_retains_the_blocked_suffix_and_drives_due_jobs()
-> Result<(), TestError> {
    // Arrange: position 2 expires before position 1, as concurrent callers can.
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    fixture
        .io
        .clock_offset_millis
        .store(1_000, Ordering::SeqCst);
    deliver(&fixture, &invocation(&notice(1)?)?).await?;
    fixture.io.clock_offset_millis.store(0, Ordering::SeqCst);
    deliver(&fixture, &invocation(&notice(2)?)?).await?;
    let original = retained_notice_rows(&fixture).await?;
    fixture
        .io
        .clock_offset_millis
        .store(RETENTION, Ordering::SeqCst);
    let third = invocation(&notice(3)?)?;
    assert!(matches!(
        fixture.runtime.delivery()?.on_call(third.clone()).await,
        Err(durable_steps::CallFailure::AnswerLost)
    ));
    fixture
        .io
        .wakes
        .lock()
        .map_err(|_| "wake lock poisoned")?
        .clear();

    // Act: maintenance must leave both original notices and run the new job.
    fixture.runtime.on_alarm().await?;

    // Assert: the original operation completed, without losing the blocked row.
    assert_eq!(fixture.runtime.delivery()?.on_call(third).await?, b"null");
    assert_eq!(rows(&fixture).await?, 3);
    assert_eq!(
        retained_notice_rows(&fixture)
            .await?
            .into_iter()
            .take(2)
            .collect::<Vec<_>>(),
        original
    );
    assert!(
        fixture
            .io
            .wakes
            .lock()
            .map_err(|_| "wake lock poisoned")?
            .iter()
            .any(|at| at.unix_millis() == 1_792_886_401_000),
        "wake must follow position 1's expiry, rather than position 2's already expired timestamp"
    );
    fixture
        .io
        .clock_offset_millis
        .store(RETENTION + 1_000, Ordering::SeqCst);
    fixture.runtime.on_alarm().await?;
    assert_eq!(rows(&fixture).await?, 1);
    fixture.objects.close().await?;
    Ok(())
}

async fn retained_notice_rows(fixture: &Fixture) -> Result<Vec<(i64, i64, i64)>, TestError> {
    Ok(fixture
        .host
        .store
        .object
        .transaction(|scope| {
            Box::pin(async move {
                use durable_steps::storage::TransactionScope;
                use sqlx_core::row::Row;
                sqlx_core::query::query::<sqlx_turso::Turso>(
                    "SELECT position,created_at,expires_at FROM machine_notices ORDER BY position",
                )
                .fetch_all(scope.executor())
                .await
                .map_err(StoreError::backend)?
                .into_iter()
                .map(|row| {
                    Ok((
                        row.try_get("position").map_err(StoreError::backend)?,
                        row.try_get("created_at").map_err(StoreError::backend)?,
                        row.try_get("expires_at").map_err(StoreError::backend)?,
                    ))
                })
                .collect::<Result<Vec<_>, StoreError>>()
            })
        })
        .await?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "A missing retained position must remain a corruption error."
)]
#[tokio::test]
async fn an_unexpired_prefix_cannot_hide_a_missing_notice_position() -> Result<(), TestError> {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    for position in 1..=3 {
        deliver(&fixture, &invocation(&notice(position)?)?).await?;
    }
    fixture
        .host
        .store
        .object
        .transaction(|scope| {
            Box::pin(async move {
                use durable_steps::storage::TransactionScope;
                sqlx_core::query::query::<sqlx_turso::Turso>(
                    "DELETE FROM machine_notices WHERE position=2",
                )
                .execute(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await?;

    let result = fixture.runtime.on_alarm().await;

    assert!(
        matches!(&result, Err(StoreError::Corrupt { field, .. }) if field == "fleet.machine_notices.range"),
        "{result:?}"
    );
    assert_eq!(rows(&fixture).await?, 2);
    fixture.objects.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "An early maintenance error must not strand the admitted operation."
)]
#[tokio::test]
async fn session_alarm_rearms_admitted_work_after_retention_failure() -> Result<(), TestError> {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let call = invocation(&notice(1)?)?;
    assert!(matches!(
        fixture.runtime.delivery()?.on_call(call.clone()).await,
        Err(durable_steps::CallFailure::AnswerLost)
    ));
    insert_zero_position(&fixture).await?;
    fixture
        .io
        .wakes
        .lock()
        .map_err(|_| "wake lock poisoned")?
        .clear();

    let result = fixture.runtime.on_alarm().await;

    assert!(matches!(result, Err(StoreError::Corrupt { .. })));
    assert!(
        !fixture
            .io
            .wakes
            .lock()
            .map_err(|_| "wake lock poisoned")?
            .is_empty(),
        "retention error stranded the pending job without a wake"
    );
    fixture
        .host
        .store
        .object
        .transaction(|scope| {
            Box::pin(async move {
                use durable_steps::storage::TransactionScope;
                sqlx_core::query::query::<sqlx_turso::Turso>(
                    "DELETE FROM machine_notices WHERE position=0",
                )
                .execute(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await?;
    fixture.runtime.on_alarm().await?;
    assert_eq!(fixture.runtime.delivery()?.on_call(call).await?, b"null");
    assert_eq!(rows(&fixture).await?, 1);
    fixture.objects.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn an_orphaned_zero_position_notice_is_rejected_before_admission() -> Result<(), TestError> {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    insert_zero_position(&fixture).await?;
    let (binding, _, _) = fixture.socket().await?;
    let refused = fixture.runtime.on_socket_opened(binding).await;
    assert!(
        matches!(&refused,
        Err(StoreError::Corrupt {field,..}) if field=="fleet.machine_notices.state"),
        "{refused:?}"
    );
    assert_eq!(state_rows(&fixture).await?, 0);
    assert_eq!(rows(&fixture).await?, 1);
    fixture.objects.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_zero_position_notice_cannot_hide_below_an_existing_read_cursor() -> Result<(), TestError>
{
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let (binding, _, mut socket) =
        admit(&fixture, PeerId::new(durable_steps::Uuid::now_v7())?).await?;
    assert_page(read(&fixture, binding, &mut socket, Some(0)).await?, &[]);
    insert_zero_position(&fixture).await?;
    let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
    let request = serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request {
        frame,
        body: OperationRequest::Consumer(BoardConsumerRequest::Read {
            scope: BoardReadScope::Notices,
            cursor: Some(BoardReadCursor::Notices(NoticePosition::new(0))),
        }),
    })?;
    let refused = fixture.runtime.on_frame(binding, request).await;
    assert!(
        matches!(&refused,
        Err(StoreError::Corrupt {field,..}) if field=="fleet.machine_notices.range"),
        "{refused:?}"
    );
    assert_eq!(rows(&fixture).await?, 1);
    fixture.objects.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn machine_notices_survive_missing_admission_disconnect_and_replacement_without_duplicates()
-> Result<(), TestError> {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let first = notice(1)?;
    let first_call = invocation(&first)?;
    deliver(&fixture, &first_call).await?;
    assert_eq!(rows(&fixture).await?, 1);
    let peer = PeerId::new(durable_steps::Uuid::now_v7())?;
    let (binding, session, mut socket) = admit(&fixture, peer).await?;
    let key = SessionKey {
        peer,
        session_id: session,
    };
    assert!(
        fixture
            .runtime
            .conformance_transport_notices(key, NoticePosition::new(0))
            .await?
            .is_empty()
    );
    assert_page(
        read(&fixture, binding, &mut socket, None).await?,
        &[(1, first.clone())],
    );

    let second = notice(2)?;
    let second_call = invocation(&second)?;
    deliver(&fixture, &second_call).await?;
    assert_hint(next(&mut socket).await?, 1, 2, &second);
    assert_eq!(rows(&fixture).await?, 2);
    let acknowledged = fixture
        .runtime
        .delivery()?
        .on_call(second_call.clone())
        .await?;
    assert_eq!(acknowledged, b"null");
    assert_page(
        read(&fixture, binding, &mut socket, None).await?,
        &[(1, first.clone()), (2, second.clone())],
    );

    fixture.runtime.on_socket_closed(binding).await?;
    let third = notice(3)?;
    deliver(&fixture, &invocation(&third)?).await?;
    let transport = fixture
        .runtime
        .conformance_transport_notices(key, NoticePosition::new(0))
        .await?;
    assert_eq!(
        transport.len(),
        2,
        "disconnected delivery skipped the library's non-ended session"
    );
    assert!(
        matches!(&transport[1].body,Notice::Consumer(PositionedBoardNotice {position,kind}) if position.value()==3 && kind==&third)
    );
    assert_eq!(
        transport[1].position.value(),
        2,
        "the transport cursor was confused with the machine cursor"
    );

    let (replacement, _, mut replacement_socket) = admit(&fixture, peer).await?;
    assert_eq!(
        fixture.runtime.delivery()?.on_call(second_call).await?,
        b"null"
    );
    assert_eq!(rows(&fixture).await?, 3);
    assert_page(
        read(&fixture, replacement, &mut replacement_socket, None).await?,
        &[(1, first), (2, second), (3, third)],
    );
    fixture.objects.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn retention_floor_and_a_restored_older_head_have_explicit_read_outcomes()
-> Result<(), TestError> {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let peer = PeerId::new(durable_steps::Uuid::now_v7())?;
    let (binding, _, mut socket) = admit(&fixture, peer).await?;
    assert_page(read(&fixture, binding, &mut socket, Some(0)).await?, &[]);
    for position in 1..=2 {
        let kind = notice(position)?;
        deliver(&fixture, &invocation(&kind)?).await?;
        assert_hint(next(&mut socket).await?, position, position, &kind);
    }
    fixture.runtime.on_socket_closed(binding).await?;
    fixture
        .io
        .clock_offset_millis
        .store(RETENTION, Ordering::SeqCst);
    fixture.runtime.on_alarm().await?;
    assert_eq!(rows(&fixture).await?, 0);
    let (binding, _, mut socket) = admit(&fixture, peer).await?;
    assert_eq!(
        read(&fixture, binding, &mut socket, Some(0)).await?,
        NoticeRead::CursorExpired {
            floor: NoticePosition::new(2)
        }
    );
    assert_eq!(
        read(&fixture, binding, &mut socket, Some(1)).await?,
        NoticeRead::CursorExpired {
            floor: NoticePosition::new(2)
        }
    );
    assert_page(read(&fixture, binding, &mut socket, Some(2)).await?, &[]);
    assert_page(read(&fixture, binding, &mut socket, None).await?, &[]);
    assert_eq!(
        read(&fixture, binding, &mut socket, Some(3)).await?,
        NoticeRead::CursorInvalid
    );
    assert_eq!(
        read(&fixture, binding, &mut socket, Some(u64::MAX)).await?,
        NoticeRead::CursorInvalid
    );
    let third = notice(3)?;
    deliver(&fixture, &invocation(&third)?).await?;
    assert_hint(next(&mut socket).await?, 1, 3, &third);
    assert_page(
        read(&fixture, binding, &mut socket, Some(2)).await?,
        &[(3, third.clone())],
    );
    fixture.objects.close().await?;
    drop(fixture);
    let saved = tempfile::tempdir()?;
    snapshot_closed_owner(root.path(), saved.path())?;

    let fixture = Fixture::reopen(root.path()).await?;
    fixture
        .io
        .clock_offset_millis
        .store(RETENTION, Ordering::SeqCst);
    let (_, _, mut socket) = admit(&fixture, peer).await?;
    let fourth = notice(4)?;
    deliver(&fixture, &invocation(&fourth)?).await?;
    assert_hint(next(&mut socket).await?, 1, 4, &fourth);
    fixture.objects.close().await?;
    drop(fixture);
    // Restore the closed owner in a fresh home, including its SQLite sidecars.
    let fixture = Fixture::reopen(saved.path()).await?;
    fixture
        .io
        .clock_offset_millis
        .store(RETENTION, Ordering::SeqCst);
    let (binding, _, mut socket) = admit(&fixture, peer).await?;
    assert_eq!(
        read(&fixture, binding, &mut socket, Some(4)).await?,
        NoticeRead::CursorInvalid
    );
    assert_page(
        read(&fixture, binding, &mut socket, None).await?,
        &[(3, third)],
    );
    fixture.objects.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_conflicting_verified_admission_peer_cannot_overwrite_the_retained_snapshot()
-> Result<(), TestError> {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let peer = PeerId::new(durable_steps::Uuid::now_v7())?;
    let (binding, _, mut socket) = admit(&fixture, peer).await?;
    let (other, _, _) = fixture
        .socket_for(PeerId::new(durable_steps::Uuid::now_v7())?)
        .await?;
    assert!(
        matches!(fixture.runtime.on_socket_opened(other).await,Err(StoreError::Aborted {reason})
        if reason=="verified admission peer contradicts the machine's retained identity")
    );
    assert_eq!(stored_peer(&fixture).await?, peer.to_string());
    assert_page(read(&fixture, binding, &mut socket, None).await?, &[]);
    fixture.objects.close().await?;
    Ok(())
}
