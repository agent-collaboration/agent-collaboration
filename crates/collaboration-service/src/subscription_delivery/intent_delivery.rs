//! C5's intent consumer uses the project gate and the existing stored-push owner.
use super::*;
use crate::{IntentReply, ProjectAdmission};
use collaboration_protocol::RouterOriginRef;
use message_board::{
    FeedCursor, IntentClaim, IntentRecipient, IntentState, NotifyIntent, ProjectId, Record,
};
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

#[cfg(test)]
#[path = "intent_progress_tests.rs"]
mod intent_progress_tests;

enum IntentHandling {
    Consumed,
    Pending,
}
const INTENT_PAGE_SIZE: usize = 100;

pub(super) enum IntentWorkState {
    Running,
    RescanRequested,
}
impl SubscriptionDeliveryService {
    /// Queue after commit; the board reply never waits on an agent delivery.
    pub async fn queue_project_intents(
        &self,
        admission: Arc<ProjectAdmission>,
        project: ProjectId,
    ) -> Result<(), BoardError> {
        self.require_started().await?;
        let mut work = self.inner.intent_work.lock().await;
        if let Some(state) = work.get_mut(&project) {
            *state = IntentWorkState::RescanRequested;
            return Ok(());
        }
        work.insert(project.clone(), IntentWorkState::Running);
        drop(work);
        let service = Self {
            inner: self.inner.clone(),
            lifetime: ServiceHandleLifetime::Background,
        };
        self.inner.tasks.spawn(async move {
            loop {
                if let Err(error)=service.consume_project_intents(&admission,&project).await {tracing::warn!(%error,project=project.as_str(),"board intent consumer could not finish");}
                let mut work=service.inner.intent_work.lock().await;
                if matches!(work.remove(&project),Some(IntentWorkState::RescanRequested)) && !service.inner.shutdown.is_cancelled() {work.insert(project.clone(),IntentWorkState::Running);continue;}
                break;
            }
        });
        Ok(())
    }
    pub async fn recover_board_intents(
        &self,
        admission: Arc<ProjectAdmission>,
    ) -> Result<(), BoardError> {
        for project in admission
            .registry
            .project_ids()
            .await
            .map_err(|_| BoardError::board_unavailable())?
        {
            self.queue_project_intents(admission.clone(), project)
                .await?;
        }
        Ok(())
    }
    async fn consume_project_intents(
        &self,
        admission: &Arc<ProjectAdmission>,
        project: &ProjectId,
    ) -> Result<(), BoardError> {
        let Some(store) = admission
            .registry
            .existing_project(project)
            .await
            .map_err(|_| BoardError::board_unavailable())?
        else {
            return Ok(());
        };
        let saved = self
            .inner
            .push
            .store
            .lock()
            .await
            .intents_scanned_through(project)
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        let mut read = store
            .snapshot()
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        let head = read.high_water();
        let Some(floor) = read
            .intent_scan_floor(&admission.installation)
            .await
            .map_err(|_| BoardError::board_unavailable())?
        else {
            read.finish()
                .await
                .map_err(|_| BoardError::board_unavailable())?;
            return Ok(());
        };
        let mut after = saved
            .filter(|saved| saved.epoch >= floor.epoch && saved.position >= floor.position)
            .unwrap_or(floor);
        if after.epoch > head.epoch || after.position > head.position {
            return Err(BoardError::board_unavailable());
        }
        loop {
            let mut keys = read
                .intent_page_keys(
                    after.position,
                    head.position,
                    u64::try_from(INTENT_PAGE_SIZE + 1)
                        .map_err(|_| BoardError::board_unavailable())?,
                )
                .await
                .map_err(|_| BoardError::board_unavailable())?;
            let has_more = keys.len() > INTENT_PAGE_SIZE;
            keys.truncate(INTENT_PAGE_SIZE);
            let page_through = if has_more {
                keys.last()
                    .ok_or_else(BoardError::board_unavailable)?
                    .cursor
            } else {
                head
            };
            read.validate_intent_scan_range(
                after.position,
                page_through.position,
                &admission.installation,
            )
            .await
            .map_err(|_| BoardError::board_unavailable())?;
            let versions: Vec<_> = keys
                .iter()
                .map(|key| (key.intent.clone(), key.cursor))
                .collect();
            let mut consumed = if versions.is_empty() {
                HashSet::new()
            } else {
                self.inner
                    .push
                    .store
                    .lock()
                    .await
                    .consumed_intents(project, &versions)
                    .await
                    .map_err(|_| BoardError::board_unavailable())?
            };
            let needed: Vec<_> = keys
                .iter()
                .filter(|key| !consumed.contains(&(key.intent.clone(), key.cursor)))
                .map(|key| key.intent.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .map(Into::into)
                .collect();
            let stored = if needed.is_empty() {
                Vec::new()
            } else {
                read.record_batch(&needed)
                    .await
                    .map_err(|_| BoardError::board_unavailable())?
            };
            read.finish()
                .await
                .map_err(|_| BoardError::board_unavailable())?;
            if stored.len() != needed.len() {
                return Err(BoardError::board_unavailable());
            }
            let mut stored: BTreeMap<_, _> = stored
                .into_iter()
                .map(|record| (record.reference().id.clone(), record))
                .collect();
            let mut through = after;
            for key in &keys {
                let intent_id = key.intent.clone();
                if !consumed.contains(&(intent_id.clone(), key.cursor)) {
                    let handled = async {
                        if self.inner.shutdown.is_cancelled() {
                            return Ok(IntentHandling::Pending);
                        }
                        let Record::NotifyIntent(intent) = stored
                            .remove(&message_board::RecordId::from(intent_id.clone()))
                            .ok_or_else(BoardError::board_unavailable)?
                            .decode()
                            .map_err(|_| BoardError::board_unavailable())?
                        else {
                            return Err(BoardError::board_unavailable());
                        };
                        let outcome = self.handle_intent(admission, project, &intent).await?;
                        if matches!(outcome, IntentHandling::Consumed) {
                            for version in keys.iter().filter(|key| key.intent == intent_id) {
                                self.mark_intent_progress(project, &intent_id, version.cursor)
                                    .await?;
                            }
                        }
                        Ok::<_, BoardError>(outcome)
                    }
                    .await;
                    match handled {
                        Ok(IntentHandling::Consumed) => {
                            consumed.extend(
                                keys.iter()
                                    .filter(|key| key.intent == intent_id)
                                    .map(|key| (key.intent.clone(), key.cursor)),
                            );
                        }
                        pending_or_failed => {
                            self.save_intent_cursor(project, through).await?;
                            return pending_or_failed.map(|_| ());
                        }
                    }
                }
                through = key.cursor;
            }
            self.save_intent_cursor(project, page_through).await?;
            if !has_more {
                return Ok(());
            }
            after = page_through;
            read = store
                .snapshot()
                .await
                .map_err(|_| BoardError::board_unavailable())?;
            if read.high_water().epoch != head.epoch {
                return Err(BoardError::board_unavailable());
            }
        }
    }
    async fn save_intent_cursor(
        &self,
        project: &ProjectId,
        through: FeedCursor,
    ) -> Result<(), BoardError> {
        self.inner
            .push
            .store
            .lock()
            .await
            .advance_intents_scanned_through(project, through)
            .await
            .map_err(|_| BoardError::board_unavailable())
    }
    async fn handle_intent(
        &self,
        admission: &ProjectAdmission,
        project: &ProjectId,
        intent: &NotifyIntent,
    ) -> Result<IntentHandling, BoardError> {
        let mut pending = VecDeque::from([intent.clone()]);
        while let Some(intent) = pending.pop_front() {
            let mut handled_cursor = None;
            let (recipient, replacements) = match intent.state() {
                IntentState::Emitted {} => match admission
                    .claim_intent(project, &intent.intent_id)
                    .await
                    .map_err(|_| BoardError::board_unavailable())?
                {
                    IntentReply::Deferred { replacements, .. } => (None, replacements),
                    IntentReply::Recorded { write, claim } => {
                        if !matches!(write.result, message_board::RequestResult::Decided(_)) {
                            return Ok(IntentHandling::Pending);
                        }
                        handled_cursor = intent_decision_cursor(&write, &intent.intent_id)?;
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
                IntentState::HandedOff {} => {
                    let IntentRecipient::Resolved(recipient) = intent.recipient() else {
                        return Err(BoardError::board_unavailable());
                    };
                    let recipient = matches!(recipient.actor.declared_identity(), Some(Identity::Session { session }) if session.endpoint.service_id == admission.installation).then(|| recipient.clone());
                    (recipient, Vec::new())
                }
                IntentState::Superseded {} | IntentState::Refused { .. } => (None, Vec::new()),
            };
            if let Some(recipient) = recipient {
                self.hand_intent_to_push_owner(admission, &intent, &recipient)
                    .await?;
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
    async fn mark_intent_progress(
        &self,
        project: &ProjectId,
        intent: &message_board::NotifyIntentId,
        state_cursor: FeedCursor,
    ) -> Result<(), BoardError> {
        self.inner
            .push
            .store
            .lock()
            .await
            .mark_intent_consumed(project, intent, state_cursor)
            .await
            .map_err(|_| BoardError::board_unavailable())
    }
    async fn hand_intent_to_push_owner(
        &self,
        admission: &ProjectAdmission,
        intent: &NotifyIntent,
        recipient: &message_board::ResolvedRecipient,
    ) -> Result<(), BoardError> {
        let project = intent
            .envelope
            .scope
            .project
            .as_ref()
            .ok_or_else(BoardError::board_unavailable)?;
        let Some(Identity::Session { session }) = recipient.actor.declared_identity() else {
            return Err(BoardError::board_unavailable());
        };
        let target: SessionRef = serde_json::from_value(
            serde_json::to_value(session).map_err(|_| BoardError::board_unavailable())?,
        )
        .map_err(|_| BoardError::board_unavailable())?;
        let origin = RouterOriginRef::BoardIntent {
            project: project.clone(),
            intent: intent.intent_id.clone(),
        };
        let existing = self
            .inner
            .push
            .store
            .lock()
            .await
            .get_push_record_by_origin_reference(&origin)
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        let completed = if let Some(existing) = existing {
            existing
        } else {
            let prepared = self
                .inner
                .push
                .board_notification(&target, project, intent)
                .await?;
            self.deliver_direct_message(target, prepared.push_id)
                .await?
        };
        if let Some(collaboration_protocol::DeliveryReceipt {
            outcome: collaboration_protocol::DeliveryOutcome::Rejected(rejection),
            ..
        }) = completed.last_outcome
        {
            let detail = rejection
                .detail
                .unwrap_or_else(|| "Push port refused delivery.".to_owned());
            let reply = admission
                .refuse_intent(project, &intent.intent_id, detail)
                .await
                .map_err(|_| BoardError::board_unavailable())?;
            if !matches!(
                &reply,
                IntentReply::Recorded {
                    write: message_board::WriteReply {
                        result: message_board::RequestResult::Decided(
                            message_board::RequestDecision::Accepted { .. }
                        ),
                        ..
                    },
                    ..
                }
            ) {
                return Err(BoardError::board_unavailable());
            }
            if let IntentReply::Recorded { write, .. } = reply
                && let Some(cursor) = intent_decision_cursor(&write, &intent.intent_id)?
            {
                self.mark_intent_progress(project, &intent.intent_id, cursor)
                    .await?;
            }
        }
        Ok(())
    }
}

fn intent_state_cursor(intent: &NotifyIntent) -> Result<FeedCursor, BoardError> {
    match intent.envelope.position {
        message_board::RecordPosition::Project(cursor) => Ok(cursor),
        message_board::RecordPosition::Fleet(_) | message_board::RecordPosition::Unsequenced => {
            Err(BoardError::board_unavailable())
        }
    }
}

fn intent_decision_cursor(
    reply: &message_board::WriteReply,
    intent: &message_board::NotifyIntentId,
) -> Result<Option<FeedCursor>, BoardError> {
    let message_board::RequestResult::Decided(message_board::RequestDecision::Accepted {
        records,
        positions,
        ..
    }) = &reply.result
    else {
        return Ok(None);
    };
    if records.len() != positions.len() {
        return Err(BoardError::board_unavailable());
    }
    for (record, position) in records.iter().zip(positions) {
        if record.kind == message_board::RecordKind::NotifyIntent
            && record.id.as_str() == intent.as_str()
        {
            let message_board::RecordPosition::Project(cursor) = position else {
                return Err(BoardError::board_unavailable());
            };
            return Ok(Some(*cursor));
        }
    }
    Ok(None)
}
