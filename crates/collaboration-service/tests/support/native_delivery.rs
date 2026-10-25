//! Scripted external clients preserve old receipts; this is not native-emitter proof.
use collaboration_protocol::{DeliveryOutcome, DeliveryReceipt};
use collaboration_service::{
    NativeDeliveryResult, NativeObservedOutcome, NativeRefusalCode, UnsentCause,
};

pub fn scripted(receipt: DeliveryReceipt) -> NativeDeliveryResult {
    let DeliveryReceipt {
        outcome,
        reachability,
        client,
    } = receipt;
    let result = match outcome {
        DeliveryOutcome::NotSubmitted { reason, retryable } => NativeDeliveryResult::not_submitted(
            reason,
            retryable,
            if retryable {
                UnsentCause::Holdable {
                    otherwise: NativeRefusalCode::ProviderNotSubmitted,
                }
            } else {
                UnsentCause::Refused(NativeRefusalCode::ProviderNotSubmitted)
            },
        )
        .with_client_receipt(client),
        DeliveryOutcome::Rejected(rejection) => {
            let cause = UnsentCause::for_rejection(rejection.reason);
            NativeDeliveryResult::rejected(rejection, cause).with_client_receipt(client)
        }
        DeliveryOutcome::Started => {
            NativeDeliveryResult::observed(NativeObservedOutcome::Started, None, client)
        }
        DeliveryOutcome::Steered => {
            NativeDeliveryResult::observed(NativeObservedOutcome::Steered, None, client)
        }
        DeliveryOutcome::StartedOrSteered => {
            NativeDeliveryResult::observed(NativeObservedOutcome::StartedOrSteered, None, client)
        }
        DeliveryOutcome::Queued => {
            NativeDeliveryResult::observed(NativeObservedOutcome::Queued, None, client)
        }
        DeliveryOutcome::PeerMessageWritten => {
            NativeDeliveryResult::observed(NativeObservedOutcome::PeerMessageWritten, None, client)
        }
        DeliveryOutcome::Unknown => {
            NativeDeliveryResult::observed(NativeObservedOutcome::Unknown, None, client)
        }
    };
    if let Some(reachability) = reachability {
        result.with_reachability(reachability)
    } else {
        result
    }
}
