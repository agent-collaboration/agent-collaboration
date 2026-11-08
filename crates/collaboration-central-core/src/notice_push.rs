//! Which notices each machine's session was already given, kept in memory.
//!
//! A cursor is only a hint: nothing here is stored. Each runtime seeds the
//! cursors once from the committed heads when it opens, so a machine without a
//! cursor afterwards has a stream that began in this incarnation and gets all of
//! it. A negotiated Router restarts from what it acknowledged regardless.
use fleet_protocol::{ConnectionId, PeerId};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{
    Mutex, PoisonError,
    atomic::{AtomicBool, Ordering},
};

/// At most this many notices go to one machine in one event.
pub const PUSH_LIMIT: usize = 32;

/// Where a machine's next push starts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NoticeCursor {
    /// Everything through this position was sent.
    Sent(u64),
    /// This connection just opened: it is caught up once its Hello has
    /// attached a session, not before (an earlier send would land in a session
    /// log the new session never replays).
    CatchUp(ConnectionId),
}

#[derive(Debug, Default)]
pub(crate) struct NoticeCursors {
    cursors: Mutex<BTreeMap<PeerId, NoticeCursor>>,
    /// Machines a push left with unsent notices (more than 32 were owed).
    behind: Mutex<BTreeSet<PeerId>>,
    seeded: AtomicBool,
}

impl NoticeCursors {
    pub(crate) fn get(&self, peer: PeerId) -> Option<NoticeCursor> {
        self.cursors
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&peer)
            .copied()
    }

    /// Everything through `position` was sent; `issued` is the stream's head.
    pub(crate) fn set(&self, peer: PeerId, position: u64, issued: u64) {
        self.cursors
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(peer, NoticeCursor::Sent(position));
        let mut behind = self.behind.lock().unwrap_or_else(PoisonError::into_inner);
        if position < issued {
            behind.insert(peer);
        } else {
            behind.remove(&peer);
        }
    }

    /// The machine opened a connection: its first push is the Welcome catch-up.
    pub(crate) fn opened(&self, peer: PeerId, connection: ConnectionId) {
        self.cursors
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(peer, NoticeCursor::CatchUp(connection));
    }

    /// Machines with a connection waiting for its catch-up.
    pub(crate) fn catching_up(&self) -> BTreeSet<PeerId> {
        self.cursors
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(_, cursor)| matches!(cursor, NoticeCursor::CatchUp(_)))
            .map(|(peer, _)| *peer)
            .collect()
    }

    /// Machines with unfinished pushes: a connection waiting for its catch-up,
    /// or notices a push left unsent.
    pub(crate) fn unfinished(&self) -> BTreeSet<PeerId> {
        let mut unfinished: BTreeSet<PeerId> = self
            .cursors
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(_, cursor)| matches!(cursor, NoticeCursor::CatchUp(_)))
            .map(|(peer, _)| *peer)
            .collect();
        unfinished.extend(
            self.behind
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .copied(),
        );
        unfinished
    }

    /// Whether this runtime already seeded its cursors from the committed heads.
    pub(crate) fn seeded(&self) -> bool {
        self.seeded.load(Ordering::Acquire)
    }

    /// Start every machine that has no cursor yet at its committed head: what
    /// was issued before this runtime opened was pushed by an earlier one, or is
    /// caught up when the machine reconnects.
    pub(crate) fn seed(&self, heads: impl IntoIterator<Item = (PeerId, u64)>) {
        let mut cursors = self.cursors.lock().unwrap_or_else(PoisonError::into_inner);
        for (peer, head) in heads {
            cursors.entry(peer).or_insert(NoticeCursor::Sent(head));
        }
        self.seeded.store(true, Ordering::Release);
    }
}

/// The protocol additions central honors. Batches arrive with slice 4d.
pub const CENTRAL_FRAME_CAPABILITIES: fleet_protocol::FrameCapabilities =
    fleet_protocol::FrameCapabilities {
        batches: false,
        notices_applied: true,
    };
