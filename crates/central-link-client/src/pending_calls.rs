use central_link_protocol::{CorrelationId, LinkRequest, ReplyPayload};
use std::collections::BTreeMap;
use workspace_model::RequestId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CallFailure {
    CentralUnavailable,
    OutcomeUnknown { request_id: RequestId },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailedCall {
    pub correlation_id: CorrelationId,
    pub failure: CallFailure,
}

/// A transport receipt, rather than an inference from the shape of an I/O error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SendFailure {
    NotSent,
    MayHaveBeenSent,
}

enum SendProgress {
    NotSent,
    MayHaveBeenSent,
}
struct PendingCall {
    request: LinkRequest,
    progress: SendProgress,
}

#[derive(Default)]
pub(crate) struct PendingCalls(BTreeMap<CorrelationId, PendingCall>);

impl PendingCalls {
    pub(crate) fn send_failed(
        &mut self,
        correlation: &CorrelationId,
        receipt: SendFailure,
    ) -> Result<FailedCall, crate::ClientError> {
        let call = self
            .0
            .remove(correlation)
            .ok_or(crate::ClientError::UnknownCorrelation)?;
        let failure = match (receipt, call.request.payload().write_request_id()) {
            (SendFailure::MayHaveBeenSent, Some(request_id)) => CallFailure::OutcomeUnknown {
                request_id: request_id.clone(),
            },
            (SendFailure::NotSent, _) | (SendFailure::MayHaveBeenSent, None) => {
                CallFailure::CentralUnavailable
            }
        };
        Ok(FailedCall {
            correlation_id: correlation.clone(),
            failure,
        })
    }

    pub(crate) fn insert(&mut self, request: LinkRequest) -> Result<(), crate::ClientError> {
        use std::collections::btree_map::Entry;
        match self.0.entry(request.correlation_id().clone()) {
            Entry::Occupied(_) => Err(crate::ClientError::DuplicateCorrelation),
            Entry::Vacant(entry) => {
                entry.insert(PendingCall {
                    request,
                    progress: SendProgress::NotSent,
                });
                Ok(())
            }
        }
    }

    pub(crate) fn mark_may_have_been_sent(
        &mut self,
        correlation: &CorrelationId,
    ) -> Result<(), crate::ClientError> {
        let call = self
            .0
            .get_mut(correlation)
            .ok_or(crate::ClientError::UnknownCorrelation)?;
        call.progress = SendProgress::MayHaveBeenSent;
        Ok(())
    }

    pub(crate) fn complete(
        &mut self,
        correlation: &CorrelationId,
        reply: &ReplyPayload,
    ) -> Result<(), crate::ClientError> {
        let call = self
            .0
            .get(correlation)
            .ok_or(crate::ClientError::UnknownCorrelation)?;
        if !reply.matches_request(call.request.payload()) {
            return Err(crate::ClientError::WrongReplyFamily);
        }
        if matches!(call.progress, SendProgress::NotSent) {
            return Err(crate::ClientError::ReplyBeforeSend);
        }
        self.0.remove(correlation);
        Ok(())
    }

    pub(crate) fn fail_all(&mut self) -> Vec<FailedCall> {
        std::mem::take(&mut self.0)
            .into_iter()
            .map(|(correlation_id, call)| {
                let failure = match (call.progress, call.request.payload().write_request_id()) {
                    (SendProgress::MayHaveBeenSent, Some(request_id)) => {
                        CallFailure::OutcomeUnknown {
                            request_id: request_id.clone(),
                        }
                    }
                    (SendProgress::NotSent, _) | (SendProgress::MayHaveBeenSent, None) => {
                        CallFailure::CentralUnavailable
                    }
                };
                FailedCall {
                    correlation_id,
                    failure,
                }
            })
            .collect()
    }
}
