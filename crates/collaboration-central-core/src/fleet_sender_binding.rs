//! Resolve sealed consumer senders from Fleet enrollment truth in the custody transaction.
use board_sync_protocol::BoardDeliveryTypes;
use collaboration_mail_protocol::MailSender;
use durable_steps::storage::TransactionScope;
use fleet_sync_delivery::{SenderBinding, SenderBindingFailure};
use fleet_sync_protocol::Submitter;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub struct FleetSenderBinding;

#[cfg(test)]
#[path = "fleet_sender_binding_test.rs"]
mod tests;

impl<S: TransactionScope> SenderBinding<S, BoardDeliveryTypes> for FleetSenderBinding
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'value> String:
        Type<S::Database> + Encode<'value, S::Database> + Decode<'value, S::Database>,
    for<'value> Option<String>:
        Type<S::Database> + Encode<'value, S::Database> + Decode<'value, S::Database>,
    for<'row> &'row str: ColumnIndex<<S::Database as Database>::Row>,
{
    #[expect(
        clippy::manual_async_fn,
        reason = "Explicit Send preserves higher-ranked borrowed custody scope futures."
    )]
    fn resolve_submitter<'a>(
        &'a self,
        scope: &'a mut S,
        sender: &'a MailSender,
    ) -> impl std::future::Future<Output = Result<Submitter, SenderBindingFailure>> + Send + 'a
    {
        async move {
            let installation = match sender {
                MailSender::Fleet => return Err(SenderBindingFailure::Unknown),
                MailSender::Project { project } => {
                    return fleet_sync_protocol::DatabaseId::try_from(project.as_str().to_owned())
                        .map(Submitter::Project)
                        .map_err(|_| SenderBindingFailure::Conflict);
                }
                MailSender::Session(session) => &session.endpoint.service_id,
                MailSender::HumanUser { installation } => installation,
            };
            let mut rows = crate::generated_enrollment::read_enrollments(
                scope,
                Some(installation.as_str().to_owned()),
                None,
                None,
                None,
            )
            .await
            .map_err(|_| SenderBindingFailure::Unavailable)?;
            if rows.len() > 1 {
                return Err(SenderBindingFailure::Conflict);
            }
            let row = rows.pop().ok_or(SenderBindingFailure::Unknown)?;
            let identity = crate::machine_enrollment_record::identity_from_stored_enrollment(row)
                .map_err(|_| SenderBindingFailure::Unavailable)?;
            if identity.origin.installation != *installation {
                return Err(SenderBindingFailure::Conflict);
            }
            // Revocation does not rewrite a historical sender's identity. The
            // existing admission and live recipient gates own liveness checks.
            Ok(Submitter::Machine(identity.peer))
        }
    }
}
