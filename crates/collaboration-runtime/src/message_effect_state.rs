//! Request-local effect evidence; never a replay or delivery store.
use collaboration_contracts::NativeRefusalCode;
use collaboration_contracts::UnsentCause;
use collaboration_protocol::{
    CodexGeneration, DeliveryNextAction, DeliveryRejectionReason, NativeDeliveryFailureCause,
    PermissionDifference, SessionRef,
};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug)]
pub enum NativeFailureKind {
    WrongService,
    EndpointNotFound,
    UnsupportedCapability,
    Unavailable,
    StaleGeneration,
    OutcomeUnknown,
    ThreadMissingOrUnmaterialized,
    LocalRejection(UnsentCause),
    NotLoaded,
    ThreadNotLoaded,
    NoActiveTurn,
}
impl NativeFailureKind {
    fn wire_kind(self) -> &'static str {
        match self {
            Self::WrongService => "wrongService",
            Self::EndpointNotFound => "endpointNotFound",
            Self::UnsupportedCapability => "unsupportedCapability",
            Self::Unavailable => "unavailable",
            Self::StaleGeneration => "staleGeneration",
            Self::OutcomeUnknown => "outcomeUnknown",
            Self::ThreadMissingOrUnmaterialized => "threadMissingOrUnmaterialized",
            Self::LocalRejection(_) => "localRejection",
            Self::NotLoaded => collaboration_contracts::NOT_LOADED_REASON,
            Self::ThreadNotLoaded => "threadNotLoaded",
            Self::NoActiveTurn => "noActiveTurn",
        }
    }
    fn disposition(self) -> NativeFailureDisposition {
        use DeliveryRejectionReason as Reason;
        use NativeRefusalCode as Code;
        NativeFailureDisposition::Unsent(match self {
            Self::WrongService => UnsentCause::InvalidInput,
            Self::EndpointNotFound | Self::ThreadMissingOrUnmaterialized => {
                UnsentCause::RecipientUnknown
            }
            Self::UnsupportedCapability => {
                UnsentCause::Refused(Code::Rejection(Reason::UnsupportedCapability))
            }
            Self::Unavailable => UnsentCause::Holdable {
                otherwise: Code::NativeUnavailable,
            },
            Self::StaleGeneration => UnsentCause::Refused(Code::Rejection(Reason::StaleGeneration)),
            Self::LocalRejection(cause) => cause,
            Self::NotLoaded => UnsentCause::Holdable {
                otherwise: Code::ThreadNotLoaded,
            },
            Self::ThreadNotLoaded => UnsentCause::Refused(Code::ThreadNotLoaded),
            Self::NoActiveTurn => UnsentCause::Refused(Code::NoActiveTurn),
            Self::OutcomeUnknown => return NativeFailureDisposition::Uncertain,
        })
    }
}
#[derive(Clone, Copy, Debug)]
pub enum NativeFailureDisposition {
    Unsent(UnsentCause),
    Uncertain,
}
#[derive(Debug)]
pub struct NativeMessageFailure {
    pub response: Value,
    pub disposition: NativeFailureDisposition,
}

pub(crate) const HELD_BY_ANOTHER_CLIENT_GUIDANCE: &str =
    "Message it from the Codex client that holds it.";

pub struct NativeMessageAttemptEvidence<'a> {
    pub effects: &'a mut agent_automation::NativeEffectEvidence<SessionRef, CodexGeneration>,
    pub sink: &'a dyn collaboration_contracts::AttemptEvidenceSink,
}
impl NativeMessageAttemptEvidence<'_> {
    pub async fn observe_permission_differences(
        &mut self,
        differences: &[PermissionDifference],
    ) -> Result<(), collaboration_contracts::DeliveryContractError> {
        self.effects.permission_differences = differences.to_vec();
        self.effects.resume = agent_automation::PreparationEffect::Accepted;
        self.sink.record(self.effects.clone().into()).await
    }
}

