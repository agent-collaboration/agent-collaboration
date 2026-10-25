//! Receipt reads inspect terminal decisions without entering write admission.
use super::project_rejection::registration_rejection;
use super::{ProjectAdmission, ProjectAdmissionError};
use message_board::*;

impl ProjectAdmission {
    pub async fn request_effects(
        &self,
        request: &RequestEffectsRequest,
    ) -> Result<RequestEffectsRead, ProjectAdmissionError> {
        let project_id = match &request.scope {
            RequestEffectsScope::Project { project_id } => project_id,
            RequestEffectsScope::Fleet { .. } => {
                return self
                    .fleet
                    .request_effects(request)
                    .await
                    .map_err(Into::into);
            }
        };
        if self.registry.is_rejected_project(project_id).await? {
            return Ok(unknown_request());
        }
        let Some(store) = self.registry.read_project(project_id).await? else {
            return Ok(unknown_request());
        };
        if store
            .registration_outcome()
            .await?
            .as_ref()
            .and_then(registration_rejection)
            .is_some()
        {
            return Ok(unknown_request());
        }
        let mut snapshot = store.snapshot().await?;
        let result = snapshot.request_effects(request).await?;
        snapshot.finish().await?;
        Ok(result)
    }
}

fn unknown_request() -> RequestEffectsRead {
    RequestEffectsRead::Refused {
        reason: RequestEffectsRefusal::UnknownRequest,
    }
}
