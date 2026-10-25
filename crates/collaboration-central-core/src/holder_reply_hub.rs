//! Transient request correlation; durable retry belongs to the caller's journal.

use board_sync_protocol::BoardConsumerProtocol;
use durable_steps::CallFailure;
use fleet_sync_protocol::{ClientAnswer, ConnectionId, FrameId};
use fleet_sync_session::SessionKey;
use futures_channel::oneshot;
use runtime_primitives::SyncTime;
use std::{
    collections::{BTreeMap, btree_map::Entry},
    sync::{Arc, Mutex, Weak},
};

type Reply = ClientAnswer<BoardConsumerProtocol>;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ReplyKey {
    session: SessionKey,
    connection: ConnectionId,
    frame: FrameId,
}

struct PendingReply {
    deadline: SyncTime,
    answer: oneshot::Sender<Reply>,
}

#[derive(Default)]
pub(crate) struct HolderReplyHub {
    pending: Mutex<BTreeMap<ReplyKey, PendingReply>>,
    #[cfg(feature = "conformance")]
    answers_processed: std::sync::atomic::AtomicU64,
}

/// Dropping an RPC removes its correlation without keeping a reset instance alive.
pub(crate) struct ReplyWaiter {
    hub: Weak<HolderReplyHub>,
    key: ReplyKey,
    received: oneshot::Receiver<Reply>,
}

impl HolderReplyHub {
    pub(crate) fn register(
        self: &Arc<Self>,
        session: SessionKey,
        connection: ConnectionId,
        frame: FrameId,
        deadline: SyncTime,
    ) -> Result<ReplyWaiter, CallFailure> {
        let key = ReplyKey {
            session,
            connection,
            frame,
        };
        let (answer, received) = oneshot::channel();
        let mut pending = self.pending.lock().map_err(|_| CallFailure::AnswerLost)?;
        match pending.entry(key) {
            Entry::Vacant(entry) => {
                entry.insert(PendingReply { deadline, answer });
            }
            Entry::Occupied(_) => return Err(CallFailure::AnswerLost),
        }
        Ok(ReplyWaiter {
            hub: Arc::downgrade(self),
            key,
            received,
        })
    }

    pub(crate) fn resolve(
        &self,
        session: SessionKey,
        connection: ConnectionId,
        frame: FrameId,
        answer: Reply,
        now: SyncTime,
    ) {
        #[cfg(feature = "conformance")]
        self.answers_processed
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let reply = self.pending.lock().ok().and_then(|mut pending| {
            pending.remove(&ReplyKey {
                session,
                connection,
                frame,
            })
        });
        // Sending or dropping a sender can wake its caller; never do it under the map lock.
        if let Some(reply) = reply
            && reply.deadline > now
        {
            let _ = reply.answer.send(answer);
        }
    }

    pub(crate) fn fail_connection(&self, connection: ConnectionId) {
        self.remove_matching(|key, _| key.connection == connection);
    }

    pub(crate) fn expire(&self, now: SyncTime) {
        self.remove_matching(|_, pending| pending.deadline <= now);
    }

    pub(crate) fn next_deadline(&self) -> Option<SyncTime> {
        self.pending
            .lock()
            .ok()
            .and_then(|pending| pending.values().map(|reply| reply.deadline).min())
    }

    #[cfg(feature = "conformance")]
    pub(crate) fn observation(&self) -> Result<(usize, u64), durable_steps::StoreError> {
        Ok((
            self.pending
                .lock()
                .map_err(durable_steps::StoreError::backend)?
                .len(),
            self.answers_processed
                .load(std::sync::atomic::Ordering::SeqCst),
        ))
    }

    #[cfg(feature = "conformance")]
    pub(crate) fn frames(&self) -> Result<Vec<FrameId>, durable_steps::StoreError> {
        Ok(self
            .pending
            .lock()
            .map_err(durable_steps::StoreError::backend)?
            .keys()
            .map(|key| key.frame)
            .collect())
    }

    fn remove_matching(&self, matches: impl Fn(&ReplyKey, &PendingReply) -> bool) {
        let removed = self.pending.lock().ok().map(|mut pending| {
            let keys: Vec<_> = pending
                .iter()
                .filter_map(|(key, reply)| matches(key, reply).then_some(*key))
                .collect();
            keys.into_iter()
                .filter_map(|key| pending.remove(&key))
                .collect::<Vec<_>>()
        });
        drop(removed);
    }
}

