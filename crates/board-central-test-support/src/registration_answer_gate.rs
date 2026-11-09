//! Hold a real registration answer while its independently committed notice passes.
use crate::FixtureResult;
use board_sync_protocol::BoardConsumerProtocol;
use fleet_protocol::{DatabaseId, LeaseAnswer, OperationAnswer, RegistrationAnswer, ServerFrame};
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

struct Selection {
    database: DatabaseId,
    held: watch::Sender<bool>,
    released: bool,
}

#[derive(Clone)]
pub(crate) struct RegistrationAnswerGate {
    selection: Arc<Mutex<Option<Selection>>>,
    changed: watch::Sender<()>,
}

impl Default for RegistrationAnswerGate {
    fn default() -> Self {
        Self {
            selection: Arc::new(Mutex::new(None)),
            changed: watch::channel(()).0,
        }
    }
}

pub struct RegistrationAnswerObservation {
    gate: RegistrationAnswerGate,
    held: watch::Receiver<bool>,
}

impl RegistrationAnswerGate {
    pub(crate) fn begin(
        &self,
        database: DatabaseId,
    ) -> FixtureResult<RegistrationAnswerObservation> {
        let mut selection = self
            .selection
            .lock()
            .map_err(|_| "registration gate poisoned")?;
        if selection.is_some() {
            return Err("registration answer holds must not overlap".into());
        }
        let (held, waiting) = watch::channel(false);
        *selection = Some(Selection {
            database,
            held,
            released: false,
        });
        Ok(RegistrationAnswerObservation {
            gate: self.clone(),
            held: waiting,
        })
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<()> {
        self.changed.subscribe()
    }

    pub(crate) fn defer(&self, bytes: &[u8]) -> FixtureResult<bool> {
        let selection = self
            .selection
            .lock()
            .map_err(|_| "registration gate poisoned")?;
        let Some(selection) = selection.as_ref().filter(|selection| !selection.released) else {
            return Ok(false);
        };
        if matches!(serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(bytes)?,
            ServerFrame::Answer { body: OperationAnswer::Lease(LeaseAnswer::Register(
                RegistrationAnswer::Registered { lease, .. }
            )), .. } if lease.database == selection.database)
        {
            selection.held.send_replace(true);
            return Ok(true);
        }
        Ok(false)
    }

    pub(crate) fn is_held(&self) -> FixtureResult<bool> {
        Ok(self
            .selection
            .lock()
            .map_err(|_| "registration gate poisoned")?
            .as_ref()
            .is_some_and(|selection| !selection.released))
    }
}

impl RegistrationAnswerObservation {
    pub async fn wait_held(&mut self) -> FixtureResult {
        self.held.wait_for(|held| *held).await?;
        Ok(())
    }

    pub fn release(&self) -> FixtureResult {
        if let Some(selection) = self
            .gate
            .selection
            .lock()
            .map_err(|_| "registration gate poisoned")?
            .as_mut()
        {
            selection.released = true;
        }
        self.gate.changed.send_replace(());
        Ok(())
    }
}

impl Drop for RegistrationAnswerObservation {
    fn drop(&mut self) {
        if let Ok(mut selection) = self.gate.selection.lock() {
            *selection = None;
            self.gate.changed.send_replace(());
        }
    }
}
