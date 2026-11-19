//! The production `LinkCalls`: tools submit through `RouterLinkCalls`, and one
//! owning task (`driver_task`) runs the central link, the Mac outbox and its
//! minute flush on the injected clock.
mod driver_task;
#[cfg(test)]
mod driver_test_support;
mod link_transport;
mod machine_link;
#[cfg(test)]
mod overlay_property_test;
#[cfg(test)]
mod scripted_central;
#[cfg(test)]
mod store_faults;

pub use central_link_client::{
    LinkStatus as CentralLinkStatus, RefusalReason as CentralLinkRefusal,
};
pub use link_transport::{
    AUTHORITY_LAPSED_CLOSE, LinkClosed, LinkInbound, LinkSocket, LinkTransport, LinkTransportError,
    MachineTokenSource, TokenAvailability, TransportFuture, WebSocketLinkTransport,
};
pub use machine_link::{
    ConfiguredCentralLink, NotEnrolledTokens, UnconfiguredLinkTransport, open_machine_link,
    start_machine_link,
};
use workspace_ids::MachineId;

use crate::outbox_store::OutboxStoreError;
use crate::{CentralInbox, OutboxStorage};
use central_link_client::{
    FlushOutcomeSink, LinkCallFuture, LinkCalls, LinkClient, LinkStatus, OutgoingWrite,
    PendingWrite, RequestStatus, Submission, SubmitRefusal,
};
use central_link_protocol::RequestPayload;
use router_clock::RuntimeClock;
use runtime_primitives::SyncTime;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use workspace_model::Caller;

/// Where on a minute mark's ±3 s window this Router sends (owner, 2026-11-15:
/// "jitter i was thinking +-3s"). Injected so tests and simulations decide it.
pub trait FlushJitter: Send + Sync {
    /// Milliseconds to add to `mark`, in -3000..=3000.
    fn jitter_millis(&self, mark: SyncTime) -> i64;
}

/// Spreads Routers across the window by their installation, deterministically.
pub struct MachineFlushJitter {
    seed: u64,
}

impl MachineFlushJitter {
    #[must_use]
    pub fn new(machine_id: &MachineId) -> Self {
        let seed = machine_id
            .as_str()
            .bytes()
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
            });
        Self { seed }
    }
}

impl FlushJitter for MachineFlushJitter {
    fn jitter_millis(&self, mark: SyncTime) -> i64 {
        let mixed = self.seed ^ mark.unix_millis().cast_unsigned();
        i64::try_from(mixed % 6_001).unwrap_or(0) - 3_000
    }
}

pub(crate) enum DriverCommand {
    QueueMessage {
        caller: Caller,
        message: central_link_client::ToolSendMessage,
        reply: oneshot::Sender<Submission>,
    },
    Submit {
        caller: Caller,
        payload: RequestPayload,
        reply: oneshot::Sender<Submission>,
    },
    Status {
        caller: Caller,
        query: central_link_protocol::RequestStatusQuery,
        reply: oneshot::Sender<RequestStatus>,
    },
    Pending {
        caller: Caller,
        reply: oneshot::Sender<Result<Vec<PendingWrite>, OutboxStoreError>>,
    },
    Outgoing {
        caller: Caller,
        limit: u16,
        reply: oneshot::Sender<Result<Vec<OutgoingWrite>, OutboxStoreError>>,
    },
    LinkStatus {
        reply: oneshot::Sender<LinkStatus>,
    },
}

/// Everything the driver needs, created once by the Host.
pub struct CentralLinkParts {
    pub machine_id: MachineId,
    pub clock: Arc<dyn RuntimeClock>,
    pub store: Arc<Mutex<dyn OutboxStorage>>,
    pub transport: Arc<dyn LinkTransport>,
    pub tokens: Arc<dyn MachineTokenSource>,
    pub jitter: Arc<dyn FlushJitter>,
    pub outcomes: Arc<dyn FlushOutcomeSink>,
    pub inbox: Option<CentralInbox>,
}

/// The tools' handle on the running link. Cheap to clone.
#[derive(Clone)]
pub struct RouterLinkCalls {
    commands: mpsc::Sender<DriverCommand>,
}

