//! Internal native classification travels beside the unchanged public receipt.
use collaboration_protocol::{
    DeliveryClientReceipt, DeliveryOutcome, DeliveryReceipt, DeliveryRejection,
    DeliveryRejectionReason, SessionReachability,
};

/// Only these outcomes can be constructed without a known-unsent cause.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeObservedOutcome {
    Started,
    Steered,
    StartedOrSteered,
    Queued,
    PeerMessageWritten,
    Unknown,
}

/// Codes are chosen at the emitter, never inferred from human-readable reason text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeRefusalCode {
    Rejection(DeliveryRejectionReason),
    RuntimeMissing,
    NativeUnavailable,
    HeldSessionBusy,
    ThreadNotLoaded,
    NoActiveTurn,
    SteerFailed,
    ActivityUnavailable,
    ProviderNotSubmitted,
    PeerUnavailable,
    PeerAuthenticationUnavailable,
    PeerEncodingFailed,
    PeerFrameTooLarge,
    PeerConnectFailed,
    PeerWriteNotStarted,
    CarrierUnavailable,
    QueuedNotSubmitted,
}
impl NativeRefusalCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rejection(reason) => match reason {
                DeliveryRejectionReason::ChildThread => "childThread",
                DeliveryRejectionReason::Busy => "busy",
                DeliveryRejectionReason::HeldByAnotherClient => "heldByAnotherClient",
                DeliveryRejectionReason::SettingsUnresolved => "settingsUnresolved",
                DeliveryRejectionReason::NotResumable => "notResumable",
                DeliveryRejectionReason::PermissionDenied => "permissionDenied",
                DeliveryRejectionReason::UnsupportedCapability => "unsupportedCapability",
                DeliveryRejectionReason::LocalRejection => "localRejection",
                DeliveryRejectionReason::Unknown => "unknown",
                DeliveryRejectionReason::EndpointUnavailable => "endpointUnavailable",
                DeliveryRejectionReason::NoRoute => "noRoute",
                DeliveryRejectionReason::LiveElsewhere => "liveElsewhere",
                DeliveryRejectionReason::SteerUnsupported => "steerUnsupported",
                DeliveryRejectionReason::QueueUnsupported => "queueUnsupported",
                DeliveryRejectionReason::StaleGeneration => "staleGeneration",
                DeliveryRejectionReason::ProviderSessionNotFound => "providerSessionNotFound",
                DeliveryRejectionReason::ProviderRejected => "providerRejected",
            },
            Self::RuntimeMissing => "runtimeMissing",
            Self::NativeUnavailable => "nativeUnavailable",
            Self::HeldSessionBusy => "heldSessionBusy",
            Self::ThreadNotLoaded => "threadNotLoaded",
            Self::NoActiveTurn => "noActiveTurn",
            Self::SteerFailed => "steerFailed",
            Self::ActivityUnavailable => "activityUnavailable",
            Self::ProviderNotSubmitted => "providerNotSubmitted",
            Self::PeerUnavailable => "peerUnavailable",
            Self::PeerAuthenticationUnavailable => "peerAuthenticationUnavailable",
            Self::PeerEncodingFailed => "peerEncodingFailed",
            Self::PeerFrameTooLarge => "peerFrameTooLarge",
            Self::PeerConnectFailed => "peerConnectFailed",
            Self::PeerWriteNotStarted => "peerWriteNotStarted",
            Self::CarrierUnavailable => "carrierUnavailable",
            Self::QueuedNotSubmitted => "queuedNotSubmitted",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnsentCause {
    RecipientUnknown,
    Refused(NativeRefusalCode),
    Holdable { otherwise: NativeRefusalCode },
    InvalidInput,
    ProcessingFailure,
}
impl UnsentCause {
    pub const fn for_rejection(reason: DeliveryRejectionReason) -> Self {
        match reason {
            DeliveryRejectionReason::NoRoute | DeliveryRejectionReason::ProviderSessionNotFound => {
                Self::RecipientUnknown
            }
            reason => Self::Refused(NativeRefusalCode::Rejection(reason)),
        }
    }
}

#[derive(Clone, Debug)]
enum NativeDisposition {
    Observed,
    Unsent(UnsentCause),
}

