//! Reads follow stored ownership and retain the corresponding local, central or faulted record.
use super::{
    WakeFailureContext, WakeOperations,
    list_cursor::{self, ListPhase},
    wake_failure,
};
use crate::{
    CentralCatalogFailure,
    catalog_results::WakeReadResponse,
    response_budget::ResultByteBudget,
    wake_operation_failure::{WakeOperationFailure, WakeSelectionFailure},
};
use automation_storage::{StoredAutomationOwner, WakeListPosition};
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationKind, AutomationPage, AutomationPageRequest,
    CatalogAutomation, LocalMutationState, SavedMessage, WakeFailureReason, WakeShowRequest,
    WakeupId,
};

fn selection_error(id: Option<WakeupId>, detail: impl std::fmt::Display) -> WakeOperationFailure {
    WakeSelectionFailure::LocalStoreUnavailable {
        operation_id: None,
        wakeup_id: id,
        message: format!(
            "Local Wake ownership selection failed: {detail}; no central fallback was used."
        ),
    }
    .into()
}
fn invalid_page(field: &str, message: &str) -> WakeOperationFailure {
    wake_failure(
        WakeFailureContext::default(),
        WakeFailureReason::InvalidField {
            field: field.into(),
            constraint: message.into(),
        },
        LocalMutationState::None,
    )
    .into()
}

