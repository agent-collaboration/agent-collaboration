//! The private link's provisioning port: each request becomes one call to the
//! named organization's Fleet object, with the sign-in link as its authority.
use crate::{
    CallerContext, CentralPlatform, EdgeKind, FleetAddress, FleetKind,
    identity_contract::{
        OrganizationStatementDigestReply, ProvisionOrganization, ProvisionOrganizationResult,
        ReadOrganizationStatementDigest,
    },
    organization_authority::{
        OrganizationProvisioningFailure, OrganizationProvisioningPort, ProvisioningApplied,
    },
    organization_provisioning_calls::{
        FleetOrganizationStatementDigest, FleetProvisionOrganization, StatementDigestAnswer,
    },
};
use durable_steps::{CallFailure, CallId, CallTarget, OperationId, StepName};
use fleet_identity_protocol::OrganizationId;

/// Build the link router over a host's object transport.
pub fn organization_link_router<Platform: CentralPlatform>(platform: Platform) -> axum::Router {
    crate::organization_provisioning_router(std::sync::Arc::new(FleetLinkProvisioner { platform }))
}

struct FleetLinkProvisioner<Platform> {
    platform: Platform,
}

impl<Platform: CentralPlatform> FleetLinkProvisioner<Platform> {
    async fn call<Call: CallTarget>(
        &self,
        organization: &OrganizationId,
        target: Call,
        request: Call::Request,
        step: &str,
    ) -> Result<Call::Answer, OrganizationProvisioningFailure> {
        let caller = CallerContext::<EdgeKind>::for_sign_in_link(organization.clone());
        let identity = CallId {
            operation: OperationId::new(self.platform.ids().new_id())
                .map_err(|_| OrganizationProvisioningFailure::Unavailable)?,
            step: StepName::new(step).map_err(|_| OrganizationProvisioningFailure::Unavailable)?,
        };
        let call = crate::object_call_envelope::encode_shared_call::<EdgeKind, FleetKind, Call>(
            &caller,
            FleetAddress::new(organization.clone()),
            target,
            request,
            identity,
        )
        .map_err(|_| OrganizationProvisioningFailure::Unavailable)?;
        match self.platform.transport_object(call).await {
            Ok(reply) => serde_json::from_slice(&reply)
                .map_err(|_| OrganizationProvisioningFailure::OutcomeUnknown),
            // Refused before dispatch: nothing was applied.
            Err(CallFailure::NotSent { .. }) => Err(OrganizationProvisioningFailure::Unavailable),
            // The Fleet may have committed; only a repeat can tell.
            Err(_) => Err(OrganizationProvisioningFailure::OutcomeUnknown),
        }
    }
}

impl<Platform: CentralPlatform> OrganizationProvisioningPort for FleetLinkProvisioner<Platform> {
    async fn provision_organization(
        &self,
        request: &ProvisionOrganization,
    ) -> Result<ProvisionOrganizationResult, OrganizationProvisioningFailure> {
        match self
            .call(
                &request.organization_id,
                FleetProvisionOrganization,
                request.clone(),
                "provision_organization",
            )
            .await?
        {
            ProvisioningApplied::Provisioned => Ok(ProvisionOrganizationResult::Provisioned {}),
            ProvisioningApplied::OrganizationMismatch => {
                Err(OrganizationProvisioningFailure::OrganizationMismatch)
            }
            ProvisioningApplied::InvalidStatement => {
                Err(OrganizationProvisioningFailure::InvalidStatement)
            }
            ProvisioningApplied::StaleStatement => {
                Err(OrganizationProvisioningFailure::StaleStatement)
            }
        }
    }

    async fn read_statement_digest(
        &self,
        request: &ReadOrganizationStatementDigest,
    ) -> Result<OrganizationStatementDigestReply, OrganizationProvisioningFailure> {
        match self
            .call(
                &request.organization_id,
                FleetOrganizationStatementDigest,
                (),
                "read_statement_digest",
            )
            .await?
        {
            StatementDigestAnswer::Current {
                digest,
                expires_at,
                signing_key_id,
            } => Ok(OrganizationStatementDigestReply {
                digest,
                expires_at,
                signing_key_id,
            }),
            StatementDigestAnswer::Unprovisioned => {
                Err(OrganizationProvisioningFailure::Unprovisioned)
            }
        }
    }
}
