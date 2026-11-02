//! Organization catalog pages retain central's rows and opaque continuation tokens.
use super::automation_operations::AutomationOperations;
use crate::{central_catalog::CatalogPageFailure, response_budget::ResultByteBudget};
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationPage, AutomationPageRequest, InstructionSnapshot,
    RevisionListRequest, RevisionRecord,
};

impl AutomationOperations {
    pub(super) async fn require_central_collection_parent(
        &self,
        collection: &automation_storage::AutomationCollection,
    ) -> Result<(), crate::CentralCatalogFailure> {
        use crate::CentralCatalogFailure;
        use automation_storage::AutomationCollection;
        use collaboration_protocol::{AutomationKind, ScheduleShowRequest, WakeShowRequest};
        let (request, kind, id) = match collection {
            AutomationCollection::Runs(id) => (
                AutomationCall::ScheduleShow(ScheduleShowRequest {
                    schedule_id: id.clone(),
                }),
                AutomationKind::Schedule,
                id.as_str(),
            ),
            AutomationCollection::Deliveries(Some(id)) => (
                AutomationCall::WakeShow(WakeShowRequest {
                    wakeup_id: id.clone(),
                }),
                AutomationKind::Wake,
                id.as_str(),
            ),
            _ => return Err(CentralCatalogFailure::InvalidReply),
        };
        let answer = self.catalog.request(request, None).await?;
        let valid = match answer {
            AutomationAnswer::Suspended(value) => {
                value.kind == kind && value.automation_id.as_str() == id
            }
            AutomationAnswer::Schedule(value) => {
                kind == AutomationKind::Schedule && value.schedule_id.as_str() == id
            }
            AutomationAnswer::Wake(value) => {
                kind == AutomationKind::Wake && value.definition.wakeup_id.as_str() == id
            }
            AutomationAnswer::Faulted(value) => {
                value.kind == kind && value.automation_id.as_str() == id
            }
            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err(CentralCatalogFailure::InvalidReply)
        }
    }

    /// Pages organization instruction documents in central's order.
    pub async fn instruction_list(
        &self,
        request: AutomationPageRequest,
        budget: ResultByteBudget,
    ) -> Result<AutomationPage<InstructionSnapshot>, CatalogPageFailure> {
        self.catalog
            .page(
                request.limit,
                budget,
                |limit| {
                    AutomationCall::InstructionList(AutomationPageRequest {
                        limit,
                        ..request.clone()
                    })
                },
                |answer| match answer {
                    AutomationAnswer::Instructions(page) => Some(page),
                    _ => None,
                },
            )
            .await
    }

    /// Pages immutable revisions of the instruction owned by central.
    pub async fn revision_list(
        &self,
        request: RevisionListRequest,
        budget: ResultByteBudget,
    ) -> Result<AutomationPage<RevisionRecord>, CatalogPageFailure> {
        self.catalog
            .page(
                request.limit,
                budget,
                |limit| {
                    AutomationCall::RevisionList(RevisionListRequest {
                        limit,
                        ..request.clone()
                    })
                },
                |answer| match answer {
                    AutomationAnswer::Revisions(page) => Some(page),
                    _ => None,
                },
            )
            .await
    }
}
