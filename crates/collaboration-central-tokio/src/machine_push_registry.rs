//! The latest welcomed socket locally owns delivery for an organization machine.
use fleet_identity_protocol::OrganizationId;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use workspace_ids::{MachineId, MessageId};

type MachineKey = (OrganizationId, MachineId);

struct PushSocket {
    sender: mpsc::Sender<MessageId>,
    replaced: CancellationToken,
}

#[derive(Clone, Default)]
pub(crate) struct MachinePushRegistry(Arc<Mutex<HashMap<MachineKey, PushSocket>>>);

impl MachinePushRegistry {
    pub(crate) fn register(
        &self,
        organization: OrganizationId,
        machine: MachineId,
    ) -> MachinePushRegistration {
        let key = (organization, machine);
        let (sender, receiver) = mpsc::channel(collaboration_central_core::WELCOME_PAGE_SIZE);
        let replaced = CancellationToken::new();
        let mut sockets = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(older) = sockets.insert(
            key.clone(),
            PushSocket {
                sender: sender.clone(),
                replaced: replaced.clone(),
            },
        ) {
            older.replaced.cancel();
        }
        MachinePushRegistration {
            registry: self.clone(),
            key,
            sender,
            replaced,
            receiver,
        }
    }

    pub(crate) fn disconnect(&self, organization: &OrganizationId, machine: &MachineId) {
        let mut sockets = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(socket) = sockets.remove(&(organization.clone(), machine.clone())) {
            socket.replaced.cancel();
        }
    }

    pub(crate) fn offer(
        &self,
        organization: &OrganizationId,
        machine: &MachineId,
        message: MessageId,
    ) {
        let sockets = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(socket) = sockets.get(&(organization.clone(), machine.clone()))
            && let Err(mpsc::error::TrySendError::Full(_)) = socket.sender.try_send(message)
        {
            // Reconnect can recover the durable queued rows after a backed-up socket closes.
            socket.replaced.cancel();
        }
    }
}

pub(crate) struct MachinePushRegistration {
    registry: MachinePushRegistry,
    key: MachineKey,
    sender: mpsc::Sender<MessageId>,
    pub(crate) replaced: CancellationToken,
    pub(crate) receiver: mpsc::Receiver<MessageId>,
}

impl Drop for MachinePushRegistration {
    fn drop(&mut self) {
        let mut sockets = self
            .registry
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if sockets
            .get(&self.key)
            .is_some_and(|socket| socket.sender.same_channel(&self.sender))
        {
            sockets.remove(&self.key);
        }
    }
}