pub struct MessageEffects {
    pub id: Value,
    pub resume: &'static str,
    pub submission: &'static str,
    pub correlation: Option<String>,
}
impl MessageEffects {
    pub fn new(id: Value) -> Self {
        Self {
            id,
            resume: "notRequested",
            submission: "notDispatched",
            correlation: None,
        }
    }
    pub fn failure(&self, kind: NativeFailureKind, stage: &str) -> NativeMessageFailure {
        let disposition = kind.disposition();
        let kind = kind.wire_kind();
        let mut data = json!({"kind":kind,"stage":stage,"message":"Message operation failed",
            "effects":{"resume":self.resume,"submission":self.submission}});
        if let (Some(fields), Some(id)) = (data.as_object_mut(), &self.correlation) {
            fields.insert("clientUserMessageId".to_owned(), json!(id));
        }
        self.classified_failure(json!({"jsonrpc":"2.0","id":self.id,"error":{"code":-32050,"message":"Message operation failed","data":data}}), disposition)
    }
    pub fn recorded_policy_failure(
        &self,
        error: &codex_acp_adapter::SessionSetupError,
        stage: &str,
        failure_cause: NativeDeliveryFailureCause,
    ) -> NativeMessageFailure {
        use codex_acp_adapter::SessionSetupError as Error;
        let cause = match error {
            Error::InvalidParameters => UnsentCause::InvalidInput,
            Error::ConfigurationMismatch
            | Error::ModelMismatch { .. }
            | Error::AccessMismatch { .. } => UnsentCause::Refused(NativeRefusalCode::Rejection(
                DeliveryRejectionReason::SettingsUnresolved,
            )),
            Error::Busy => {
                UnsentCause::Refused(NativeRefusalCode::Rejection(DeliveryRejectionReason::Busy))
            }
            Error::SchemaUnavailable => UnsentCause::Refused(NativeRefusalCode::Rejection(
                DeliveryRejectionReason::UnsupportedCapability,
            )),
            Error::LaunchAdmissionFailed(_) | Error::RegistrationRefused => UnsentCause::Refused(
                NativeRefusalCode::Rejection(DeliveryRejectionReason::PermissionDenied),
            ),
            Error::NativeRejected => UnsentCause::Refused(NativeRefusalCode::Rejection(
                DeliveryRejectionReason::Unknown,
            )),
            Error::Unavailable | Error::CancellationUnresolved => {
                UnsentCause::Refused(NativeRefusalCode::NativeUnavailable)
            }
            Error::HostLocationsUnavailable { .. }
            | Error::NativeThreadStatusUnavailable
            | Error::OutcomeUnknown => UnsentCause::ProcessingFailure,
        };
        self.local_rejection("thread/resume", stage, failure_cause, cause)
    }
    pub fn local_rejection(
        &self,
        operation: &str,
        stage: &str,
        cause: NativeDeliveryFailureCause,
        unsent: UnsentCause,
    ) -> NativeMessageFailure {
        let mut response = self.failure(NativeFailureKind::LocalRejection(unsent), stage);
        if let Some(data) = response
            .response
            .pointer_mut("/error/data")
            .and_then(Value::as_object_mut)
        {
            data.insert("operation".to_owned(), json!(operation));
            data.insert("cause".to_owned(), json!(cause));
        }
        response
    }
    pub fn unsupported_method(&self, operation: &str, stage: &str) -> NativeMessageFailure {
        let mut response = self.failure(NativeFailureKind::UnsupportedCapability, stage);
        if let Some(data) = response
            .response
            .pointer_mut("/error/data")
            .and_then(Value::as_object_mut)
        {
            data.insert("operation".to_owned(), json!(operation));
            data.insert(
                "cause".to_owned(),
                json!(NativeDeliveryFailureCause::NativeMethodUnavailable),
            );
        }
        response
    }
    pub fn native_rejection(
        &self,
        operation: &str,
        stage: &str,
        code: i64,
        native: Option<&Value>,
    ) -> NativeMessageFailure {
        let (reason, next_action) = classify_native_rejection(code, native);
        let message = if reason == DeliveryRejectionReason::HeldByAnotherClient {
            HELD_BY_ANOTHER_CLIENT_GUIDANCE
        } else {
            "Message operation failed"
        };
        let mut data = json!({"kind":"nativeRejected","operation":operation,"stage":stage,"cause":"nativeRejected","message":message,
            "reason":reason,"nextAction":next_action,"clientCode":code,
            "effects":{"resume":self.resume,"submission":self.submission}});
        if reason == DeliveryRejectionReason::Unknown
            && let Some(fields) = data.as_object_mut()
        {
            fields.insert("nativeCode".into(), json!(code));
        }
        if let (Some(fields), Some(id)) = (data.as_object_mut(), &self.correlation) {
            fields.insert("clientUserMessageId".to_owned(), json!(id));
        }
        self.classified_failure(json!({"jsonrpc":"2.0","id":self.id,"error":{"code":-32050,"message":"Message operation failed","data":data}}),NativeFailureDisposition::Unsent(UnsentCause::for_rejection(reason)))
    }
    fn classified_failure(
        &self,
        response: Value,
        disposition: NativeFailureDisposition,
    ) -> NativeMessageFailure {
        NativeMessageFailure {
            response,
            disposition: if self.resume == "unknown" || self.submission == "unknown" {
                NativeFailureDisposition::Uncertain
            } else {
                disposition
            },
        }
    }
}