/// Private fields and distinct constructors require classification at every unsent emitter.
/// This internal port has no serializer and cannot change the released RPC receipt shape.
#[derive(Clone, Debug)]
pub struct NativeDeliveryResult {
    receipt: DeliveryReceipt,
    disposition: NativeDisposition,
    route_evidence: Option<collaboration_protocol::ProviderAcpRouteEvidence>,
}
impl NativeDeliveryResult {
    pub fn unknown() -> Self {
        Self::observed(NativeObservedOutcome::Unknown, None, None)
    }
    pub fn reconciled_acceptance(receipt: DeliveryReceipt) -> Option<Self> {
        let outcome = match receipt.outcome {
            DeliveryOutcome::Started => NativeObservedOutcome::Started,
            DeliveryOutcome::Steered => NativeObservedOutcome::Steered,
            DeliveryOutcome::StartedOrSteered => NativeObservedOutcome::StartedOrSteered,
            DeliveryOutcome::Queued => NativeObservedOutcome::Queued,
            DeliveryOutcome::PeerMessageWritten => NativeObservedOutcome::PeerMessageWritten,
            _ => return None,
        };
        Some(Self::observed(
            outcome,
            receipt.reachability,
            receipt.client,
        ))
    }
    pub fn observed(
        outcome: NativeObservedOutcome,
        reachability: Option<SessionReachability>,
        client: Option<DeliveryClientReceipt>,
    ) -> Self {
        let outcome = match outcome {
            NativeObservedOutcome::Started => DeliveryOutcome::Started,
            NativeObservedOutcome::Steered => DeliveryOutcome::Steered,
            NativeObservedOutcome::StartedOrSteered => DeliveryOutcome::StartedOrSteered,
            NativeObservedOutcome::Queued => DeliveryOutcome::Queued,
            NativeObservedOutcome::PeerMessageWritten => DeliveryOutcome::PeerMessageWritten,
            NativeObservedOutcome::Unknown => DeliveryOutcome::Unknown,
        };
        Self {
            receipt: DeliveryReceipt {
                outcome,
                reachability,
                client,
            },
            disposition: NativeDisposition::Observed,
            route_evidence: None,
        }
    }
    pub fn not_submitted(reason: impl Into<String>, retryable: bool, cause: UnsentCause) -> Self {
        Self {
            receipt: DeliveryReceipt {
                outcome: DeliveryOutcome::NotSubmitted {
                    reason: reason.into(),
                    retryable,
                },
                reachability: None,
                client: None,
            },
            disposition: NativeDisposition::Unsent(cause),
            route_evidence: None,
        }
    }
    pub fn rejected(rejection: DeliveryRejection, cause: UnsentCause) -> Self {
        Self {
            receipt: DeliveryReceipt {
                outcome: DeliveryOutcome::Rejected(rejection),
                reachability: None,
                client: None,
            },
            disposition: NativeDisposition::Unsent(cause),
            route_evidence: None,
        }
    }
    pub fn with_reachability(mut self, reachability: SessionReachability) -> Self {
        self.receipt.reachability = Some(reachability);
        self
    }
    pub fn with_client_receipt(mut self, client: Option<DeliveryClientReceipt>) -> Self {
        self.receipt.client = client;
        self
    }
    pub fn receipt(&self) -> &DeliveryReceipt {
        &self.receipt
    }
    pub fn unsent_cause(&self) -> Option<UnsentCause> {
        match self.disposition {
            NativeDisposition::Observed => None,
            NativeDisposition::Unsent(cause) => Some(cause),
        }
    }
    pub fn into_receipt(self) -> DeliveryReceipt {
        self.receipt
    }
    pub fn with_provider_acp_evidence(
        mut self,
        evidence: collaboration_protocol::ProviderAcpRouteEvidence,
    ) -> Self {
        self.route_evidence = Some(evidence);
        self
    }
    pub fn into_local_observation(self) -> crate::RouterLocalObservation {
        crate::RouterLocalObservation {
            receipt: self.receipt,
            scheduled_retry_at: None,
            route_evidence: self.route_evidence,
        }
    }
}

/// Existing external-client stand-ins supply scripted receipts, not native
/// emitter evidence. Production constructors always require the typed cause.
#[cfg(any(test, feature = "test-support"))]
pub fn scripted_delivery_result(receipt: DeliveryReceipt) -> NativeDeliveryResult {
    let disposition = match &receipt.outcome {
        DeliveryOutcome::Rejected(rejection) => {
            NativeDisposition::Unsent(UnsentCause::for_rejection(rejection.reason))
        }
        DeliveryOutcome::NotSubmitted { retryable, .. } => {
            NativeDisposition::Unsent(if *retryable {
                UnsentCause::Holdable {
                    otherwise: NativeRefusalCode::ProviderNotSubmitted,
                }
            } else {
                UnsentCause::Refused(NativeRefusalCode::ProviderNotSubmitted)
            })
        }
        _ => NativeDisposition::Observed,
    };
    NativeDeliveryResult {
        receipt,
        disposition,
        route_evidence: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cause_is_required_and_receipt_serialization_is_unchanged_when_reason_text_changes() {
        let cause = UnsentCause::Refused(NativeRefusalCode::RuntimeMissing);
        for reason in [
            "provider runtime is unavailable",
            "renamed human explanation",
        ] {
            let delivery = NativeDeliveryResult::not_submitted(reason, false, cause)
                .with_reachability(SessionReachability::ProviderAcp);
            assert_eq!(delivery.unsent_cause(), Some(cause));
            let expected = DeliveryReceipt {
                outcome: DeliveryOutcome::NotSubmitted {
                    reason: reason.to_owned(),
                    retryable: false,
                },
                reachability: Some(SessionReachability::ProviderAcp),
                client: None,
            };
            assert_eq!(
                serde_json::to_value(delivery.into_receipt()).unwrap(),
                serde_json::to_value(expected).unwrap()
            );
        }
    }
}
