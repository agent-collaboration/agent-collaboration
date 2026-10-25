//! Enrich an epoch-only library settlement using the current public Registry snapshot.
use super::{CoreHost, PlatformIo, SharedTaskCaller, no_route};
use crate::{
    RegistryIncarnation,
    fleet_lease_calls::{AdvanceLeaseIncarnation, AdvanceLeaseIncarnationRequest, ClaimGranted},
    lease_projection::{ClaimGrantedUpdate, LeaseProjection, LeaseProjectionResult},
    registry_incarnation_run::{IncarnationRun, RunAcquisition},
    registry_lease_store::{LeaseSnapshotRead, RegistryLeaseStore},
    registry_publication_startup::PublicationStartup,
};
use durable_steps::{CallFailure, CallId, CallTarget};
use fleet_sync_protocol::{
    ClaimSettlement, HolderChangeRequest, SettleOutcome, SettlementRecorded,
};
use std::sync::Arc;

pub(crate) struct RegistryLeasePublication {
    pub(super) store: Arc<dyn RegistryLeaseStore>,
    pub(super) run: IncarnationRun,
    pub(super) startup: PublicationStartup,
}
impl RegistryLeasePublication {
    pub fn new(store: Arc<dyn RegistryLeaseStore>) -> Self {
        Self {
            store,
            run: IncarnationRun::default(),
            startup: PublicationStartup::default(),
        }
    }
}
impl<Io: PlatformIo> CoreHost<Io> {
    pub(super) async fn settle_granted_claim(
        &self,
        request: ClaimSettlement,
        identity: CallId,
    ) -> Result<serde_json::Value, CallFailure> {
        let SharedTaskCaller::Registry {
            lease_publication: Some(publication),
            ..
        } = &self.caller
        else {
            return Err(no_route());
        };
        let HolderChangeRequest::Claim {
            attempt, database, ..
        } = request.change
        else {
            return Err(no_route());
        };
        let SettleOutcome::Granted { epoch } = request.outcome else {
            return Err(no_route());
        };
        let LeaseSnapshotRead::Available { snapshot, holder } = publication
            .store
            .snapshot(database)
            .await
            .map_err(|_| CallFailure::AnswerLost)?
        else {
            return Err(no_route());
        };
        // A later committed grant or retirement may have superseded the original grant.
        if snapshot.revision.epoch < epoch {
            return Err(CallFailure::AnswerLost);
        }
        let incarnation = self.acquire_registry_incarnation(publication).await?;
        let projection = LeaseProjection::from_snapshot(snapshot, holder, incarnation)
            .map_err(|_| CallFailure::AnswerLost)?;
        let result = self
            .registry_fleet_call(
                ClaimGranted,
                ClaimGrantedUpdate {
                    attempt,
                    projection,
                },
                identity,
            )
            .await?;
        match result {
            LeaseProjectionResult::Applied
            | LeaseProjectionResult::Duplicate
            | LeaseProjectionResult::Older { .. } => {
                serde_json::to_value(SettlementRecorded).map_err(|_| CallFailure::AnswerLost)
            }
            LeaseProjectionResult::Fenced { fence } => {
                publication.run.invalidate(fence.value())?;
                Err(CallFailure::AnswerLost)
            }
            LeaseProjectionResult::Conflict { stored } => {
                publication.run.invalidate(stored.incarnation().value())?;
                Err(CallFailure::AnswerLost)
            }
        }
    }
    pub(super) async fn acquire_registry_incarnation(
        &self,
        publication: &RegistryLeasePublication,
    ) -> Result<RegistryIncarnation, CallFailure> {
        let acquisition = match publication.run.acquire()? {
            RunAcquisition::Active(incarnation) => return Ok(incarnation),
            RunAcquisition::Acquire(acquisition) => acquisition,
        };
        let at_least = publication
            .store
            .last_incarnation()
            .await
            .map_err(|_| CallFailure::AnswerLost)?
            .max(acquisition.floor);
        let incarnation = self
            .registry_fleet_call(
                AdvanceLeaseIncarnation,
                AdvanceLeaseIncarnationRequest { at_least },
                self.call_identity("lease_incarnation")?,
            )
            .await?;
        if incarnation.value() <= at_least {
            return Err(CallFailure::AnswerLost);
        }
        publication
            .store
            .remember_incarnation(incarnation, self.io.new_id())
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        acquisition.activate(incarnation)
    }
    pub(super) async fn registry_fleet_call<Call: CallTarget>(
        &self,
        target: Call,
        request: Call::Request,
        identity: CallId,
    ) -> Result<Call::Answer, CallFailure> {
        let call = self.classify(target, request, identity).await?;
        serde_json::from_slice(&self.transmit(call).await?).map_err(|_| CallFailure::AnswerLost)
    }
}
