//! A Schedule list applies the same local-custody precedence as Show on each page.
use super::{
    AutomationOperations, ResultByteBudget,
    schedule_list_cursor::{self, ScheduleListPhase},
};
use crate::{
    CatalogPageFailure, CentralCatalogFailure, ScheduleCommandFailure, ScheduleReadResponse,
    ScheduleSelectionFailure,
};
use automation_storage::{AutomationCollection, AutomationListPosition, StoredAutomationOwner};
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationKind, AutomationPage, AutomationPageRequest,
    CatalogAutomation, EndpointRef, ScheduleId, SessionRef,
};
fn selection(detail: impl std::fmt::Display) -> ScheduleCommandFailure {
    ScheduleSelectionFailure::LocalStoreUnavailable {
        operation_id: None,
        schedule_id: None,
        message: format!(
            "Local Schedule listing selection failed: {detail}; no central fallback was used."
        ),
    }
    .into()
}
fn invalid(field: &str, message: &str) -> CatalogPageFailure {
    crate::automation_inspection_failure::invalid(field, message).into()
}
impl AutomationOperations {
    /// Handover may move an ID between pages; across-page exactly-once is not guaranteed.
    pub async fn schedule_list(
        &self,
        request: AutomationPageRequest,
        budget: ResultByteBudget,
    ) -> Result<AutomationPage<ScheduleReadResponse>, CatalogPageFailure> {
        let phase =
            schedule_list_cursor::decode(request.cursor.as_deref(), self.identity.service_id())
                .map_err(|()| invalid("cursor", "Use this service's Schedule listing cursor."))?;
        let mut limit = u32::from(request.limit);
        loop {
            let page = match &phase {
                ScheduleListPhase::Local { after } => {
                    self.local_schedule_page(after.clone(), limit).await?
                }
                ScheduleListPhase::Central { after } => {
                    self.central_schedule_page(after.clone(), limit).await?
                }
            };
            if budget.admits(&page) {
                return Ok(page);
            }
            if limit == 1 {
                return Err(invalid(
                    "limit",
                    "One complete Schedule record or continuation exceeds the response budget; no record was truncated.",
                ));
            }
            limit /= 2;
        }
    }
    async fn local_schedule_page(
        &self,
        after: Option<AutomationListPosition>,
        limit: u32,
    ) -> Result<AutomationPage<ScheduleReadResponse>, CatalogPageFailure> {
        let store = self
            .store()
            .ok_or_else(|| selection("storage unavailable"))?;
        let (records, next) = {
            let mut store = store.lock().await;
            let page = store
                .list_collection_keys(&AutomationCollection::Schedules, after, limit)
                .await
                .map_err(selection)?;
            let mut records = Vec::new();
            for key in &page.records {
                let id = ScheduleId::try_from(key.resource_id.clone()).map_err(selection)?;
                if store.schedule_command_owner(&id).await.map_err(selection)?
                    != StoredAutomationOwner::Central
                {
                    let snapshot = store
                        .inspect_schedule::<SessionRef, EndpointRef>(&id)
                        .await
                        .map_err(selection)?;
                    let value = crate::schedule_projection::snapshot(snapshot)
                        .map_err(|()| selection("invalid local Schedule"))?;
                    records.push(ScheduleReadResponse::Local { value });
                }
            }
            let next = if page.has_more {
                Some(AutomationListPosition {
                    upper: page
                        .upper
                        .ok_or_else(|| selection("missing local upper key"))?,
                    last: page
                        .records
                        .last()
                        .cloned()
                        .ok_or_else(|| selection("missing local continuation key"))?,
                })
            } else {
                None
            };
            (records, next)
        };
        if records.is_empty() && next.is_none() {
            return self.central_schedule_page(None, limit).await;
        }
        let phase = match next {
            Some(after) => ScheduleListPhase::Local { after: Some(after) },
            None => ScheduleListPhase::Central { after: None },
        };
        Ok(AutomationPage {
            records,
            next_cursor: Some(
                schedule_list_cursor::encode(self.identity.service_id(), phase)
                    .map_err(|()| invalid("cursor", "Cannot encode Schedule continuation."))?,
            ),
        })
    }
    async fn central_schedule_page(
        &self,
        after: Option<String>,
        limit: u32,
    ) -> Result<AutomationPage<ScheduleReadResponse>, CatalogPageFailure> {
        let store = self
            .store()
            .ok_or_else(|| selection("storage unavailable"))?;
        let limit = limit
            .try_into()
            .map_err(|_| invalid("limit", "Use 1 through 100 records."))?;
        let AutomationAnswer::List { automations, next } = self
            .catalog
            .request(
                AutomationCall::List(AutomationPageRequest {
                    cursor: after,
                    limit,
                }),
                None,
            )
            .await?
        else {
            return Err(CentralCatalogFailure::InvalidReply.into());
        };
        let mut records = Vec::new();
        let mut store = store.lock().await;
        for automation in automations {
            let id = match &automation {
                CatalogAutomation::Suspended(value) if value.kind == AutomationKind::Schedule => {
                    ScheduleId::try_from(value.automation_id.as_str().to_owned())
                        .map_err(|_| CentralCatalogFailure::InvalidReply)?
                }
                CatalogAutomation::Schedule(value) => value.schedule_id.clone(),
                CatalogAutomation::Faulted(value) if value.kind == AutomationKind::Schedule => {
                    ScheduleId::try_from(value.automation_id.as_str().to_owned())
                        .map_err(|_| CentralCatalogFailure::InvalidReply)?
                }
                _ => continue,
            };
            if store.schedule_command_owner(&id).await.map_err(selection)?
                == StoredAutomationOwner::Central
            {
                records.push(
                    ScheduleReadResponse::from_catalog(automation)
                        .ok_or(CentralCatalogFailure::InvalidReply)?,
                );
            }
        }
        let next_cursor = next
            .map(|after| {
                schedule_list_cursor::encode(
                    self.identity.service_id(),
                    ScheduleListPhase::Central { after: Some(after) },
                )
            })
            .transpose()
            .map_err(|()| invalid("cursor", "Cannot encode Schedule continuation."))?;
        Ok(AutomationPage {
            records,
            next_cursor,
        })
    }
}
