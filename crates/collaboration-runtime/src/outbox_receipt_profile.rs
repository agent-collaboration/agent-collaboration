//! Body-free outbox data retained after a request ends or central answers it.
use central_link_client::{OutboxWriteTarget, RequestStatus, SenderDeliveryOutcome};
use central_link_protocol::ReplyPayload;
use serde::{Deserialize, Serialize};
use workspace_model::{
    RequestId,
    central_mail::{CentralMailAnswer, CentralMailResult},
};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AnsweredWrite {
    pub(crate) request_id: RequestId,
    pub(crate) target: OutboxWriteTarget,
    /// A resubmit must still name the original target and content.
    pub(crate) content_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) delivery_outcome: Option<SenderDeliveryOutcome>,
}

impl AnsweredWrite {
    pub(crate) fn matches_delivery_receipt(&self, status: &RequestStatus) -> bool {
        self.delivery_outcome.as_ref().is_none_or(|notice| {
            self.request_id == notice.request_id
                && matches!(self.target, OutboxWriteTarget::Session { .. })
                && matches!(status, RequestStatus::Answered(ReplyPayload::Mail(
                    CentralMailResult::Done(CentralMailAnswer::Queued { message_id })
                )) if *message_id == notice.message_id)
        })
    }
}