impl WakeOperations {
    pub async fn wake_show(
        &self,
        request: WakeShowRequest,
    ) -> Result<WakeReadResponse, WakeOperationFailure> {
        let id = request.wakeup_id.clone();
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| selection_error(Some(id.clone()), "storage unavailable"))?;
        {
            let mut store = store.lock().await;
            match store
                .wake_command_owner(&id)
                .await
                .map_err(|error| selection_error(Some(id.clone()), error))?
            {
                StoredAutomationOwner::Local | StoredAutomationOwner::HandoverPending => {
                    let record = store
                        .read_wakeup::<SavedMessage>(&id)
                        .await
                        .map_err(|error| selection_error(Some(id.clone()), error))?;
                    let value = crate::wakeup_projection::snapshot(
                        record,
                        &self.service_id,
                        chrono::Utc::now().timestamp_millis(),
                    )
                    .map_err(|()| selection_error(Some(id.clone()), "invalid stored Wake"))?;
                    return Ok(WakeReadResponse::Local { value });
                }
                StoredAutomationOwner::Central => {}
            }
        }
        match self
            .catalog
            .request(AutomationCall::WakeShow(request), None)
            .await?
        {
            AutomationAnswer::Suspended(value)
                if value.kind == AutomationKind::Wake
                    && value.automation_id.as_str() == id.as_str() =>
            {
                Ok(WakeReadResponse::suspended(value))
            }
            AutomationAnswer::Wake(value) if value.definition.wakeup_id == id => {
                Ok(WakeReadResponse::Central { value })
            }
            AutomationAnswer::Faulted(fault)
                if fault.kind == AutomationKind::Wake
                    && fault.automation_id.as_str() == id.as_str() =>
            {
                Ok(WakeReadResponse::faulted(fault))
            }
            _ => Err(CentralCatalogFailure::InvalidReply.into()),
        }
    }

    /// Lists local custody first, then central; the opaque continuation belongs to this service.
    /// Handover can move an ID between pages, so cross-page exactly-once is not guaranteed.
    pub async fn wake_list(
        &self,
        request: AutomationPageRequest,
        budget: ResultByteBudget,
    ) -> Result<AutomationPage<WakeReadResponse>, WakeOperationFailure> {
        let phase =
            list_cursor::decode(request.cursor.as_deref(), &self.service_id).map_err(|()| {
                invalid_page(
                    "cursor",
                    "Use a cursor from this service's Wake listing, or start a fresh listing.",
                )
            })?;
        let mut limit = u32::from(request.limit);
        loop {
            let page = match &phase {
                ListPhase::Local { after } => self.local_wake_page(after.clone(), limit).await?,
                ListPhase::Central { after } => {
                    self.central_wake_page(after.clone(), limit).await?
                }
            };
            if budget.admits(&page) {
                return Ok(page);
            }
            if limit == 1 {
                return Err(invalid_page(
                    "limit",
                    "One complete Wake record or its continuation exceeds the response budget; no record was truncated.",
                ));
            }
            // Every read retries the same phase and original position, never a mutation.
            limit /= 2;
        }
    }

    async fn local_wake_page(
        &self,
        after: Option<WakeListPosition>,
        limit: u32,
    ) -> Result<AutomationPage<WakeReadResponse>, WakeOperationFailure> {
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| selection_error(None, "storage unavailable"))?;
        let (records, next) = {
            let mut store = store.lock().await;
            let page = store
                .list_wakeups::<SavedMessage>(after, limit)
                .await
                .map_err(|error| selection_error(None, error))?;
            let mut records = Vec::new();
            for record in page.records {
                match store
                    .wake_command_owner(&record.definition.wakeup_id)
                    .await
                    .map_err(|error| selection_error(None, error))?
                {
                    StoredAutomationOwner::Central => {}
                    StoredAutomationOwner::Local | StoredAutomationOwner::HandoverPending => {
                        let value = crate::wakeup_projection::snapshot(
                            record,
                            &self.service_id,
                            chrono::Utc::now().timestamp_millis(),
                        )
                        .map_err(|()| selection_error(None, "invalid stored Wake"))?;
                        records.push(WakeReadResponse::Local { value });
                    }
                }
            }
            (records, page.next)
        };
        if records.is_empty() && next.is_none() {
            return self.central_wake_page(None, limit).await;
        }
        let next = match next {
            Some(after) => ListPhase::Local { after: Some(after) },
            None => ListPhase::Central { after: None },
        };
        Ok(AutomationPage {
            records,
            next_cursor: Some(
                list_cursor::encode(&self.service_id, next).map_err(|()| {
                    invalid_page("cursor", "Wake continuation could not be encoded.")
                })?,
            ),
        })
    }

    async fn central_wake_page(
        &self,
        after: Option<String>,
        limit: u32,
    ) -> Result<AutomationPage<WakeReadResponse>, WakeOperationFailure> {
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| selection_error(None, "storage unavailable"))?;
        let limit = limit
            .try_into()
            .map_err(|_| invalid_page("limit", "Use 1 through 100 records."))?;
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
                CatalogAutomation::Suspended(value) if value.kind == AutomationKind::Wake => value
                    .automation_id
                    .as_str()
                    .to_owned()
                    .try_into()
                    .map_err(|_| CentralCatalogFailure::InvalidReply)?,
                CatalogAutomation::Wake(wake) => wake.definition.wakeup_id.clone(),
                CatalogAutomation::Faulted(fault) if fault.kind == AutomationKind::Wake => fault
                    .automation_id
                    .as_str()
                    .to_owned()
                    .try_into()
                    .map_err(|_| CentralCatalogFailure::InvalidReply)?,
                _ => continue,
            };
            if store
                .wake_command_owner(&id)
                .await
                .map_err(|error| selection_error(None, error))?
                != StoredAutomationOwner::Central
            {
                continue;
            }
            match automation {
                CatalogAutomation::Wake(value) => records.push(WakeReadResponse::Central { value }),
                CatalogAutomation::Faulted(fault) => records.push(WakeReadResponse::faulted(fault)),
                CatalogAutomation::Suspended(value) => {
                    records.push(WakeReadResponse::suspended(value))
                }
                CatalogAutomation::Schedule(_) => {
                    return Err(CentralCatalogFailure::InvalidReply.into());
                }
            }
        }
        let next_cursor = next
            .map(|after| {
                list_cursor::encode(&self.service_id, ListPhase::Central { after: Some(after) })
            })
            .transpose()
            .map_err(|()| invalid_page("cursor", "Wake continuation could not be encoded."))?;
        Ok(AutomationPage {
            records,
            next_cursor,
        })
    }
}
