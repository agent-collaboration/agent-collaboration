//! Wake lifetime and provenance checks precede any slot replacement writes.
use crate::{
    automation_catalog_record::timestamp_millis,
    automation_store_error::{AutomationStoreError, AutomationStoreResult},
};
use collaboration_protocol::{
    AutomationRefusal, AutomationTiming, AutomationValidationError, ExpiryRequest,
    FiringMessageOrigin, MessageContent, SavedMessage, TimingRequest, WakeCreator,
    limits::{LimitName, WAKE_ONE_SHOT_MAX_AHEAD},
};
use message_board::ServiceId;

pub(crate) fn origin(
    installation: ServiceId,
    creator: &WakeCreator,
    message: &SavedMessage,
) -> AutomationStoreResult<FiringMessageOrigin> {
    if matches!(creator, WakeCreator::Unrecorded {}) {
        return Err(AutomationStoreError::InvalidRequest {
            field: "requestedBy",
            reason: collaboration_protocol::AutomationRequestConstraint::MalformedShape,
        });
    }
    if matches!(creator,WakeCreator::Session(session) if String::from(session.endpoint.service_id.clone())!=installation.as_str())
    {
        return Err(AutomationStoreError::Refused(
            AutomationRefusal::OriginMismatch,
        ));
    }
    match &message.content {
        MessageContent::Agent { sender, .. } => {
            if String::from(sender.endpoint.service_id.clone()) != installation.as_str() {
                return Err(AutomationStoreError::Refused(
                    AutomationRefusal::OriginMismatch,
                ));
            }
            Ok(FiringMessageOrigin::Agent {
                sender: sender.clone(),
            })
        }
        MessageContent::HumanUser { .. } => Ok(FiringMessageOrigin::HumanUser),
        MessageContent::Router { .. } => Ok(FiringMessageOrigin::Router),
    }
}

pub(crate) fn expiry(
    request: &ExpiryRequest,
    timing_request: &TimingRequest,
    timing: &AutomationTiming,
    now_ms: i64,
) -> AutomationStoreResult<i64> {
    let end = match request {
        ExpiryRequest::None => None,
        ExpiryRequest::At { at } => Some(timestamp_millis(at)?),
        ExpiryRequest::After { seconds } => Some(
            now_ms
                .checked_add(i64::from(u32::from(*seconds)) * 1000)
                .ok_or(AutomationValidationError::TimeOutOfRange)?,
        ),
    };
    let due = timing.first_due().unix_millis();
    if matches!(timing_request, TimingRequest::At { .. }) {
        LimitName::WakeOneShotMaxAhead
            .validate(seconds_ceil(due.saturating_sub(now_ms).max(0))?)?;
        let cap = due
            .checked_add(i64::from(WAKE_ONE_SHOT_MAX_AHEAD) * 1000)
            .ok_or(AutomationValidationError::TimeOutOfRange)?;
        if let Some(end) = end.filter(|end| *end > cap) {
            LimitName::WakeOneShotMaxAhead.validate(seconds_ceil(
                end.checked_sub(due)
                    .ok_or(AutomationValidationError::TimeOutOfRange)?,
            )?)?;
        }
        return Ok(end.unwrap_or(cap));
    }
    let end = end.ok_or(AutomationStoreError::Refused(
        AutomationRefusal::WakeExpiryRequired,
    ))?;
    if end > now_ms {
        LimitName::WakeRepeatMaxLifetime.validate(seconds_ceil(
            end.checked_sub(now_ms)
                .ok_or(AutomationValidationError::TimeOutOfRange)?,
        )?)?;
    }
    Ok(end)
}

fn seconds_ceil(millis: i64) -> AutomationStoreResult<u64> {
    let millis = u64::try_from(millis).map_err(|_| AutomationValidationError::TimeOutOfRange)?;
    Ok(millis.div_ceil(1000))
}
