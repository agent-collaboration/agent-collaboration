//! Listing pages preserve recipient inspection failures and the shared named limit.
use super::{
    MessageFailure, MessageFailureKind, MessageFailureStage, MessageOperations, StorageError,
    caller_is_local,
};
use crate::collaboration_rejection::{
    CollaborationRejection, CollaborationRejectionReason, PublishedRejection,
};

#[derive(Debug)]
pub enum NotificationMembersFailure {
    Limit(collaboration_protocol::limits::LimitExceeded),
    Inspection(MessageFailure),
}
impl From<MessageFailure> for NotificationMembersFailure {
    fn from(failure: MessageFailure) -> Self {
        Self::Inspection(failure)
    }
}
impl CollaborationRejection for NotificationMembersFailure {
    fn rejection_reason(&self) -> Option<CollaborationRejectionReason> {
        match self {
            Self::Limit(_) => None,
            Self::Inspection(failure) => failure.rejection_reason(),
        }
    }
    fn published_rejection(&self) -> PublishedRejection {
        match self {
            Self::Inspection(failure) => failure.published_rejection(),
            Self::Limit(detail) => {
                #[derive(serde::Serialize)]
                struct LimitFailure<'a> {
                    kind: &'static str,
                    #[serde(flatten)]
                    detail: &'a collaboration_protocol::limits::LimitExceeded,
                }
                PublishedRejection::typed(
                    PublishedRejection::INVALID_PARAMS,
                    detail.to_string(),
                    &LimitFailure {
                        kind: "limitExceeded",
                        detail,
                    },
                )
            }
        }
    }
}

impl MessageOperations {
    pub async fn notification_members(
        &self,
        request: collaboration_protocol::NotificationMembersRequest,
    ) -> Result<collaboration_protocol::NotificationMembersPage, NotificationMembersFailure> {
        collaboration_protocol::limits::LimitName::NotificationListingMaxEntries
            .validate(u64::from(request.limit))
            .map_err(NotificationMembersFailure::Limit)?;
        if !caller_is_local(&request.caller, &self.identity) {
            return Err(MessageFailure::new(
                MessageFailureKind::WrongService,
                MessageFailureStage::Inspect,
                "Caller belongs to another Router",
            )
            .into());
        }
        let store = self.push_store(MessageFailureStage::Inspect)?;
        let (records, next) = store
            .notification_listing_members(
                &request.caller,
                &request.listing_id,
                request.after.as_ref(),
                request.limit,
            )
            .await
            .map_err(|error| {
                let (kind, message) = match error {
                    StorageError::PushNotPermitted => (
                        MessageFailureKind::NotPermitted,
                        "Only the listing recipient may read its members",
                    ),
                    StorageError::PushNotFound => (
                        MessageFailureKind::NotFound,
                        "Notification listing not found",
                    ),
                    StorageError::InvalidRecord => (
                        MessageFailureKind::InvalidField,
                        "Listing id and page limit (1 through 50) must be valid",
                    ),
                    _ => (
                        MessageFailureKind::Unavailable,
                        "Notification listing members could not be read",
                    ),
                };
                MessageFailure::new(kind, MessageFailureStage::Inspect, message)
            })?;
        Ok(collaboration_protocol::NotificationMembersPage {
            listing_id: request.listing_id,
            records: self.notice_list(records)?.records,
            next,
        })
    }
}
