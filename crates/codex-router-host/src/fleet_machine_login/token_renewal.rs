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
        let issued = issuer.refresh(&credential).await.map_err(|_| ())?;
        publication
            .publish_rotation(&issued.credential)
            .map_err(|_| ())?;
        let Some(connection) = &connection else {
            return Ok(issued);
        };
        if !*connection.welcomed.borrow() {
            // No open connection needs renewing. Its next connect obtains this
            // token from the Host source, never from a captured startup value.
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
            ))) => return Ok(issued),
            Ok(OperationAnswer::Consumer(BoardConsumerAnswer::RenewMachineToken(
                MachineTokenRenewal::Refused {
                    reason: MachineTokenRenewalRefusal::EnrollmentNotActive {},
                },
            ))) => return Err(()),
            Ok(OperationAnswer::Consumer(BoardConsumerAnswer::RenewMachineToken(
                MachineTokenRenewal::Refused { .. },
            ))) if attempt == 0 => {
                credential = issued.credential;
            }
            // The ordinary pump owns reconnects. A lost connection/reply does
            // not discard a fresh issuer token; the next connect presents it.
            Err(_) => return Ok(issued),
            Ok(_) => return Err(()),
        }
    }
    Err(())
}
