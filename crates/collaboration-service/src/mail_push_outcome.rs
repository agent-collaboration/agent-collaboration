//! Native receipts remain intact while the library owns canonical custody outcomes.
use collaboration_contracts::NativeDeliveryResult;
use collaboration_contracts::NativeRefusalCode;
use collaboration_contracts::RouterDeliveryTypes;
use collaboration_contracts::UnsentCause;
use collaboration_mail_protocol::CarrierAcceptance;
use collaboration_protocol::DeliveryOutcome;
use fleet_sync_delivery::{DeliveryError, LocalDelivery, LocalOutcome};
use fleet_sync_protocol::{DeliveryFailureKind, DeliveryRefusal, RecipientRefusalCode};

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

#[cfg(test)]
#[path = "mail_push_outcome_tests.rs"]
mod tests;
