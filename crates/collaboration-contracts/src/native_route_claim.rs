//! Internal cause metadata accompanies the unchanged serialized route claim.
use crate::{RouteClaim, RouteUnavailableReason, UnsentCause};
use collaboration_protocol::DeliveryRejection;

#[derive(Clone, Debug)]
pub struct NativeRouteClaim {
    claim: RouteClaim,
    cause: Option<UnsentCause>,
}

impl NativeRouteClaim {
    pub fn not_mine() -> Self {
        Self {
            claim: RouteClaim::NotMine,
            cause: None,
        }
    }
    pub fn holds() -> Self {
        Self {
            claim: RouteClaim::Holds,
            cause: None,
        }
    }
    pub fn can_load() -> Self {
        Self {
            claim: RouteClaim::CanLoad,
            cause: None,
        }
    }
    pub fn rejected(rejection: DeliveryRejection, cause: UnsentCause) -> Self {
        Self {
            claim: RouteClaim::Rejected { rejection },
            cause: Some(cause),
        }
    }
    pub fn live_elsewhere(writable: bool, detail: Option<String>, cause: UnsentCause) -> Self {
        Self {
            claim: RouteClaim::LiveElsewhere { writable, detail },
            cause: Some(cause),
        }
    }
    pub fn unavailable(
        reason: RouteUnavailableReason,
        retryable: bool,
        cause: UnsentCause,
    ) -> Self {
        Self {
            claim: RouteClaim::Unavailable { reason, retryable },
            cause: Some(cause),
        }
    }
    pub fn claim(&self) -> &RouteClaim {
        &self.claim
    }
    pub fn unsent_cause(&self) -> Option<UnsentCause> {
        self.cause
    }
    pub fn into_claim(self) -> RouteClaim {
        self.claim
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NativeRefusalCode;
    use collaboration_protocol::DeliveryRejectionReason;

    #[test]
    fn different_internal_causes_leave_the_complete_serialized_claim_unchanged() {
        let original = RouteClaim::LiveElsewhere {
            writable: false,
            detail: Some("retained detail".to_owned()),
        };
        for cause in [
            UnsentCause::ProcessingFailure,
            UnsentCause::Refused(NativeRefusalCode::Rejection(
                DeliveryRejectionReason::LiveElsewhere,
            )),
        ] {
            let claim =
                NativeRouteClaim::live_elsewhere(false, Some("retained detail".to_owned()), cause);
            assert_eq!(claim.unsent_cause(), Some(cause));
            assert_eq!(
                serde_json::to_value(claim.into_claim()).unwrap(),
                serde_json::to_value(&original).unwrap()
            );
        }
    }
}
