//! Observe SDK settlement and the frozen target through their owners on one read-only connection.
use crate::intent_recovery_rows::{RecoveryError, RecoveryResult, require};
use collaboration_contracts::RouterDeliveryTypes;
use collaboration_protocol::{PushDeliveryState, PushId};
use collaboration_service::MachineSenderBinding;
use durable_steps::{
    CallFailure, CallId, CallTarget, Host, NotSentReason, turso_store::TursoScope,
};
use fleet_mail_delivery::{DeliveryLimits, Inbox, minimum_settlement_allowance};
use fleet_protocol::{ItemId, PeerId, RecipientOutcome};
use message_board::ServiceId;
use runtime_primitives::{Clock, IdSource, SyncTime, SystemClock, WakeFailure, WakeScheduler};
use sqlx::Connection;
use sqlx_turso::{TursoConnectOptions, TursoConnection};
use std::{os::unix::fs::MetadataExt, path::Path};

pub(crate) struct RecoveryCustody {
    connection: TursoConnection,
    binding: MachineSenderBinding,
    peer: PeerId,
    limits: DeliveryLimits,
}
impl RecoveryCustody {
    pub async fn open(path: &Path, service: &ServiceId, peer: PeerId) -> RecoveryResult<Self> {
        let metadata = std::fs::symlink_metadata(path).map_err(|_| RecoveryError::Read)?;
        require(
            metadata.is_file() && metadata.uid() == nix::unistd::geteuid().as_raw(),
            "owned custody file",
        )?;
        let connection = Self::read_connection(path).await?;
        let limits = DeliveryLimits::new::<RouterDeliveryTypes>(
            0,
            0,
            0,
            minimum_settlement_allowance::<RouterDeliveryTypes>()
                .map_err(|_| RecoveryError::Read)?,
        )
        .map_err(|_| RecoveryError::Read)?;
        Ok(Self {
            connection,
            binding: MachineSenderBinding::new(service.clone(), peer),
            peer,
            limits,
        })
    }
    async fn read_connection(path: &Path) -> RecoveryResult<TursoConnection> {
        TursoConnection::connect_with(
            &TursoConnectOptions::new()
                .filename(path)
                .create_if_missing(false)
                .read_only(true),
        )
        .await
        .map_err(|_| RecoveryError::Read)
    }
    pub async fn terminal(
        &mut self,
        item: ItemId,
        target: &message_board::SessionRef,
    ) -> RecoveryResult<()> {
        let host = CustodyReadHost;
        let mut scope = TursoScope::new(&mut self.connection, &host);
        let outcome = Inbox::<_, RouterDeliveryTypes, _>::new(
            &mut scope,
            self.peer,
            self.limits,
            &self.binding,
        )
        .local_outcome(item)
        .await
        .map_err(|_| RecoveryError::Read)?;
        require(
            matches!(outcome, Some(outcome) if !matches!(outcome, RecipientOutcome::Held)),
            "terminal local custody missing",
        )?;
        let id: PushId = item
            .as_uuid()
            .to_string()
            .try_into()
            .map_err(|_| RecoveryError::Invalid("custody item identifier"))?;
        let push = interaction_storage::read_mail_push_in_scope(&mut scope, &id)
            .await
            .map_err(|_| RecoveryError::Read)?
            .ok_or(RecoveryError::Invalid("custody target missing"))?;
        let target: collaboration_protocol::SessionRef = target
            .clone()
            .try_into()
            .map_err(|_| RecoveryError::Invalid("custody target identity"))?;
        require(
            push.push_id == id
                && push.target == target
                && push.last_outcome.is_some()
                && push.settled_at.is_some()
                && matches!(
                    push.delivery_state,
                    PushDeliveryState::Delivered
                        | PushDeliveryState::OutcomeUnknown
                        | PushDeliveryState::Rejected
                ),
            "terminal custody target or item",
        )
    }
    pub async fn close(self) -> RecoveryResult<()> {
        self.connection
            .close()
            .await
            .map_err(|_| RecoveryError::Read)
    }
}

// The SDK read needs a scope Host type; it cannot dispatch, schedule, or own a runner.
struct CustodyReadHost;
impl Clock for CustodyReadHost {
    fn now(&self) -> SyncTime {
        SystemClock.now()
    }
}
impl IdSource for CustodyReadHost {
    fn new_id(&self) -> durable_steps::Uuid {
        uuid::Uuid::now_v7()
    }
}
impl WakeScheduler for CustodyReadHost {
    async fn wake_at(&self, _: SyncTime) -> Result<(), WakeFailure> {
        Err(WakeFailure::Unavailable {
            message: "offline custody observer cannot schedule".to_owned(),
        })
    }
}
impl Host for CustodyReadHost {
    async fn dispatch<C: CallTarget>(
        &self,
        _: C,
        _: C::Request,
        _: CallId,
    ) -> Result<C::Answer, CallFailure> {
        Err(CallFailure::NotSent {
            reason: NotSentReason::Overloaded,
        })
    }
}

#[cfg(test)]
#[path = "recovery_custody_test.rs"]
mod tests;
