use durable_steps::CallId;
use fleet_sync_lease::{GateFailure, HolderChangeGate, LifecycleGate};
use fleet_sync_protocol::{
    AuthRefusal, DatabaseId, GateAnswer, HolderChangeRequest, NoConsumer, PeerId, SettleOutcome,
};
use fleet_sync_receiver::{
    AppliedPiece, FollowUpDone, FollowUpFailure, FollowUpHandler, HookError, ReceiverRules,
};
use fleet_sync_service::{ConsumerHandler, ServiceConsumer, ServiceError, ServiceScope};
use fleet_sync_session::{
    ClientSessionState, PeerAuthenticator, SessionClientStore, StoreError, VerifiedPeer,
};
use runtime_primitives::{Clock, SystemClock};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

#[derive(Default)]
pub struct SessionConsumer {
    pub requests: AtomicU64,
}

impl ConsumerHandler<NoConsumer> for SessionConsumer {
    async fn request(&self, _: PeerId, (): ()) -> Result<(), ServiceError> {
        self.requests.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

impl ServiceConsumer<NoConsumer> for SessionConsumer {
    type HookItem = ();
}

impl ReceiverRules<ServiceScope<'_>> for SessionConsumer {
    type Protocol = NoConsumer;
    type FollowUp = ();

    async fn on_applied(
        &self,
        _: &mut ServiceScope<'_>,
        _: &AppliedPiece<'_, ()>,
    ) -> Result<Vec<()>, HookError> {
        Ok(Vec::new())
    }
}

impl FollowUpHandler for SessionConsumer {
    type Item = ();

    async fn follow_up(&self, _: &(), _: CallId) -> Result<FollowUpDone, FollowUpFailure> {
        Ok(FollowUpDone)
    }
}

impl LifecycleGate for SessionConsumer {
    async fn admit_registration(
        &self,
        _: &DatabaseId,
        _: &PeerId,
        _: CallId,
    ) -> Result<GateAnswer, GateFailure> {
        Ok(GateAnswer::Admitted)
    }

    async fn admit_retirement(&self, _: &DatabaseId, _: CallId) -> Result<GateAnswer, GateFailure> {
        Ok(GateAnswer::Admitted)
    }
}

impl HolderChangeGate for SessionConsumer {
    async fn reserve(&self, _: &HolderChangeRequest, _: CallId) -> Result<GateAnswer, GateFailure> {
        Ok(GateAnswer::Admitted)
    }

    async fn settle(
        &self,
        _: &HolderChangeRequest,
        _: SettleOutcome,
        _: CallId,
    ) -> Result<(), GateFailure> {
        Ok(())
    }
}

pub struct SessionAuthenticator {
    pub peer: PeerId,
}

impl PeerAuthenticator for SessionAuthenticator {
    type Material = String;
    type Route = ();

    async fn verify(&self, material: String) -> Result<(VerifiedPeer, ()), AuthRefusal> {
        match material.as_str() {
            "Bearer fixture-expired" => return Err(AuthRefusal::Expired),
            "Bearer fixture-revoked" => return Err(AuthRefusal::Revoked),
            "Bearer fixture-wrong-kind" => return Err(AuthRefusal::WrongKind),
            _ => {}
        }
        if material != "Bearer fixture-machine" {
            return Err(AuthRefusal::Unknown);
        }
        Ok((
            VerifiedPeer {
                peer: self.peer,
                expires_at: SystemClock.now().after(Duration::from_secs(3600)),
            },
            (),
        ))
    }
}

// Same-process reconnect only. This fixture makes no restart durability claim.
#[derive(Clone, Default)]
pub struct MemorySessionStore(pub Arc<Mutex<Option<ClientSessionState>>>);

impl SessionClientStore for MemorySessionStore {
    #[expect(
        clippy::unwrap_used,
        reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
    )]
    async fn load(&self) -> Result<Option<ClientSessionState>, StoreError> {
        Ok(self.0.lock().unwrap().clone())
    }

    #[expect(
        clippy::unwrap_used,
        reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
    )]
    async fn save(&self, state: &ClientSessionState) -> Result<(), StoreError> {
        *self.0.lock().unwrap() = Some(state.clone());
        Ok(())
    }
}
