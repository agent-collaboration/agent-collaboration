//! A's stateless requester admission; the Fleet owns persistence and B owns creation.
use super::{ConversationFailure, ConversationOperations};
use collaboration_contracts::{RemoteCreateFleet, RemoteCreateFleetError};
use collaboration_mail_protocol::{RemoteCreateAdmission, RemoteCreateRequest, RemoteCreateStatus};
use collaboration_protocol::{
    ConversationOperationFailure, ConversationOperationFailureKind as Kind,
    ConversationOperationFailureStage, NonEmptyText, ProviderOperationEffect,
};
use message_board::RequestId;
use std::sync::Arc;

impl ConversationOperations {
    #[must_use]
    pub fn with_remote_create_fleet(mut self, fleet: Option<Arc<dyn RemoteCreateFleet>>) -> Self {
        self.remote_fleet = fleet;
        self
    }

    #[expect(
        clippy::result_large_err,
        reason = "Preserve the existing typed conversation failure contract."
    )]
    pub async fn remote_create(
        &self,
        request: RemoteCreateRequest,
    ) -> Result<RemoteCreateAdmission, ConversationFailure> {
        if request.requester.endpoint.service_id.as_str()
            != String::from(self.identity.service_id().clone())
        {
            return Err(Self::remote_create_failure(
                &request.request_id,
                Kind::InvalidRequest,
                ProviderOperationEffect::None,
                "remote-create requester must be a Session on this Router",
            ));
        }
        let fleet = self.remote_fleet.as_ref().ok_or_else(|| {
            Self::remote_create_failure(
                &request.request_id,
                Kind::Unavailable,
                ProviderOperationEffect::None,
                "Fleet machine session is unavailable",
            )
        })?;
        let mut effect = ProviderOperationEffect::None;
        // C1 keys admission by this frozen request id and content. A bounded
        // retry is call-local; it creates no directory/outbox/request record.
        for attempt in 0..2 {
            match fleet.request_remote_create(request.clone()).await {
                Ok(answer) => return Ok(answer),
                Err(RemoteCreateFleetError::AnswerLost) => {
                    effect = ProviderOperationEffect::Unknown
                }
                Err(RemoteCreateFleetError::NotSent | RemoteCreateFleetError::Retryable) => {}
                Err(RemoteCreateFleetError::InvalidRequest) => {
                    return Err(Self::remote_create_failure(
                        &request.request_id,
                        Kind::InvalidRequest,
                        effect,
                        "remote-create request could not be encoded",
                    ));
                }
                Err(RemoteCreateFleetError::InvalidReply) => {
                    return Err(Self::remote_create_failure(
                        &request.request_id,
                        Kind::ProtocolViolation,
                        ProviderOperationEffect::Unknown,
                        "Fleet returned an invalid remote-create reply",
                    ));
                }
            }
            if attempt == 1 {
                break;
            }
        }
        let kind = if effect == ProviderOperationEffect::Unknown {
            Kind::OutcomeUnknown
        } else {
            Kind::Unavailable
        };
        Err(Self::remote_create_failure(
            &request.request_id,
            kind,
            effect,
            "Fleet admission answer unavailable; inspect remote status with the request id",
        ))
    }

    #[expect(
        clippy::result_large_err,
        reason = "Preserve the existing typed conversation failure contract."
    )]
    pub async fn remote_status(
        &self,
        request: RequestId,
    ) -> Result<RemoteCreateStatus, ConversationFailure> {
        let fleet = self.remote_fleet.as_ref().ok_or_else(|| {
            Self::remote_create_failure(
                &request,
                Kind::Unavailable,
                ProviderOperationEffect::None,
                "Fleet machine session is unavailable",
            )
        })?;
        for _ in 0..2 {
            match fleet.remote_create_status(request.clone()).await {
                Ok(answer) => return Ok(answer),
                Err(
                    RemoteCreateFleetError::AnswerLost
                    | RemoteCreateFleetError::NotSent
                    | RemoteCreateFleetError::Retryable,
                ) => {}
                Err(_) => {
                    return Err(Self::remote_create_failure(
                        &request,
                        Kind::ProtocolViolation,
                        ProviderOperationEffect::None,
                        "Fleet returned an invalid remote-create status",
                    ));
                }
            }
        }
        Err(Self::remote_create_failure(
            &request,
            Kind::Unavailable,
            ProviderOperationEffect::None,
            "Fleet remote-create status is unavailable",
        ))
    }

    fn remote_create_failure(
        id: &RequestId,
        kind: Kind,
        effect: ProviderOperationEffect,
        message: &'static str,
    ) -> ConversationFailure {
        let (Ok(operation_id), Ok(message)) = (
            id.as_str().to_owned().try_into(),
            NonEmptyText::try_from(message.to_owned()),
        ) else {
            return ConversationFailure::UndescribableFailure;
        };
        ConversationFailure::Operation(ConversationOperationFailure {
            kind,
            stage: ConversationOperationFailureStage::Admission,
            effect,
            message,
            operation_id: Some(operation_id),
            invalid_setting: None,
            provider_code: None,
            target: None,
            endpoint: None,
            availability: None,
        })
    }
}
