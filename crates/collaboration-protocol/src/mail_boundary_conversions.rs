//! Equivalent mail and RPC vocabulary is converted once at the facade boundary.
use crate::{CodexGeneration, GenerationNumber, MessageDelivery, SessionRef, UuidIdentity};
use collaboration_mail_protocol::{GenerationGuard, MailDeliveryMode};

#[derive(Debug, thiserror::Error)]
#[error("invalid mail boundary field: {0}")]
pub struct MailBoundaryConversionError(&'static str);

impl TryFrom<SessionRef> for message_board::SessionRef {
    type Error = MailBoundaryConversionError;
    fn try_from(session: SessionRef) -> Result<Self, Self::Error> {
        let encoded =
            serde_json::to_value(session).map_err(|_| MailBoundaryConversionError("session"))?;
        serde_json::from_value(encoded).map_err(|_| MailBoundaryConversionError("session"))
    }
}
impl TryFrom<message_board::SessionRef> for SessionRef {
    type Error = MailBoundaryConversionError;
    fn try_from(session: message_board::SessionRef) -> Result<Self, Self::Error> {
        let encoded =
            serde_json::to_value(session).map_err(|_| MailBoundaryConversionError("session"))?;
        serde_json::from_value(encoded).map_err(|_| MailBoundaryConversionError("session"))
    }
}

impl From<MessageDelivery> for MailDeliveryMode {
    fn from(delivery: MessageDelivery) -> Self {
        match delivery {
            MessageDelivery::Auto => Self::Auto,
            MessageDelivery::Queue => Self::Queue,
            MessageDelivery::Steer => Self::Steer,
        }
    }
}
impl From<MailDeliveryMode> for MessageDelivery {
    fn from(delivery: MailDeliveryMode) -> Self {
        match delivery {
            MailDeliveryMode::Auto => Self::Auto,
            MailDeliveryMode::Queue => Self::Queue,
            MailDeliveryMode::Steer => Self::Steer,
        }
    }
}
impl TryFrom<CodexGeneration> for GenerationGuard {
    type Error = MailBoundaryConversionError;
    fn try_from(generation: CodexGeneration) -> Result<Self, Self::Error> {
        let service_epoch = String::from(generation.service_epoch)
            .try_into()
            .map_err(|_| MailBoundaryConversionError("serviceEpoch"))?;
        Self::new(service_epoch, generation.generation.into())
            .map_err(|_| MailBoundaryConversionError("generation"))
    }
}
impl TryFrom<GenerationGuard> for CodexGeneration {
    type Error = MailBoundaryConversionError;
    fn try_from(guard: GenerationGuard) -> Result<Self, Self::Error> {
        Ok(Self {
            service_epoch: UuidIdentity::try_from(guard.service_epoch().as_str().to_owned())
                .map_err(|_| MailBoundaryConversionError("serviceEpoch"))?,
            generation: GenerationNumber::try_from(guard.generation())
                .map_err(|_| MailBoundaryConversionError("generation"))?,
        })
    }
}

impl TryFrom<crate::MailItemId> for crate::PushId {
    type Error = MailBoundaryConversionError;
    fn try_from(id: crate::MailItemId) -> Result<Self, Self::Error> {
        Self::try_from(id.as_str().to_owned()).map_err(|_| MailBoundaryConversionError("itemId"))
    }
}
impl TryFrom<crate::PushId> for crate::MailItemId {
    type Error = MailBoundaryConversionError;
    fn try_from(id: crate::PushId) -> Result<Self, Self::Error> {
        Self::try_from(id.as_str().to_owned()).map_err(|_| MailBoundaryConversionError("pushId"))
    }
}
