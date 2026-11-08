//! Transient request correlation; durable retry belongs to the caller's journal.

use board_sync_protocol::BoardConsumerProtocol;
use durable_steps::CallFailure;
use fleet_protocol::{ClientAnswer, ConnectionId, FrameId};
use fleet_session::SessionKey;
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
    deadline: SyncTime,
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
            deadline,
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

    #[cfg(feature = "conformance")]
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
    use fleet_protocol::{PeerId, ServerCallAnswer, SessionId, WithdrawAnswer};
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
    pub(crate) fn deadline(&self) -> SyncTime {
        self.deadline
    }
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
#[path = "holder_reply_hub_test.rs"]
mod tests;
