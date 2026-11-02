//! Catalog calls preserve central refusals and never turn an unknown write into an unsent one.
use crate::CentralCatalogFailure;
use crate::collaboration_rejection::{CollaborationRejection, PublishedRejection};
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationInspectionFailure, AutomationPage, OperationId,
    PageLimit,
};
use collaboration_runtime::{CentralAutomationError, CentralAutomationTransport};
use serde::Serialize;
use std::sync::Arc;

/// Catalog failures retain central evidence; response-size failures retain inspection evidence.
#[derive(Debug, thiserror::Error)]
pub enum CatalogPageFailure {
    #[error(transparent)]
    Command(Box<crate::ScheduleCommandFailure>),
    #[error(transparent)]
    Catalog(Box<CentralCatalogFailure>),
    #[error("catalog record exceeds the response budget")]
    Inspection(Box<AutomationInspectionFailure>),
}
impl From<crate::ScheduleCommandFailure> for CatalogPageFailure {
    fn from(value: crate::ScheduleCommandFailure) -> Self {
        Self::Command(Box::new(value))
    }
}
impl From<CentralCatalogFailure> for CatalogPageFailure {
    fn from(error: CentralCatalogFailure) -> Self {
        Self::Catalog(Box::new(error))
    }
}
impl From<AutomationInspectionFailure> for CatalogPageFailure {
    fn from(error: AutomationInspectionFailure) -> Self {
        Self::Inspection(Box::new(error))
    }
}
impl CollaborationRejection for CatalogPageFailure {
    fn rejection_reason(
        &self,
    ) -> Option<crate::collaboration_rejection::CollaborationRejectionReason> {
        match self {
            Self::Command(error) => error.rejection_reason(),
            Self::Catalog(error) => error.rejection_reason(),
            Self::Inspection(error) => error.rejection_reason(),
        }
    }
    fn published_rejection(&self) -> PublishedRejection {
        match self {
            Self::Command(error) => error.published_rejection(),
            Self::Catalog(error) => error.published_rejection(),
            Self::Inspection(error) => error.published_rejection(),
        }
    }
}

pub(crate) struct CentralCatalog {
    transport: Option<Arc<dyn CentralAutomationTransport>>,
}
impl CentralCatalog {
    pub(crate) fn new(transport: Option<Arc<dyn CentralAutomationTransport>>) -> Self {
        Self { transport }
    }
    pub(crate) async fn request(
        &self,
        request: AutomationCall,
        operation_id: Option<OperationId>,
    ) -> Result<AutomationAnswer, CentralCatalogFailure> {
        let transport =
            self.transport
                .as_ref()
                .ok_or_else(|| CentralCatalogFailure::CentralUnreachable {
                    operation_id: operation_id.clone(),
                })?;
        match transport.call(request).await {
            Ok(AutomationAnswer::Refused(error)) => Err(CentralCatalogFailure::Refused {
                operation_id,
                error,
            }),
            Ok(answer) => Ok(answer),
            Err(CentralAutomationError::Unavailable) => {
                Err(CentralCatalogFailure::CentralUnreachable { operation_id })
            }
            Err(CentralAutomationError::OutcomeUnknown) if operation_id.is_none() => {
                Err(CentralCatalogFailure::CentralUnreachable { operation_id: None })
            }
            Err(_) => Err(CentralCatalogFailure::unanswered(operation_id)),
        }
    }

    /// Reduce only the requested row count; every returned cursor comes from central.
    pub(crate) async fn page<TRecord: Serialize>(
        &self,
        limit: PageLimit,
        budget: crate::response_budget::ResultByteBudget,
        request: impl Fn(PageLimit) -> AutomationCall,
        answer_page: impl Fn(AutomationAnswer) -> Option<AutomationPage<TRecord>>,
    ) -> Result<AutomationPage<TRecord>, CatalogPageFailure> {
        let mut limit = limit;
        loop {
            let answer = self.request(request(limit), None).await?;
            let page = answer_page(answer).ok_or(CentralCatalogFailure::InvalidReply)?;
            if budget.admits(&page) {
                return Ok(page);
            }
            let count = u32::from(limit);
            if count == 1 || page.records.len() <= 1 {
                return Err(CatalogPageFailure::Inspection(Box::new(
                    crate::automation_inspection_failure::invalid(
                        "limit",
                        "One catalog record exceeds this transport's response budget; no record was truncated.",
                    ),
                )));
            }
            // At most seven reads for the protocol's maximum of 100 rows. The original
            // cursor and filters stay fixed; mutations are never retried by this adapter.
            limit = (count / 2)
                .try_into()
                .map_err(|_| CentralCatalogFailure::InvalidReply)?;
        }
    }
}
