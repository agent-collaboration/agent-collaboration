//! One-shot session faults are runtime state, outside the SQL step callback.

use super::{RuntimeControlFailure, RuntimeFaults};
use collaboration_central_core::{SessionFinalizeFailure, SessionFinalizeFault};
use fleet_sync_protocol::ConnectionId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SessionFaultControl {
    ArmBarrier { connection: ConnectionId },
    FailRegistrationPublication { connection: ConnectionId },
    FailFinalize { fault: SessionFinalizeFault },
    Observe,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum FinalizeFaultObservation {
    #[default]
    Idle,
    Armed {
        fault: SessionFinalizeFault,
    },
    Triggered {
        failure: SessionFinalizeFailure,
    },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum PublicationFaultObservation {
    #[default]
    Idle,
    Armed {
        connection: ConnectionId,
    },
    Triggered {
        connection: ConnectionId,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionFaultObservation {
    pub finalize: FinalizeFaultObservation,
    pub publication: PublicationFaultObservation,
}

#[derive(Default)]
pub(super) struct SessionFaultState {
    finalize: FinalizeFaultObservation,
    publication: PublicationFaultObservation,
    barrier: Option<ConnectionId>,
}

impl RuntimeFaults {
    pub fn consume_session_barrier(
        &self,
        connection: ConnectionId,
    ) -> Result<(), RuntimeControlFailure> {
        let mut state = self.state()?;
        if state.session.barrier != Some(connection) {
            return Err(RuntimeControlFailure::NotArmed);
        }
        state.session.barrier = None;
        Ok(())
    }
    pub fn session_finalize_fault(
        &self,
    ) -> Result<Option<SessionFinalizeFault>, RuntimeControlFailure> {
        Ok(match self.state()?.session.finalize {
            FinalizeFaultObservation::Armed { fault } => Some(fault),
            FinalizeFaultObservation::Idle | FinalizeFaultObservation::Triggered { .. } => None,
        })
    }

    pub fn observe_session_finalize_failure(
        &self,
        failure: SessionFinalizeFailure,
    ) -> Result<(), RuntimeControlFailure> {
        let mut state = self.state()?;
        if matches!(state.session.finalize, FinalizeFaultObservation::Armed { fault } if fault == failure.fault())
        {
            state.session.finalize = FinalizeFaultObservation::Triggered { failure };
        }
        Ok(())
    }

    pub fn fail_registration_publication(
        &self,
        connection: ConnectionId,
    ) -> Result<bool, RuntimeControlFailure> {
        let mut state = self.state()?;
        if matches!(state.session.publication, PublicationFaultObservation::Armed { connection: selected } if connection == selected)
        {
            state.session.publication = PublicationFaultObservation::Triggered { connection };
            return Ok(true);
        }
        Ok(false)
    }

    pub fn session_control(
        &self,
        command: SessionFaultControl,
    ) -> Result<SessionFaultObservation, RuntimeControlFailure> {
        let mut state = self.state()?;
        match command {
            SessionFaultControl::ArmBarrier { connection } => {
                if state.session.barrier.is_some() {
                    return Err(RuntimeControlFailure::AlreadyArmed);
                }
                state.session.barrier = Some(connection);
            }
            SessionFaultControl::FailRegistrationPublication { connection } => {
                if matches!(
                    state.session.publication,
                    PublicationFaultObservation::Armed { .. }
                ) {
                    return Err(RuntimeControlFailure::AlreadyArmed);
                }
                state.session.publication = PublicationFaultObservation::Armed { connection };
            }
            SessionFaultControl::FailFinalize { fault } => {
                if matches!(
                    state.session.finalize,
                    FinalizeFaultObservation::Armed { .. }
                ) {
                    return Err(RuntimeControlFailure::AlreadyArmed);
                }
                state.session.finalize = FinalizeFaultObservation::Armed { fault };
            }
            SessionFaultControl::Observe => {}
        }
        Ok(SessionFaultObservation {
            finalize: state.session.finalize.clone(),
            publication: state.session.publication.clone(),
        })
    }
}
