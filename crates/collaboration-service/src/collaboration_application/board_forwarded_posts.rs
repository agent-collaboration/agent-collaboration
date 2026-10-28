//! Only posts leave this Router; central supplies their verified provenance.
use super::{BoardOperations, admission_error};
use board_sync_protocol::{BoardPayload, ForwardOutcome, ForwardedPost, ForwardedPostKind};
use message_board::{
    BoardError, BoardOperation, ProjectRequest, RequestEffectsScope, public_replies as public,
};

impl BoardOperations {
    pub(super) async fn forward_non_holder_post(
        &self,
        request: &ProjectRequest,
    ) -> Result<Option<public::WriteReply>, BoardError> {
        let kind = match &request.operation {
            BoardOperation::ThreadCreate { .. } => ForwardedPostKind::ThreadCreate,
            BoardOperation::ThreadPost { .. } => ForwardedPostKind::ThreadPost,
            _ => return Ok(None),
        };
        let admission = self.admission()?;
        let registry = &admission.registry;
        // These probes only establish forwarding eligibility. Native admission
        // owns local open failures, including its exact schema refusal.
        let Ok(superseded) = registry.is_superseded(&request.project_id).await else {
            return Ok(None);
        };
        if !superseded {
            let Ok(store) = registry.existing_project(&request.project_id).await else {
                return Ok(None);
            };
            if let Some(store) = store
                && !matches!(
                    store.writer_state().await,
                    Ok(fleet_sync_lease::WriterState::Released { .. }
                        | fleet_sync_lease::WriterState::Superseded { .. })
                )
            {
                return Ok(None);
            }
        }
        let post = ForwardedPost {
            kind,
            body: BoardPayload::encode(request).map_err(|_| BoardError::board_unavailable())?,
        };
        let outcome = match admission
            .fleet
            .forward_post(request.project_id.clone(), post)
            .await
        {
            Ok(outcome) => outcome,
            Err(collaboration_runtime::FleetClientError::Unavailable) => {
                return Ok(Some(public::WriteReply {
                    result: public::RequestResult::PendingUnreachable,
                    reader: None,
                }));
            }
            Err(error) => return Err(admission_error(error.into())),
        };
        let result = match outcome {
            ForwardOutcome::Answered { reply } => {
                let mut reply: message_board::WriteReply = reply
                    .decode()
                    .map_err(|_| BoardError::board_unavailable())?;
                // The holder's optional read mark says nothing about this
                // sender's read copy. The ledger decision itself is unchanged.
                reply.reader = None;
                return self
                    .public_write_reply(
                        RequestEffectsScope::Project {
                            project_id: request.project_id.clone(),
                        },
                        &request.request_id,
                        reply,
                    )
                    .await
                    .map(Some);
            }
            // Authority changed since the local check. Enter local admission
            // once; never forward its refusal recursively or retry automatically.
            ForwardOutcome::SenderHolds => return Ok(None),
            ForwardOutcome::NotHeld => public::RequestResult::NotHeld,
            ForwardOutcome::HolderUnreachable { last_seen } => {
                let last_seen = last_seen
                    .map(|time| {
                        chrono::DateTime::from_timestamp_millis(time.unix_millis())
                            .ok_or_else(BoardError::board_unavailable)
                    })
                    .transpose()?;
                public::RequestResult::HolderUnreachable { last_seen }
            }
            ForwardOutcome::Unanswered => public::RequestResult::Unanswered,
            ForwardOutcome::Overloaded => public::RequestResult::Overloaded,
        };
        Ok(Some(public::WriteReply {
            result,
            reader: None,
        }))
    }
}
