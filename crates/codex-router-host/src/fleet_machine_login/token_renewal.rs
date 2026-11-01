//! Rotates at the issuer, then renews the existing connection through its ordinary request owner.
use std::sync::Arc;

use board_sync_protocol::{BoardConsumerAnswer, BoardConsumerProtocol, BoardConsumerRequest};
use fleet_identity_protocol::{MachineTokenRenewal, MachineTokenRenewalRefusal, RenewMachineToken};
use fleet_sync_protocol::{OperationAnswer, OperationRequest};
use fleet_sync_tokio_runner::SessionRequests;
use tokio::sync::watch;

use super::{
    device_code_flow::{IssuedMachineToken, MachineTokenIssuer, RefreshCredentialBundle},
    login_publication::MachineLoginPublication,
};

#[derive(Clone)]
pub(crate) struct MachineRenewalConnection {
    pub requests: SessionRequests<BoardConsumerProtocol>,
    pub welcomed: watch::Receiver<bool>,
}

pub(super) async fn renew_machine_token(
    issuer: Arc<dyn MachineTokenIssuer>,
    publication: Arc<MachineLoginPublication>,
    mut credential: RefreshCredentialBundle,
    connection: Option<MachineRenewalConnection>,
) -> Result<IssuedMachineToken, ()> {
    for attempt in 0..2 {
        let issued = issuer.refresh(&credential).await.map_err(|reason| {
            tracing::warn!(event = "machineTokenRenewal", outcome = "requiresLogin", reason = ?reason, enrollment_id = credential.machine_client_id.as_str(), "machine token refresh failed");
        })?;
        publication
            .publish_rotation(&issued.credential)
            .map_err(|reason| {
                tracing::warn!(event = "machineTokenRenewal", outcome = "requiresLogin", reason = ?reason, enrollment_id = credential.machine_client_id.as_str(), "rotated credential publication unconfirmed");
            })?;
        let Some(connection) = &connection else {
            tracing::info!(
                event = "machineTokenRenewal",
                outcome = "refreshedForConnect",
                enrollment_id = issued.credential.machine_client_id.as_str(),
                "fresh machine token available for the next connection"
            );
            return Ok(issued);
        };
        if !*connection.welcomed.borrow() {
            // No open connection needs renewing. Its next connect obtains this
            // token from the Host source, never from a captured startup value.
            tracing::info!(
                event = "machineTokenRenewal",
                outcome = "refreshedForConnect",
                enrollment_id = issued.credential.machine_client_id.as_str(),
                "fresh machine token available for the next connection"
            );
            return Ok(issued);
        }
        let answer = connection
            .requests
            .request(OperationRequest::Consumer(
                BoardConsumerRequest::RenewMachineToken(RenewMachineToken {
                    token: issued.token.expose_secret().to_owned(),
                }),
            ))
            .await;
        match answer {
            Ok(OperationAnswer::Consumer(BoardConsumerAnswer::RenewMachineToken(
                MachineTokenRenewal::Renewed {},
            ))) => {
                tracing::info!(
                    event = "machineTokenRenewal",
                    outcome = "renewed",
                    enrollment_id = issued.credential.machine_client_id.as_str(),
                    "machine connection token renewed"
                );
                return Ok(issued);
            }
            Ok(OperationAnswer::Consumer(BoardConsumerAnswer::RenewMachineToken(
                MachineTokenRenewal::Refused {
                    reason: MachineTokenRenewalRefusal::EnrollmentNotActive {},
                },
            ))) => {
                tracing::warn!(
                    event = "machineTokenRenewal",
                    outcome = "requiresLogin",
                    reason = "enrollmentNotActive",
                    enrollment_id = credential.machine_client_id.as_str(),
                    "machine connection renewal refused"
                );
                return Err(());
            }
            Ok(OperationAnswer::Consumer(BoardConsumerAnswer::RenewMachineToken(
                MachineTokenRenewal::Refused { reason },
            ))) if attempt == 0 => {
                tracing::info!(event = "machineTokenRenewal", outcome = "refreshOnceMore", reason = ?reason, enrollment_id = credential.machine_client_id.as_str(), "machine connection renewal refused");
                credential = issued.credential;
            }
            Ok(OperationAnswer::Consumer(BoardConsumerAnswer::RenewMachineToken(
                MachineTokenRenewal::Refused { reason },
            ))) => {
                tracing::warn!(event = "machineTokenRenewal", outcome = "requiresLogin", reason = ?reason, enrollment_id = credential.machine_client_id.as_str(), "machine connection renewal refused after one extra refresh");
                return Err(());
            }
            // The ordinary pump owns reconnects. A lost connection/reply does
            // not discard a fresh issuer token; the next connect presents it.
            Err(_) => {
                tracing::info!(
                    event = "machineTokenRenewal",
                    outcome = "refreshedForReconnect",
                    enrollment_id = issued.credential.machine_client_id.as_str(),
                    "machine connection renewal reply unavailable"
                );
                return Ok(issued);
            }
            Ok(_) => {
                tracing::warn!(
                    event = "machineTokenRenewal",
                    outcome = "requiresLogin",
                    reason = "renewalNotConfirmed",
                    enrollment_id = credential.machine_client_id.as_str(),
                    "machine connection renewal failed"
                );
                return Err(());
            }
        }
    }
    Err(())
}

#[cfg(test)]
#[path = "token_renewal_tests.rs"]
mod tests;
