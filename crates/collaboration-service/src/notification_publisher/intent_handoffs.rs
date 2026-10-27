//! Admission freezes notifications; this owner publishes their native handoffs.
use super::{BoardError, IntentHandling, ProjectId, PublisherInner, publication_epoch};
use crate::IntentReply;
use message_board::{
    FeedCursor, Identity, IntentClaim, IntentRecipient, IntentState, NotifyIntent, Record,
};
use std::collections::{BTreeSet, VecDeque};

impl PublisherInner {
    pub(super) async fn handle_intent(
        &self,
        project: &ProjectId,
        intent: &NotifyIntent,
    ) -> Result<IntentHandling, BoardError> {
        let admission = &self.admission;
        let mut pending = VecDeque::from([intent.clone()]);
        while let Some(intent) = pending.pop_front() {
            let mut handled_cursor = None;
            let (recipient, replacements) = match intent.state() {
                IntentState::Emitted {} => {
                    match self.claim_native_intent(project, &intent).await? {
                        None => return Ok(IntentHandling::Pending),
                        Some(reply) => match reply {
                            IntentReply::Deferred { replacements, .. } => (None, replacements),
                            IntentReply::Recorded {
                                write,
                                claim,
                                transition,
                            } => {
                                if !matches!(write.result, message_board::RequestResult::Decided(_))
                                {
                                    return Ok(IntentHandling::Pending);
                                }
                                handled_cursor = transition;
                                match claim {
                                    Some(IntentClaim::Claimed {
                                        recipient,
                                        replacements,
                                    }) => (Some(recipient), replacements),
                                    Some(
                                        IntentClaim::Superseded { replacements }
                                        | IntentClaim::Deferred { replacements, .. },
                                    ) => (None, replacements),
                                    None => (None, Vec::new()),
                                }
                            }
                        },
                    }
                }
                IntentState::HandedOff { .. } => {
                    let IntentRecipient::Resolved(recipient) = intent.recipient() else {
                        return Err(BoardError::board_unavailable());
                    };
                    (Some(recipient.clone()), Vec::new())
                }
                IntentState::Superseded {} | IntentState::Refused { .. } => (None, Vec::new()),
            };
            if recipient.is_some()
                && matches!(
                    self.publish_intent_handoff(project, &intent.intent_id)
                        .await?,
                    IntentHandling::Pending
                )
            {
                return Ok(IntentHandling::Pending);
            }
            self.mark_intent_progress(project, &intent.intent_id, intent_state_cursor(&intent)?)
                .await?;
            if let Some(cursor) = handled_cursor {
                self.mark_intent_progress(project, &intent.intent_id, cursor)
                    .await?;
            }
            if !replacements.is_empty() {
                let Some(store) = admission
                    .registry
                    .existing_project(project)
                    .await
                    .map_err(|_| BoardError::board_unavailable())?
                else {
                    return Err(BoardError::board_unavailable());
                };
                let mut read = store
                    .snapshot()
                    .await
                    .map_err(|_| BoardError::board_unavailable())?;
                let keys: Vec<_> = replacements
                    .into_iter()
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .map(Into::into)
                    .collect();
                let stored = read
                    .record_batch(&keys)
                    .await
                    .map_err(|_| BoardError::board_unavailable())?;
                read.finish()
                    .await
                    .map_err(|_| BoardError::board_unavailable())?;
                if stored.len() != keys.len() {
                    return Err(BoardError::board_unavailable());
                }
                for stored in stored {
                    let Record::NotifyIntent(current) = stored
                        .decode()
                        .map_err(|_| BoardError::board_unavailable())?
                    else {
                        return Err(BoardError::board_unavailable());
                    };
                    if current.cause != intent.cause {
                        return Err(BoardError::board_unavailable());
                    }
                    // Direct traversal bypasses old consumed versions and the scan cursor.
                    // A crash before this step is recovered from the new state feed entry.
                    pending.push_back(current);
                }
            }
        }
        Ok(IntentHandling::Consumed)
    }
    pub(super) async fn mark_intent_progress(
        &self,
        project: &ProjectId,
        intent: &message_board::NotifyIntentId,
        state_cursor: FeedCursor,
    ) -> Result<(), BoardError> {
        self.progress
            .mark_intent_consumed(project, intent, state_cursor)
            .await
            .map_err(|_| BoardError::board_unavailable())
    }
    async fn claim_native_intent(
        &self,
        project: &ProjectId,
        intent: &NotifyIntent,
    ) -> Result<Option<IntentReply>, BoardError> {
        use crate::{
            IntentHandoffInput, IntentRecipientEnrollment,
            mail_delivery_runtime::InstallationPeerResolutionError,
        };
        let admission = &self.admission;
        let runtime = self.mail.upgrade();
        let session = match intent.recipient() {
            IntentRecipient::Resolved(recipient) => match recipient.actor.declared_identity() {
                Some(Identity::Session { session }) => Some(session),
                _ => None,
            },
            IntentRecipient::NoHolder { .. } => None,
        };
        let cached = match (&runtime, &session) {
            (Some(runtime), Some(session)) => runtime
                .cached_installation_peer(&session.endpoint.service_id)
                .await
                .map_err(|_| BoardError::board_unavailable())?,
            _ => None,
        };
        let input = cached
            .zip(session.as_ref())
            .map(|(peer, session)| -> Result<_, BoardError> {
                Ok(IntentHandoffInput {
                    item: fleet_sync_protocol::ItemId::new(uuid::Uuid::now_v7())
                        .map_err(|_| BoardError::board_unavailable())?,
                    enrollment: IntentRecipientEnrollment {
                        service: session.endpoint.service_id.clone(),
                        peer,
                    },
                })
            })
            .transpose()?;
        let reply = admission
            .claim_intent(project, &intent.intent_id, input)
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        // NI-2 changes and replacements must first be consumed by the existing
        // traversal. A later version retries a retained intent from its new state.
        let IntentReply::Deferred { recipient, .. } = &reply else {
            return Ok(Some(reply));
        };
        let Some(Identity::Session { session }) = recipient.actor.declared_identity() else {
            return Ok(Some(reply));
        };
        let Some(runtime) = runtime else {
            return Ok(None);
        };
        match runtime
            .resolve_installation_peer(&session.endpoint.service_id)
            .await
        {
            Ok(peer) => admission
                .claim_intent(
                    project,
                    &intent.intent_id,
                    Some(IntentHandoffInput {
                        item: fleet_sync_protocol::ItemId::new(uuid::Uuid::now_v7())
                            .map_err(|_| BoardError::board_unavailable())?,
                        enrollment: IntentRecipientEnrollment {
                            service: session.endpoint.service_id,
                            peer,
                        },
                    }),
                )
                .await
                .map(Some)
                .map_err(|_| BoardError::board_unavailable()),
            Err(InstallationPeerResolutionError::RecipientUnknown) => admission
                .refuse_unknown_intent_recipient(
                    project,
                    &intent.intent_id,
                    session.endpoint.service_id,
                )
                .await
                .map(Some)
                .map_err(|_| BoardError::board_unavailable()),
            Err(InstallationPeerResolutionError::Unavailable) => Ok(None),
            Err(InstallationPeerResolutionError::Corrupt { .. }) => {
                Err(BoardError::board_unavailable())
            }
        }
    }

