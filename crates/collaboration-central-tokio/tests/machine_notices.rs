//! The machine stream owns retention; library notices only carry positioned hints.
#![cfg(feature = "conformance")]
#![allow(clippy::panic_in_result_fn)]
#[path = "session_publication/support.rs"]
#[allow(dead_code)]
mod session_fixture;
#[path = "machine_notices/support.rs"]
mod support;
use board_sync_protocol::*;
use durable_steps::StoreError;
use fleet_sync_protocol::*;
use fleet_sync_session::SessionKey;
use session_fixture::{Fixture, TestError};
use std::sync::atomic::Ordering;
use support::*;

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
