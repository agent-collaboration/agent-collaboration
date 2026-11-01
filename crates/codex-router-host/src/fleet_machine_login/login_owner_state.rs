//! The lifetime owner's confirmed public facts and attached display observers.
use super::{
    device_code_flow::{MachineTokenIssuer, RefreshCredentialBundle},
    login_contract::FleetLoginEvent,
};
use codex_router_fleet_state::fleet_login::SavedFleetLogin;
use std::sync::Arc;
use tokio::sync::mpsc;

pub(super) enum SavedLogin {
    Fresh,
    RequiresApproval(Arc<SavedFleetLogin>),
    Restoring {
        saved: Arc<SavedFleetLogin>,
        credential: RefreshCredentialBundle,
    },
    Ready {
        saved: Arc<SavedFleetLogin>,
        credential: RefreshCredentialBundle,
        issuer: Arc<dyn MachineTokenIssuer>,
    },
}
impl SavedLogin {
    pub(super) fn saved(&self) -> Option<&Arc<SavedFleetLogin>> {
        match self {
            Self::Fresh => None,
            Self::RequiresApproval(saved)
            | Self::Restoring { saved, .. }
            | Self::Ready { saved, .. } => Some(saved),
        }
    }
    pub(super) fn require_approval(&mut self) {
        if let Some(saved) = self.saved().cloned() {
            *self = Self::RequiresApproval(saved);
        }
    }
}

pub(super) struct LoginObservers {
    pub code: Option<FleetLoginEvent>,
    pub observers: Vec<mpsc::Sender<FleetLoginEvent>>,
}
impl LoginObservers {
    pub(super) fn attach(&mut self, observer: mpsc::Sender<FleetLoginEvent>) {
        if let Some(code) = &self.code {
            let _sent = observer.try_send(code.clone());
        }
        self.observers.retain(|observer| !observer.is_closed());
        self.observers.push(observer);
    }
    pub(super) fn show(&mut self, code: FleetLoginEvent) {
        self.observers
            .retain(|observer| observer.try_send(code.clone()).is_ok());
        self.code = Some(code);
    }
    pub(super) fn finish(self, event: FleetLoginEvent) {
        for observer in self.observers {
            let _sent = observer.try_send(event.clone());
        }
    }
}