/// Isolate the session-ID dimension on the owning runtime. Shared frame rules
/// derive it from Attached state, so a machine cannot legitimately vary it on
/// an unchanged connection; this probe tests the actual hub primitive directly.
#[cfg(feature = "conformance")]
pub(crate) fn session_identity_probe(
    source: &(impl runtime_primitives::Clock + runtime_primitives::IdSource),
) -> Result<(), durable_steps::StoreError> {
    use durable_steps::StoreError;
    use fleet_sync_protocol::{PeerId, ServerCallAnswer, SessionId, WithdrawAnswer};
    let hub = Arc::new(HolderReplyHub::default());
    let session = SessionKey {
        peer: PeerId::new(source.new_id()).map_err(StoreError::backend)?,
        session_id: SessionId::new(source.new_id()).map_err(StoreError::backend)?,
    };
    let wrong_session = SessionKey {
        session_id: SessionId::new(source.new_id()).map_err(StoreError::backend)?,
        ..session
    };
    let connection = ConnectionId::new(source.new_id()).map_err(StoreError::backend)?;
    let frame = FrameId::new(source.new_id()).map_err(StoreError::backend)?;
    let now = source.now();
    let mut waiter = hub
        .register(
            session,
            connection,
            frame,
            now.after(std::time::Duration::from_secs(20)),
        )
        .map_err(StoreError::backend)?;
    hub.resolve(
        wrong_session,
        connection,
        frame,
        ClientAnswer::Crate(ServerCallAnswer::WithdrawRelease(WithdrawAnswer::Withdrawn)),
        now,
    );
    if !matches!(waiter.received.try_recv(), Ok(None)) {
        return Err(StoreError::Aborted {
            reason: "session-only mismatch consumed the unchanged connection/frame waiter".into(),
        });
    }
    hub.resolve(
        session,
        connection,
        frame,
        ClientAnswer::Crate(ServerCallAnswer::WithdrawRelease(WithdrawAnswer::Withdrawn)),
        now,
    );
    if !matches!(
        waiter.received.try_recv(),
        Ok(Some(ClientAnswer::Crate(
            ServerCallAnswer::WithdrawRelease(WithdrawAnswer::Withdrawn)
        )))
    ) {
        return Err(StoreError::Aborted {
            reason: "exact session did not resolve the waiter".into(),
        });
    }
    Ok(())
}

impl ReplyWaiter {
    pub(crate) async fn answer(mut self) -> Result<Reply, CallFailure> {
        (&mut self.received)
            .await
            .map_err(|_| CallFailure::AnswerLost)
    }
}

impl Drop for ReplyWaiter {
    fn drop(&mut self) {
        let removed = self.hub.upgrade().and_then(|hub| {
            hub.pending
                .lock()
                .ok()
                .and_then(|mut pending| pending.remove(&self.key))
        });
        drop(removed);
    }
}

#[cfg(test)]
#[allow(
    clippy::panic_in_result_fn,
    reason = "Test assertions fail the case; fixture construction remains fallible."
)]
mod tests {
    use super::*;
    use fleet_sync_protocol::{PeerId, ServerCallAnswer, SessionId, WithdrawAnswer};

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
        assert_eq!(hub.next_deadline(), None);
        Ok(())
    }

    #[test]
    fn close_timeout_drop_and_instance_loss_cancel_only_their_pending_replies()
    -> Result<(), Box<dyn std::error::Error>> {
        let hub = Arc::new(HolderReplyHub::default());
        let first = key()?;
        let second = key()?;
        let mut closed = hub.register(first.session, first.connection, first.frame, deadline())?;
        let mut healthy =
            hub.register(second.session, second.connection, second.frame, deadline())?;
        hub.fail_connection(first.connection);
        assert!(closed.received.try_recv().is_err());
        assert!(matches!(healthy.received.try_recv(), Ok(None)));
        hub.expire(now());
        assert!(matches!(healthy.received.try_recv(), Ok(None)));
        hub.expire(deadline());
        assert!(healthy.received.try_recv().is_err());
        let dropped = hub.register(first.session, first.connection, first.frame, deadline())?;
        drop(dropped);
        assert_eq!(hub.next_deadline(), None);
        let mut reset = hub.register(first.session, first.connection, first.frame, deadline())?;
        drop(hub);
        assert!(
            reset.received.try_recv().is_err(),
            "waiter retained the reset instance"
        );
        Ok(())
    }

    #[test]
    fn an_answer_at_the_expiry_boundary_is_lost_even_before_the_alarm_runs()
    -> Result<(), Box<dyn std::error::Error>> {
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
}
