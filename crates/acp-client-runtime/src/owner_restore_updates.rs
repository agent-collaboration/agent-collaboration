//! Route untagged ACP updates to the current restore's own stream.

use crate::{acp_transport::TransportRun, owner_wire::OwnerAcpWire};
use agent_client_protocol::{Agent, ConnectionTo, Dispatch, Error, HandleDispatchFrom, Handled};
use session_event_model::session_profile_codec::STATE_METHOD;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, PoisonError},
};
use tokio::sync::mpsc;

#[derive(Default)]
pub(crate) struct RestoreUpdates(Mutex<BTreeMap<TransportRun, RestoreUpdateTap>>);

struct RestoreUpdateTap {
    session: String,
    updates: Option<mpsc::UnboundedSender<Dispatch>>,
    awaiting_reply: bool,
}

impl RestoreUpdates {
    pub(crate) fn open(
        &self,
        run: TransportRun,
        session: String,
    ) -> mpsc::UnboundedReceiver<Dispatch> {
        let (send, receive) = mpsc::unbounded_channel();
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                run,
                RestoreUpdateTap {
                    session,
                    updates: Some(send),
                    awaiting_reply: true,
                },
            );
        receive
    }

    pub(crate) fn retire(&self, run: TransportRun) {
        let mut taps = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(tap) = taps.get_mut(&run) {
            tap.updates = None;
            if !tap.awaiting_reply {
                taps.remove(&run);
            }
        }
    }

    pub(crate) fn reply_settled(&self, run: TransportRun) {
        let mut taps = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(tap) = taps.get_mut(&run) {
            tap.awaiting_reply = false;
            if tap.updates.is_none() {
                taps.remove(&run);
            }
        }
    }

    pub(crate) fn forward(
        &self,
        session: &str,
        current: Option<TransportRun>,
        message: Dispatch,
    ) -> Option<Dispatch> {
        let taps = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(tap) = current.and_then(|run| taps.get(&run)) {
            // An untagged state notification from an earlier unanswered
            // restore cannot supply live turn evidence for its successor.
            if matches!(&message, Dispatch::Notification(notification) if notification.method() == STATE_METHOD)
                && taps.iter().any(|(run, earlier)| {
                    Some(*run) != current && earlier.session == session && earlier.awaiting_reply
                })
            {
                return None;
            }
            if let Some(updates) = &tap.updates {
                let _owner_retired = updates.send(message);
            }
            return None;
        }
        if current.is_none() && taps.values().any(|tap| tap.session == session) {
            return None;
        }
        Some(message)
    }
}

pub(crate) struct OwnerRestoreUpdateRouter(pub(crate) Arc<OwnerAcpWire>);

impl HandleDispatchFrom<Agent> for OwnerRestoreUpdateRouter {
    async fn handle_dispatch_from(
        &mut self,
        message: Dispatch,
        _connection: ConnectionTo<Agent>,
    ) -> Result<Handled<Dispatch>, Error> {
        let session = match &message {
            Dispatch::Notification(notification)
                if matches!(notification.method(), "session/update" | STATE_METHOD) =>
            {
                notification
                    .params()
                    .get("sessionId")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            }
            _ => None,
        };
        let Some(session) = session else {
            return Ok(Handled::No {
                message,
                retry: false,
            });
        };
        Ok(match self.0.forward_restored_update(&session, message) {
            None => Handled::Yes,
            Some(message) => Handled::No {
                message,
                retry: false,
            },
        })
    }

    fn describe_chain(&self) -> impl std::fmt::Debug {
        "OwnerRestoreUpdateRouter"
    }
}
