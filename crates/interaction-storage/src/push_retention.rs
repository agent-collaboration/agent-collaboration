//! Carrier history policy is independent of the lifetime of its custody row.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PushRetention {
    CreationAge,
    MailTerminalAge,
}

impl PushRetention {
    pub(crate) fn validate_clock(
        self,
        state: collaboration_protocol::PushDeliveryState,
        settled_at: Option<&str>,
    ) -> Result<(), crate::StorageError> {
        use collaboration_protocol::PushDeliveryState;
        let terminal = matches!(
            state,
            PushDeliveryState::Delivered
                | PushDeliveryState::OutcomeUnknown
                | PushDeliveryState::Rejected
        );
        if self == Self::MailTerminalAge && terminal != settled_at.is_some() {
            return Err(crate::StorageError::InvalidRecord);
        }
        Ok(())
    }
}
