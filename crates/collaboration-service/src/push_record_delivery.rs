//! Native SDK custody precedes direct-message delivery through the reader's actor.
use collaboration_protocol::{PushId, PushRecordDraft, PushRecordValidationError};

#[derive(Debug)]
pub(crate) enum PushDeliveryFailure {
    MailRefused(collaboration_mail_protocol::MailRefusal),
    StoreUnavailable,
    DeliveryUnavailable,
    InvalidRecord(PushRecordValidationError),
    StoreFailed,
    DeliveryUnknown(PushId),
}

pub(crate) async fn store_first_and_deliver(
    draft: PushRecordDraft,
    target_identity: String,
    interaction_store: Option<&std::sync::Arc<interaction_storage::InteractionStore>>,
    mail_delivery: Option<&std::sync::Arc<crate::MailDeliveryRuntime>>,
) -> Result<collaboration_protocol::PushMessageSendResult, PushDeliveryFailure> {
    if interaction_store.is_none() {
        return Err(PushDeliveryFailure::StoreUnavailable);
    }
    let Some(runtime) = mail_delivery else {
        return Err(PushDeliveryFailure::DeliveryUnavailable);
    };
    runtime.send_local(draft, target_identity).await
}

#[cfg(test)]
#[path = "push_record_delivery_test.rs"]
mod tests;
