use central_link_protocol::{CorrelationId, LinkRequest, ReplyPayload};
use router_clock::ClockInstant;
use std::collections::{BTreeMap, BTreeSet};
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
    DefinitelyNotSent,
    MayHaveBeenSent,
}

pub(crate) enum ReplyDisposition {
    Completed,
    AlreadyResolved,
}

struct PendingCall {
    request: LinkRequest,
    expires_at: ClockInstant,
}

#[derive(Default)]
pub(crate) struct PendingCalls {
    active: BTreeMap<CorrelationId, PendingCall>,
    resolved: BTreeSet<CorrelationId>,
}

impl PendingCalls {
    pub(crate) fn send_failed(
        &mut self,
        correlation: &CorrelationId,
        receipt: SendFailure,
    ) -> Result<FailedCall, crate::ClientError> {
        let call = self.retire(correlation)?;
        Ok(failed_call(correlation.clone(), call, receipt))
    }

    pub(crate) fn insert(
        &mut self,
        request: LinkRequest,
        expires_at: ClockInstant,
    ) -> Result<(), crate::ClientError> {
        use std::collections::btree_map::Entry;
        if self.resolved.contains(request.correlation_id()) {
            return Err(crate::ClientError::DuplicateCorrelation);
        }
        match self.active.entry(request.correlation_id().clone()) {
            Entry::Occupied(_) => Err(crate::ClientError::DuplicateCorrelation),
            Entry::Vacant(entry) => {
                entry.insert(PendingCall {
                    request,
                    expires_at,
                });
                Ok(())
            }
        }
    }

    pub(crate) fn complete(
        &mut self,
        correlation: &CorrelationId,
        reply: &ReplyPayload,
    ) -> Result<ReplyDisposition, crate::ClientError> {
        if self.resolved.contains(correlation) {
            return Ok(ReplyDisposition::AlreadyResolved);
        }
        let call = self
            .active
            .get(correlation)
            .ok_or(crate::ClientError::UnknownCorrelation)?;
        if !reply.matches_request(call.request.payload()) {
            return Err(crate::ClientError::WrongReplyFamily);
        }
        self.retire(correlation)?;
        Ok(ReplyDisposition::Completed)
    }

    pub(crate) fn next_deadline(&self) -> Option<ClockInstant> {
        self.active.values().map(|call| call.expires_at).min()
    }

    pub(crate) fn expire(&mut self, now: ClockInstant) -> Vec<FailedCall> {
        #[expect(
            clippy::needless_collect,
            reason = "finish the immutable traversal before removing entries from the same map"
        )]
        let due: Vec<_> = self
            .active
            .iter()
            .filter(|(_, call)| call.expires_at <= now)
            .map(|(id, _)| id.clone())
            .collect();
        due.into_iter()
            .filter_map(|id| {
                self.active.remove(&id).map(|call| {
                    self.resolved.insert(id.clone());
                    failed_call(id, call, SendFailure::MayHaveBeenSent)
                })
            })
            .collect()
    }

    pub(crate) fn fail_all(&mut self) -> Vec<FailedCall> {
        std::mem::take(&mut self.active)
            .into_iter()
            .map(|(id, call)| {
                self.resolved.insert(id.clone());
                failed_call(id, call, SendFailure::MayHaveBeenSent)
            })
            .collect()
    }

    pub(crate) fn begin_connection(&mut self) {
        self.resolved.clear();
    }

    fn retire(&mut self, correlation: &CorrelationId) -> Result<PendingCall, crate::ClientError> {
        let call = self
            .active
            .remove(correlation)
            .ok_or(crate::ClientError::UnknownCorrelation)?;
        self.resolved.insert(correlation.clone());
        Ok(call)
    }
}

fn failed_call(
    correlation_id: CorrelationId,
    call: PendingCall,
    receipt: SendFailure,
) -> FailedCall {
    let failure = match (receipt, call.request.payload().write_request_id()) {
        (SendFailure::MayHaveBeenSent, Some(request_id)) => CallFailure::OutcomeUnknown {
            request_id: request_id.clone(),
        },
        (SendFailure::DefinitelyNotSent, _) | (SendFailure::MayHaveBeenSent, None) => {
            CallFailure::CentralUnavailable
        }
    };
    FailedCall {
        correlation_id,
        failure,
    }
}
