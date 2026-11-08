//! The machine stream owns retention; library notices only carry positioned hints.
#![cfg(feature = "conformance")]

use crate::session_publication_fixture as session_fixture;
mod support;
use board_sync_protocol::*;
use durable_steps::{AtomicStore, StoreError};
use fleet_protocol::*;
use fleet_session::SessionKey;
use session_fixture::{Fixture, TestError};
use std::sync::atomic::Ordering;
use support::*;

struct RetentionSeed {
    peer: PeerId,
    service: &'static str,
    missing: Option<i64>,
}

async fn seed_production_window(fixture: &Fixture, seed: RetentionSeed) -> Result<(), TestError> {
    let mut notices = Vec::new();
    for position in 99..=120_i64 {
        if seed.missing == Some(position) {
            continue;
        }
        let expires = if position <= 110 {
            1_792_281_599_999_i64
        } else {
            1_792_281_601_000_i64
        };
        notices.push((
            durable_steps::Uuid::now_v7().to_string(),
            position,
            expires - RETENTION,
            expires,
        ));
    }
    fixture.host.store.object.transaction(move |scope| Box::pin(async move {
        use durable_steps::storage::TransactionScope;
        use sqlx_core::query::query;
        let peer = seed.peer.to_string();
        query::<sqlx_turso::Turso>("INSERT INTO machine_enrollments(machine_id,service_id,machine_label,access_client_id,credential_hash,state,expires_at,created_at,updated_at) VALUES(?,?,'retention fixture',?,?,'enrolled','2033-05-18T03:33:20+00:00','2026-10-18T00:00:00+00:00','2026-10-18T00:00:00+00:00')")
            .bind(peer.clone()).bind(seed.service).bind(format!("fixture-client-{peer}")).bind(format!("fixture-hash-{peer}"))
            .execute(scope.executor()).await.map_err(StoreError::backend)?;
        query::<sqlx_turso::Turso>("INSERT INTO machine_control_project_progress(machine_id,last_issued_position,expired_through,created_at,updated_at) VALUES(?,120,98,1792281600000,1792281600000)")
            .bind(peer.clone()).execute(scope.executor()).await.map_err(StoreError::backend)?;
        for (id, position, created, expires) in notices {
            query::<sqlx_turso::Turso>("INSERT INTO machine_control_project_notices(notice_id,machine_id,position,kind,created_at,expires_at,updated_at) VALUES(?,?,?,'project_list_changed',?,?,?)")
                .bind(id).bind(peer.clone()).bind(position).bind(created).bind(expires).bind(created)
                .execute(scope.executor()).await.map_err(StoreError::backend)?;
        }
        Ok(())
    })).await?;
    Ok(())
}

async fn retained_stream(fixture: &Fixture, peer: PeerId) -> Result<(i64, Vec<i64>), TestError> {
    Ok(fixture.host.store.object.transaction(move |scope| Box::pin(async move {
        use durable_steps::storage::TransactionScope;
        use sqlx_core::{query::query, row::Row};
        let floor = query::<sqlx_turso::Turso>("SELECT expired_through FROM machine_control_project_progress WHERE machine_id=?")
            .bind(peer.to_string()).fetch_one(scope.executor()).await.map_err(StoreError::backend)?
            .try_get::<i64,_>("expired_through").map_err(StoreError::backend)?;
        let positions = query::<sqlx_turso::Turso>("SELECT position FROM machine_control_project_notices WHERE machine_id=? ORDER BY position")
            .bind(peer.to_string()).fetch_all(scope.executor()).await.map_err(StoreError::backend)?
            .into_iter().map(|row| row.try_get("position").map_err(StoreError::backend)).collect::<Result<Vec<i64>,_>>()?;
        Ok((floor, positions))
    })).await?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert the exact production retention window and admitted operation outcome."
)]
#[tokio::test]
async fn fleet_floor_98_prunes_99_through_110_and_drives_due_work() -> Result<(), TestError> {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let peer = PeerId::try_from("019f0000-0000-7000-8000-000000008102".to_owned())?;
    seed_production_window(
        &fixture,
        RetentionSeed {
            peer,
            service: "019f0000-0000-7000-8000-000000000001",
            missing: None,
        },
    )
    .await?;
    let call = invocation(&notice(121)?)?;
    assert!(matches!(
        fixture.runtime.delivery()?.on_call(call.clone()).await,
        Err(durable_steps::CallFailure::AnswerLost)
    ));

    fixture.runtime.on_alarm().await?;

    assert_eq!(fixture.runtime.delivery()?.on_call(call).await?, b"null");
    assert_eq!(
        retained_stream(&fixture, peer).await?,
        (110, (111..=121).collect())
    );
    fixture.objects.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "A corrupt stream must not roll back a healthy machine or strand its admitted work."
)]
#[tokio::test]
async fn one_machine_hole_does_not_block_other_retention_jobs_or_future_wakes()
-> Result<(), TestError> {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    // The corrupt machine sorts first, so fail-fast enumeration cannot pass.
    let broken = PeerId::try_from("019f0000-0000-7000-8000-000000008101".to_owned())?;
    let healthy = PeerId::try_from("019f0000-0000-7000-8000-000000008102".to_owned())?;
    seed_production_window(
        &fixture,
        RetentionSeed {
            peer: broken,
            service: "019f0000-0000-7000-8000-000000000002",
            missing: Some(105),
        },
    )
    .await?;
    seed_production_window(
        &fixture,
        RetentionSeed {
            peer: healthy,
            service: "019f0000-0000-7000-8000-000000000001",
            missing: None,
        },
    )
    .await?;
    fixture
        .io
        .clock_offset_millis
        .store(5_000, Ordering::SeqCst);
    let future = invocation(&notice(122)?)?;
    assert!(matches!(
        fixture.runtime.delivery()?.on_call(future.clone()).await,
        Err(durable_steps::CallFailure::AnswerLost)
    ));
    fixture.io.clock_offset_millis.store(0, Ordering::SeqCst);
    let due = invocation(&notice(121)?)?;
    assert!(matches!(
        fixture.runtime.delivery()?.on_call(due.clone()).await,
        Err(durable_steps::CallFailure::AnswerLost)
    ));
    fixture
        .io
        .wakes
        .lock()
        .map_err(|_| "wake lock poisoned")?
        .clear();

    let result = fixture.runtime.on_alarm().await;

    assert!(
        matches!(result, Err(StoreError::Corrupt { .. })),
        "maintenance corruption must remain visible"
    );
    assert_eq!(fixture.runtime.delivery()?.on_call(due).await?, b"null");
    assert!(matches!(
        fixture.runtime.delivery()?.on_call(future).await,
        Err(durable_steps::CallFailure::AnswerLost)
    ));
    assert_eq!(
        retained_stream(&fixture, healthy).await?,
        (110, (111..=121).collect())
    );
    assert_eq!(
        retained_stream(&fixture, broken).await?,
        (98, (99..=120).filter(|position| *position != 105).collect())
    );
    assert!(
        fixture
            .io
            .wakes
            .lock()
            .map_err(|_| "wake lock poisoned")?
            .iter()
            .any(|at| at.unix_millis() == 1_792_281_605_000)
    );
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
    let (binding, _, _socket) = fixture.socket().await?;
    insert_zero_position(&fixture).await?;
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
    let peer = PeerId::new(durable_steps::Uuid::now_v7())?;
    fixture.enroll(peer).await?;
    let first = notice(1)?;
    let first_call = invocation(&first)?;
    deliver(&fixture, &first_call).await?;
    assert_eq!(rows(&fixture).await?, 1);
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
