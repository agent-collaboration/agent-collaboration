//! Typed central failures are shared data, independent of the operation adapter.
use crate::collaboration_rejection::{CollaborationRejection, PublishedRejection};
use collaboration_protocol::{AutomationRefusal, OperationId};
use serde::Serialize;

#[derive(Debug, thiserror::Error, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CentralCatalogFailure {
    #[error("central catalog unavailable; no mutation was dispatched")]
    CentralUnreachable { operation_id: Option<OperationId> },
    #[error("central outcome unknown; inspect or replay the same operation ID")]
    OutcomeUnknown { operation_id: OperationId },
    #[error("central catalog refused the request: {error:?}")]
    Refused {
        operation_id: Option<OperationId>,
        error: AutomationRefusal,
    },
    #[error("central catalog read did not return a valid answer")]
    #[serde(skip)]
    InvalidReply,
}

impl CentralCatalogFailure {
    pub(crate) fn unanswered(operation_id: Option<OperationId>) -> Self {
        match operation_id {
            Some(operation_id) => Self::OutcomeUnknown { operation_id },
            None => Self::InvalidReply,
        }
    }
}

impl CollaborationRejection for CentralCatalogFailure {
    fn rejection_reason(
        &self,
    ) -> Option<crate::collaboration_rejection::CollaborationRejectionReason> {
        None
    }
    fn published_rejection(&self) -> PublishedRejection {
        match self {
            Self::InvalidReply => {
                PublishedRejection::bare(PublishedRejection::INTERNAL, self.to_string())
            }
            _ => {
                let mut published = PublishedRejection::typed(
                    PublishedRejection::OPERATION_FAILED,
                    self.to_string(),
                    self,
                );
                let Some(serde_json::Value::Object(fields)) = published.data.as_mut() else {
                    return PublishedRejection::bare(
                        PublishedRejection::INTERNAL,
                        self.to_string(),
                    );
                };
                fields.insert("message".into(), self.to_string().into());
                published
            }
        }
    }
}
