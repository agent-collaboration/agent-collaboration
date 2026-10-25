//! A session may register a writer only under its authenticated peer identity.

use board_sync_protocol::BoardConsumerProtocol;
use fleet_sync_protocol::{LeaseRequest, OperationRequest, RegistrationRefusal};
use fleet_sync_session::SessionKey;

pub(crate) fn refuse_register_from_another_peer(
    session: SessionKey,
    operation: &OperationRequest<BoardConsumerProtocol>,
) -> Option<RegistrationRefusal> {
    match operation {
        OperationRequest::Lease(LeaseRequest::Register(request))
            if request.writer != session.peer =>
        {
            Some(RegistrationRefusal::NotWriter { current: None })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fleet_sync_protocol::{DatabaseId, Origin, PeerId, Registration, RequestId, SessionId};

    #[test]
    fn the_authenticated_writer_is_not_refused_by_the_session_peer_guard()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let peer = PeerId::try_from("019f0000-0000-7000-8000-000000000701".to_owned())?;
        let session = SessionKey {
            peer,
            session_id: SessionId::try_from("019f0000-0000-7000-8000-000000000702".to_owned())?,
        };
        let operation = OperationRequest::Lease(LeaseRequest::Register(Registration {
            attempt: RequestId::try_from("019f0000-0000-7000-8000-000000000703".to_owned())?,
            database: DatabaseId::try_from("019f0000-0000-7000-8000-000000000704".to_owned())?,
            writer: peer,
            origin: Origin::Empty,
        }));
        if refuse_register_from_another_peer(session, &operation).is_some() {
            return Err("the authenticated writer was refused by the session peer guard".into());
        }
        Ok(())
    }
}
