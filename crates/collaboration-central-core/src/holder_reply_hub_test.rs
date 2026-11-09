use super::*;
use fleet_protocol::{PeerId, ServerCallAnswer, SessionId, WithdrawAnswer};

fn key() -> Result<ReplyKey, Box<dyn std::error::Error>> {
    Ok(ReplyKey {
        session: SessionKey {
            peer: PeerId::new(durable_steps::Uuid::now_v7())?,
            session_id: SessionId::new(durable_steps::Uuid::now_v7())?,
        },
        connection: ConnectionId::new(durable_steps::Uuid::now_v7())?,
        frame: FrameId::new(durable_steps::Uuid::now_v7())?,
    })
}
fn answer() -> Reply {
    ClientAnswer::Crate(ServerCallAnswer::WithdrawRelease(WithdrawAnswer::Withdrawn))
}
fn now() -> SyncTime {
    SyncTime::from_unix_millis(100)
}
fn deadline() -> SyncTime {
    SyncTime::from_unix_millis(200)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn only_the_exact_session_connection_and_frame_can_resolve_a_waiter()
-> Result<(), Box<dyn std::error::Error>> {
    let hub = Arc::new(HolderReplyHub::default());
    let expected = key()?;
    let other = key()?;
    let mut waiter = hub.register(
        expected.session,
        expected.connection,
        expected.frame,
        deadline(),
    )?;
    for wrong in [
        ReplyKey {
            session: SessionKey {
                peer: other.session.peer,
                ..expected.session
            },
            ..expected
        },
        ReplyKey {
            session: SessionKey {
                session_id: other.session.session_id,
                ..expected.session
            },
            ..expected
        },
        ReplyKey {
            connection: other.connection,
            ..expected
        },
        ReplyKey {
            frame: other.frame,
            ..expected
        },
    ] {
        hub.resolve(
            wrong.session,
            wrong.connection,
            wrong.frame,
            answer(),
            now(),
        );
        assert!(
            matches!(waiter.received.try_recv(), Ok(None)),
            "a mismatched answer consumed the waiter: {wrong:?}"
        );
    }
    hub.resolve(
        expected.session,
        expected.connection,
        expected.frame,
        answer(),
        now(),
    );
    assert!(matches!(
        waiter.received.try_recv(),
        Ok(Some(ClientAnswer::Crate(
            ServerCallAnswer::WithdrawRelease(WithdrawAnswer::Withdrawn)
        )))
    ));
    assert!(hub.pending.lock().map_err(|_| "poisoned")?.is_empty());
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn close_drop_and_instance_loss_cancel_only_their_pending_replies()
-> Result<(), Box<dyn std::error::Error>> {
    let hub = Arc::new(HolderReplyHub::default());
    let first = key()?;
    let second = key()?;
    let mut closed = hub.register(first.session, first.connection, first.frame, deadline())?;
    let mut healthy = hub.register(second.session, second.connection, second.frame, deadline())?;
    hub.fail_connection(first.connection);
    assert!(closed.received.try_recv().is_err());
    assert!(matches!(healthy.received.try_recv(), Ok(None)));
    // A wait is bounded by its own event deadline, never by an alarm (H2):
    // closing one connection leaves the other connection's waiter pending.
    drop(healthy);
    let dropped = hub.register(first.session, first.connection, first.frame, deadline())?;
    drop(dropped);
    assert!(hub.pending.lock().map_err(|_| "poisoned")?.is_empty());
    let mut reset = hub.register(first.session, first.connection, first.frame, deadline())?;
    drop(hub);
    assert!(
        reset.received.try_recv().is_err(),
        "waiter retained the reset instance"
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn an_answer_at_the_expiry_boundary_is_lost() -> Result<(), Box<dyn std::error::Error>> {
    let hub = Arc::new(HolderReplyHub::default());
    let expected = key()?;
    let mut waiter = hub.register(
        expected.session,
        expected.connection,
        expected.frame,
        deadline(),
    )?;
    hub.resolve(
        expected.session,
        expected.connection,
        expected.frame,
        answer(),
        deadline(),
    );
    assert!(waiter.received.try_recv().is_err());
    Ok(())
}
