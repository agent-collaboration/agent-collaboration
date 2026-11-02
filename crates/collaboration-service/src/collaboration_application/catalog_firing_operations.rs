//! Firing evidence is read from central independently of local Run or delivery projections.
use super::automation_operations::AutomationOperations;
use crate::{
    CentralCatalogFailure, central_catalog::CatalogPageFailure, response_budget::ResultByteBudget,
};
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationPage, FiringId, FiringListRequest, FiringSnapshot,
};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FiringShowRequest {
    pub firing_id: FiringId,
}

impl AutomationOperations {
    pub async fn firing_show(
        &self,
        request: FiringShowRequest,
    ) -> Result<FiringSnapshot, CentralCatalogFailure> {
        match self
            .catalog
            .request(
                AutomationCall::FiringShow {
                    firing_id: request.firing_id.clone(),
                },
                None,
            )
            .await?
        {
            AutomationAnswer::Firing(firing) if firing.firing_id == request.firing_id => Ok(firing),
            _ => Err(CentralCatalogFailure::InvalidReply),
        }
    }

    pub async fn firing_list(
        &self,
        request: FiringListRequest,
        budget: ResultByteBudget,
    ) -> Result<AutomationPage<FiringSnapshot>, CatalogPageFailure> {
        self.catalog
            .page(
                request.limit,
                budget,
                |limit| {
                    AutomationCall::FiringList(FiringListRequest {
                        limit,
                        ..request.clone()
                    })
                },
                |answer| match answer {
                    AutomationAnswer::Firings(page)
                        if page
                            .records
                            .iter()
                            .all(|firing| firing.automation_id == request.automation_id) =>
                    {
                        Some(page)
                    }
                    _ => None,
                },
            )
            .await
    }
}