    async fn publish_intent_handoff(
        &self,
        project: &ProjectId,
        intent_id: &message_board::NotifyIntentId,
    ) -> Result<IntentHandling, BoardError> {
        let admission = &self.admission;
        let store = admission
            .registry
            .existing_project(project)
            .await
            .map_err(|_| BoardError::board_unavailable())?
            .ok_or_else(BoardError::board_unavailable)?;
        let epoch = publication_epoch(&store).await?;
        let mut read = store
            .snapshot()
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        let Some(Record::NotifyIntent(current)) = read
            .record(&intent_id.clone().into())
            .await
            .map_err(|_| BoardError::board_unavailable())?
        else {
            return Err(BoardError::board_unavailable());
        };
        let IntentState::HandedOff { item } = current.state() else {
            return Err(BoardError::board_unavailable());
        };
        let handoff = read
            .mail_handoff(item)
            .await
            .map_err(|_| BoardError::board_unavailable())?
            .ok_or_else(BoardError::board_unavailable)?;
        read.finish()
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        self.publish_delivery_handoff(project, epoch, handoff).await
    }
}

fn intent_state_cursor(intent: &NotifyIntent) -> Result<FeedCursor, BoardError> {
    intent
        .envelope
        .position
        .project_cursor()
        .ok_or_else(BoardError::board_unavailable)
}
