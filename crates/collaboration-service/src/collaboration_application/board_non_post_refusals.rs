//! Missing writers are classified from retained directory and read-copy authority.
use super::{BoardOperations, admission_error};
use message_board::{
    BoardError, BoardOperation, CurrentWriter, ProjectRequest, Record, ScopeRefusal,
    public_replies as public,
};

impl BoardOperations<'_> {
    pub(super) async fn cached_non_post_refusal(
        &self,
        request: &ProjectRequest,
    ) -> Result<Option<public::WriteReply>, BoardError> {
        if matches!(
            request.operation,
            BoardOperation::ThreadCreate { .. } | BoardOperation::ThreadPost { .. }
        ) {
            return Ok(None);
        }
        let admission = self.admission()?;
        let registry = &admission.registry;
        // Local files, retained rejections and open failures still belong to
        // admission, which preserves their typed schema and authority refusals.
        // None means continue into that owner, never suppress its error.
        if !matches!(
            registry.rejected_project(&request.project_id).await,
            Ok(None)
        ) || !matches!(
            registry.existing_project(&request.project_id).await,
            Ok(None)
        ) {
            return Ok(None);
        }
        let authority = registry
            .completed_project_authority(&request.project_id)
            .await
            .map_err(|error| admission_error(error.into()))?;
        let directory = admission
            .fleet
            .record(&request.project_id.clone().into())
            .await
            .map_err(|error| admission_error(error.into()))?;
        let known_in_fleet = match directory {
            Some(Record::DirectoryEntry(_)) => true,
            None => false,
            Some(_) => return Err(BoardError::board_unavailable()),
        };
        let result = if known_in_fleet || authority.is_some() {
            let current = authority.and_then(|(writer, observation)| match writer {
                CurrentWriter::Held { epoch } => Some(epoch),
                CurrentWriter::Unknown {} | CurrentWriter::Unheld {} => {
                    observation.map(|observation| observation.lease_version.epoch())
                }
            });
            public::RequestResult::NotWriter { current }
        } else {
            public::RequestResult::ScopeUnavailable(ScopeRefusal::ProjectNotFound {
                project: request.project_id.clone(),
            })
        };
        // Optional reader state would refresh remotely. These pre-admission
        // refusals deliberately report no new read observation or ledger effect.
        Ok(Some(public::WriteReply {
            result,
            reader: None,
        }))
    }
}