/// The closed rejection reason set and its corrective action, shared by every
/// native path so one refusal reads the same whichever call produced it.
pub fn classify_native_rejection(
    code: i64,
    native: Option<&Value>,
) -> (DeliveryRejectionReason, DeliveryNextAction) {
    let native_message = native
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str);
    if code == -32600
        && native_message.is_some_and(|message| {
            message
                .strip_prefix("thread ")
                .and_then(|thread| thread.strip_suffix(" already has an active writer"))
                .is_some_and(|thread| {
                    !thread.is_empty() && !thread.chars().any(char::is_whitespace)
                })
        })
    {
        return (
            DeliveryRejectionReason::HeldByAnotherClient,
            DeliveryNextAction::MessageFromHoldingCodexClient,
        );
    }
    let description = native
        .map(Value::to_string)
        .unwrap_or_default()
        .to_lowercase();
    if description.contains("subagent") || description.contains("child") {
        (
            DeliveryRejectionReason::ChildThread,
            DeliveryNextAction::InspectTarget,
        )
    } else if description.contains("busy") || description.contains("active turn") {
        (
            DeliveryRejectionReason::Busy,
            DeliveryNextAction::UseDeliverySteer,
        )
    } else if description.contains("resume") {
        (
            DeliveryRejectionReason::NotResumable,
            DeliveryNextAction::InspectTarget,
        )
    } else if description.contains("permission") || code == -32001 {
        (
            DeliveryRejectionReason::PermissionDenied,
            DeliveryNextAction::RequestApproval,
        )
    } else if code == -32601 || description.contains("unsupported") {
        (
            DeliveryRejectionReason::UnsupportedCapability,
            DeliveryNextAction::CorrectRequest,
        )
    } else {
        (
            DeliveryRejectionReason::Unknown,
            DeliveryNextAction::RetryLater,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{MessageEffects, classify_native_rejection};
    use serde_json::json;

    #[test]
    fn native_rejections_have_closed_reasons_and_corrective_actions() {
        let effects = MessageEffects::new(json!(1));
        for (code, native, reason, action) in [
            (
                -32600,
                json!({"message":"thread 00000000-0000-4000-8000-000000000001 already has an active writer"}),
                "heldByAnotherClient",
                "messageFromHoldingCodexClient",
            ),
            (
                -32000,
                json!({"message":"target is a spawned child subagent"}),
                "childThread",
                "inspectTarget",
            ),
            (
                -32000,
                json!({"message":"thread has an active turn and is busy"}),
                "busy",
                "useDeliverySteer",
            ),
            (
                -32000,
                json!({"message":"thread cannot resume"}),
                "notResumable",
                "inspectTarget",
            ),
            (
                -32001,
                json!({"message":"denied"}),
                "permissionDenied",
                "requestApproval",
            ),
            (
                -32601,
                json!({"message":"missing method"}),
                "unsupportedCapability",
                "correctRequest",
            ),
            (
                -32099,
                json!({"message":"provider refused"}),
                "unknown",
                "retryLater",
            ),
        ] {
            let rejection = effects.native_rejection("turn/start", "start", code, Some(&native));
            assert_eq!(
                rejection.response.pointer("/error/data/reason"),
                Some(&json!(reason))
            );
            assert_eq!(
                rejection.response.pointer("/error/data/nextAction"),
                Some(&json!(action))
            );
            if reason == "unknown" {
                assert_eq!(
                    rejection.response.pointer("/error/data/nativeCode"),
                    Some(&json!(code))
                );
            } else {
                assert!(
                    rejection
                        .response
                        .pointer("/error/data/nativeCode")
                        .is_none()
                );
            }
        }
    }

    #[test]
    fn only_the_exact_invalid_request_writer_lock_error_names_another_client() {
        for (code, message, expected) in [
            (
                -32601,
                "thread target already has an active writer",
                ("unsupportedCapability", "correctRequest"),
            ),
            (
                -32600,
                "thread target already has an active writer; retry",
                ("unknown", "retryLater"),
            ),
            (
                -32600,
                "thread  already has an active writer",
                ("unknown", "retryLater"),
            ),
            (-32600, "thread target not found", ("unknown", "retryLater")),
        ] {
            assert_eq!(
                serde_json::to_value(classify_native_rejection(
                    code,
                    Some(&json!({"message":message}))
                ))
                .unwrap(),
                serde_json::to_value(expected).unwrap(),
                "unrelated native error: {message}"
            );
        }
    }
}
