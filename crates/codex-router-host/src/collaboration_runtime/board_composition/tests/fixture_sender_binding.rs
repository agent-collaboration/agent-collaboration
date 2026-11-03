//! Resolve fixture senders from configured authentication and explicit directory answers.
use super::*;
use fleet_sync_delivery::{SenderBinding, SenderBindingFailure};
use fleet_sync_protocol::Submitter;
impl SenderBinding<ServiceScope<'_>, board_sync_protocol::BoardDeliveryTypes> for Consumer {
    async fn resolve_submitter(
        &self,
        _: &mut ServiceScope<'_>,
        sender: &collaboration_mail_protocol::MailSender,
    ) -> Result<Submitter, SenderBindingFailure> {
        use collaboration_mail_protocol::MailSender;
        let service = match sender {
            MailSender::Project { project } => {
                return DatabaseId::try_from(project.as_str().to_owned())
                    .map(Submitter::Project)
                    .map_err(|_| SenderBindingFailure::Conflict);
            }
            MailSender::Session(session) => &session.endpoint.service_id,
            MailSender::HumanUser { installation } => installation,
        };
        if service.as_str() == "00000000-0000-4000-8000-000000000001" {
            return Ok(Submitter::Machine(self.peer));
        }
        match self
            .directory
            .lookup(service)
            .await
            .map_err(|_| SenderBindingFailure::Unavailable)?
        {
            board_sync_protocol::InstallationPeerLookup::Enrolled { peer } => {
                Ok(Submitter::Machine(peer))
            }
            board_sync_protocol::InstallationPeerLookup::Unknown => {
                Err(SenderBindingFailure::Unknown)
            }
        }
    }
}