#[derive(Debug, thiserror::Error)]
pub enum OutboxReadError {
    #[error("the central link driver has stopped")]
    DriverStopped,
    #[error(transparent)]
    Store(#[from] OutboxStoreError),
}

impl RouterLinkCalls {
    /// Starts the link driver on `tasks`; it stops when `stop` is cancelled.
    pub fn start(parts: CentralLinkParts, tasks: &TaskTracker, stop: CancellationToken) -> Self {
        let (commands, receiver) = mpsc::channel(256);
        let client = LinkClient::new(parts.machine_id.clone(), parts.clock.clone());
        let stop = stop.child_token();
        if let Some(inbox) = &parts.inbox {
            inbox.start_handoff(
                parts.clock.clone(),
                parts.store.clone(),
                parts.outcomes.clone(),
                tasks,
                stop.clone(),
            );
        }
        let driver = driver_task::LinkDriver::new(parts, client);
        tasks.spawn(async move {
            let _stop_inbox_on_exit = stop.clone().drop_guard();
            driver.run(receiver, stop).await;
        });
        Self { commands }
    }

    /// The caller's own unresolved outbox writes, after any batch in flight.
    pub async fn pending_for(&self, caller: Caller) -> Result<Vec<PendingWrite>, OutboxReadError> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(DriverCommand::Pending { caller, reply })
            .await
            .map_err(|_| OutboxReadError::DriverStopped)?;
        Ok(answer.await.map_err(|_| OutboxReadError::DriverStopped)??)
    }

    /// Where the link stands. `Refused { reason: CredentialRefused }` is "not
    /// enrolled; new code needed": the token source answered NeedsNewCode and
    /// the link waits for it to change. None once the driver has stopped.
    pub async fn link_status(&self) -> Option<LinkStatus> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(DriverCommand::LinkStatus { reply })
            .await
            .ok()?;
        answer.await.ok()
    }

    /// The caller's recently answered outbox writes, newest first.
    pub async fn outgoing_for(
        &self,
        caller: Caller,
        limit: u16,
    ) -> Result<Vec<OutgoingWrite>, OutboxReadError> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(DriverCommand::Outgoing {
                caller,
                limit,
                reply,
            })
            .await
            .map_err(|_| OutboxReadError::DriverStopped)?;
        Ok(answer.await.map_err(|_| OutboxReadError::DriverStopped)??)
    }
}

impl LinkCalls for RouterLinkCalls {
    fn submit_message(
        &self,
        caller: Caller,
        message: central_link_client::ToolSendMessage,
    ) -> LinkCallFuture<'_, Submission> {
        Box::pin(async move {
            let (reply, answer) = oneshot::channel();
            let request = message.request_id.clone();
            if self
                .commands
                .send(DriverCommand::QueueMessage {
                    caller,
                    message,
                    reply,
                })
                .await
                .is_err()
            {
                return Submission::Refused(SubmitRefusal::OutboxUnavailable);
            }
            answer.await.unwrap_or(Submission::Uncertain {
                request,
                scope: central_link_protocol::RequestScope::Organization,
            })
        })
    }

    fn submit(&self, caller: Caller, payload: RequestPayload) -> LinkCallFuture<'_, Submission> {
        Box::pin(async move {
            let (reply, answer) = oneshot::channel();
            let sent = self
                .commands
                .send(DriverCommand::Submit {
                    caller,
                    payload,
                    reply,
                })
                .await;
            match sent {
                Ok(()) => answer
                    .await
                    .unwrap_or(Submission::Refused(SubmitRefusal::CentralUnavailable)),
                Err(_) => Submission::Refused(SubmitRefusal::CentralUnavailable),
            }
        })
    }

    fn request_status(
        &self,
        caller: Caller,
        query: central_link_protocol::RequestStatusQuery,
    ) -> LinkCallFuture<'_, RequestStatus> {
        Box::pin(async move {
            let (reply, answer) = oneshot::channel();
            let sent = self
                .commands
                .send(DriverCommand::Status {
                    caller,
                    query,
                    reply,
                })
                .await;
            match sent {
                Ok(()) => answer.await.unwrap_or(RequestStatus::StatusUnavailable),
                Err(_) => RequestStatus::StatusUnavailable,
            }
        })
    }
}
