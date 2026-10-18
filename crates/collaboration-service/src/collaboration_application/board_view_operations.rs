//! View transports call the snapshot/projection owner and store nothing themselves.
use super::BoardOperations;
use crate::board_views::{
    ViewReadSelection, ViewSnapshot, continuation_card, lead_owed, owner_waiting, work_view,
};
use message_board::*;

impl BoardOperations<'_> {
    pub async fn view_card(
        &self,
        request: ContinuationCardRequest,
    ) -> Result<ContinuationCard, BoardError> {
        let snapshot = ViewSnapshot::load(
            self.identity,
            &request.project_id,
            ViewReadSelection::Card(&request),
        )
        .await?
        .ok_or_else(BoardError::board_unavailable)?;
        let card = continuation_card(
            snapshot,
            &request,
            self.card_session_state(&request.actor).await,
        )?;
        Ok(card)
    }
    pub async fn view_owner(&self, _request: OwnerViewRequest) -> Result<OwnerView, BoardError> {
        let mut waiting = Vec::new();
        let mut unavailable_projects = Vec::new();
        for project in self.view_projects().await? {
            match ViewSnapshot::load(self.identity, &project, ViewReadSelection::Owner).await {
                Ok(Some(snapshot)) => match owner_waiting(&snapshot) {
                    Ok(rows) => {
                        waiting.extend(rows);
                    }
                    Err(_) => {
                        unavailable_projects.push(project.clone());
                    }
                },
                Ok(None) => {}
                Err(_) => {
                    unavailable_projects.push(project.clone());
                }
            }
        }
        Ok(OwnerView {
            as_of: chrono::Utc::now(),
            waiting,
            unavailable_projects,
        })
    }
    pub async fn view_lead_load(&self, request: LeadLoadRequest) -> Result<LeadLoad, BoardError> {
        let mut owed = Vec::new();
        let mut unavailable_projects = Vec::new();
        if let Some(reader) = request.lead.declared_identity() {
            for project in self.view_projects().await? {
                match ViewSnapshot::load(self.identity, &project, ViewReadSelection::Lead(reader))
                    .await
                {
                    Ok(Some(snapshot)) => match lead_owed(&snapshot) {
                        Ok(items) => owed.extend(items.into_iter().map(|(_, item)| item)),
                        Err(_) => unavailable_projects.push(project),
                    },
                    Ok(None) => {}
                    Err(_) => unavailable_projects.push(project),
                }
            }
        }
        Ok(LeadLoad {
            lead: request.lead,
            owed,
            unavailable_projects,
        })
    }
    pub(super) async fn view_projects(&self) -> Result<Vec<ProjectId>, BoardError> {
        self.admission()?
            .fleet
            .scoped_projects(None, &self.admission()?.registry)
            .await
            .map_err(|error| super::admission_error(error.into()))
    }
    pub async fn view_work(&self, request: ProjectViewRequest) -> Result<WorkView, BoardError> {
        let snapshot = ViewSnapshot::load(
            self.identity,
            &request.project_id,
            ViewReadSelection::Work {
                include_removed: request.include_removed,
            },
        )
        .await?
        .ok_or_else(BoardError::board_unavailable)?;
        work_view(&snapshot, request.include_removed)
    }
}
