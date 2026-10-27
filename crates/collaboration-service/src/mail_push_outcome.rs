//! Native receipts remain intact while the library owns canonical custody outcomes.
use crate::{NativeDeliveryResult, NativeRefusalCode, RouterDeliveryTypes, UnsentCause};
use collaboration_mail_protocol::CarrierAcceptance;
use collaboration_protocol::DeliveryOutcome;
use fleet_sync_delivery::{DeliveryError, LocalDelivery, LocalOutcome};
use fleet_sync_protocol::{DeliveryFailureKind, DeliveryRefusal, RecipientRefusalCode};
use runtime_primitives::SyncTime;

pub(crate) fn local_message_outcome(
    delivery: NativeDeliveryResult,
    allow_hold: bool,
) -> Result<LocalDelivery<RouterDeliveryTypes>, DeliveryError> {
    let outcome = if let Some(outcome) = observed_outcome(&delivery.receipt().outcome) {
        outcome
    } else {
        match delivery.unsent_cause().ok_or(DeliveryError::Corrupt {
            field: "native delivery classification",
        })? {
            UnsentCause::RecipientUnknown => LocalOutcome::Refused {
                reason: DeliveryRefusal::RecipientUnknown,
            },
            UnsentCause::Refused(code) => recipient_refused(code)?,
            UnsentCause::Holdable { .. } if allow_hold => LocalOutcome::Held { until: None },
            UnsentCause::Holdable { otherwise } => recipient_refused(otherwise)?,
            UnsentCause::InvalidInput => processing_failed(DeliveryFailureKind::InvalidPayload),
            UnsentCause::ProcessingFailure => {
                processing_failed(DeliveryFailureKind::ExecutionFailed)
            }
        }
    };
    Ok(LocalDelivery {
        outcome,
        observation: delivery.into_local_observation(),
    })
}

fn observed_outcome(outcome: &DeliveryOutcome) -> Option<LocalOutcome<CarrierAcceptance>> {
    Some(match outcome {
        DeliveryOutcome::Started => LocalOutcome::Delivered {
            acceptance: CarrierAcceptance::Started,
        },
        DeliveryOutcome::Steered => LocalOutcome::Delivered {
            acceptance: CarrierAcceptance::Steered,
        },
        DeliveryOutcome::StartedOrSteered => LocalOutcome::Delivered {
            acceptance: CarrierAcceptance::StartedOrSteered,
        },
        DeliveryOutcome::Queued => LocalOutcome::Delivered {
            acceptance: CarrierAcceptance::Queued,
        },
        DeliveryOutcome::PeerMessageWritten => LocalOutcome::Delivered {
            acceptance: CarrierAcceptance::PeerMessageWritten,
        },
        DeliveryOutcome::Unknown => LocalOutcome::Uncertain,
        DeliveryOutcome::NotSubmitted { .. } | DeliveryOutcome::Rejected(_) => return None,
    })
}

fn recipient_refused(
    code: NativeRefusalCode,
) -> Result<LocalOutcome<CarrierAcceptance>, DeliveryError> {
    let code = RecipientRefusalCode::new(code.as_str()).map_err(|_| DeliveryError::Corrupt {
        field: "native recipient refusal code",
    })?;
    Ok(LocalOutcome::Refused {
        reason: DeliveryRefusal::RecipientRefused { code },
    })
}

fn processing_failed(kind: DeliveryFailureKind) -> LocalOutcome<CarrierAcceptance> {
    LocalOutcome::Refused {
        reason: DeliveryRefusal::ProcessingFailed { kind },
    }
}

pub(crate) fn local_follow_outcome(
    mut observation: crate::RouterLocalObservation,
    retry_attempts: u32,
    now: SyncTime,
) -> LocalDelivery<RouterDeliveryTypes> {
    let outcome = match &observation.receipt.outcome {
        DeliveryOutcome::Started => LocalOutcome::Delivered {
            acceptance: CarrierAcceptance::Started,
        },
        DeliveryOutcome::Steered => LocalOutcome::Delivered {
            acceptance: CarrierAcceptance::Steered,
        },
        DeliveryOutcome::StartedOrSteered => LocalOutcome::Delivered {
            acceptance: CarrierAcceptance::StartedOrSteered,
        },
        DeliveryOutcome::Queued => LocalOutcome::Delivered {
            acceptance: CarrierAcceptance::Queued,
        },
        DeliveryOutcome::PeerMessageWritten => LocalOutcome::Delivered {
            acceptance: CarrierAcceptance::PeerMessageWritten,
        },
        DeliveryOutcome::Unknown => LocalOutcome::Uncertain,
        DeliveryOutcome::NotSubmitted {
            retryable: true, ..
        } => LocalOutcome::Held { until: None },
        DeliveryOutcome::NotSubmitted { .. } | DeliveryOutcome::Rejected(_) => LocalOutcome::Held {
            until: Some(
                now.after(crate::subscription_delivery::delivery_retry_delay(
                    retry_attempts,
                )),
            ),
        },
    };
    let scheduled_retry_at = match &outcome {
        LocalOutcome::Held { until } => *until,
        _ => None,
    };
    observation.scheduled_retry_at = scheduled_retry_at;
    LocalDelivery {
        outcome,
        observation,
    }
}

#[cfg(test)]
#[path = "mail_push_outcome_tests.rs"]
mod tests;
