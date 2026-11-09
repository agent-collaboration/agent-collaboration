//! Claim settlement observes the projection already committed by the Fleet lease hook.
use super::{CoreHost, PlatformIo, SharedTaskCaller, no_route};
use crate::{
    fleet_lease_calls::ClaimGranted,
    lease_projection::{ClaimGrantedUpdate, LeaseProjectionResult},
    registry_lease_store::RegistryLeaseStore,
};
use durable_steps::{CallFailure, CallId, CallTarget, StoreError};
use fleet_protocol::{ClaimSettlement, HolderChangeRequest, SettleOutcome, SettlementRecorded};
use std::sync::Arc;

pub(crate) struct RegistryLeasePublication {
    pub(super) store: Arc<dyn RegistryLeaseStore>,
}
impl RegistryLeasePublication {
    pub fn new(store: Arc<dyn RegistryLeaseStore>) -> Self {
        Self { store }
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
        let projection = publication
            .store
            .projection(database)
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        if projection.snapshot().revision.epoch < epoch {
            return Err(CallFailure::AnswerLost);
        }
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
            LeaseProjectionResult::Conflict { .. } => Err(CallFailure::AnswerLost),
        }
    }
    pub(crate) async fn retirement_directory_acknowledged(
        &self,
        database: fleet_protocol::DatabaseId,
        retired_at: fleet_protocol::SyncTime,
    ) -> Result<bool, StoreError> {
        let SharedTaskCaller::Registry {
            lease_publication: Some(publication),
            ..
        } = &self.caller
        else {
            return Err(StoreError::backend("Fleet has no lease projection reader"));
        };
        let projection = publication.store.projection(database).await?;
        Ok(projection.snapshot().lifecycle
            == (fleet_project_lease::DatabaseLifecycle::Retired { at: retired_at }))
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
