//! Derived scan positions bound repeated intent decoding without owning custody.
use super::{BoardError, INTENT_PAGE_SIZE, IntentHandling, ProjectId, PublisherInner};
use message_board::{EpochNumber, FeedCursor, FeedPosition, Record};
use std::collections::{BTreeMap, BTreeSet, HashSet};

impl PublisherInner {
    pub(super) async fn consume_project_intents(
        &self,
        project: &ProjectId,
    ) -> Result<(), BoardError> {
        let admission = &self.admission;
        let Some(store) = admission
            .registry
            .existing_project(project)
            .await
            .map_err(|_| BoardError::board_unavailable())?
        else {
            return Ok(());
        };
        if !matches!(
            store
                .writer_state()
                .await
                .map_err(|_| BoardError::board_unavailable())?,
            fleet_sync_lease::WriterState::Provisional
                | fleet_sync_lease::WriterState::Writing { .. }
        ) {
            return Ok(());
        }
        let saved = self
            .progress
            .intents_scanned_through(project)
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        let mut read = store
            .snapshot()
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        let head = read.high_water();
        let mut after = match saved {
            Some(saved) => match read
                .cursor_validity(saved)
                .await
                .map_err(|_| BoardError::board_unavailable())?
            {
                message_board_storage::CursorValidity::Valid => saved,
                message_board_storage::CursorValidity::Superseded { resume_from } => resume_from,
                message_board_storage::CursorValidity::Invalid => {
                    return Err(BoardError::board_unavailable());
                }
            },
            None => FeedCursor {
                epoch: EpochNumber::INITIAL,
                position: FeedPosition::ZERO,
            },
        };
        // Scan past gaps, but commit only the contiguous consumed prefix.
        let mut contiguous = after;
        let mut prefix_is_open = true;
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
            read.validate_intent_scan_range(after.position, page_through.position)
                .await
                .map_err(|_| BoardError::board_unavailable())?;
            let versions: Vec<_> = keys
                .iter()
                .map(|key| (key.intent.clone(), key.cursor))
                .collect();
            let mut consumed = if versions.is_empty() {
                HashSet::new()
            } else {
                self.progress
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
            let mut pending = HashSet::new();
            for key in &keys {
                if self.shutdown.is_cancelled() {
                    self.save_intent_cursor(project, contiguous).await?;
                    return Ok(());
                }
                let intent_id = key.intent.clone();
                // A page may contain several versions of the same current
                // record. A Pending record is decoded and attempted once.
                if pending.contains(&intent_id) {
                    continue;
                }
                if !consumed.contains(&(intent_id.clone(), key.cursor)) {
                    let handled = async {
                        let Record::NotifyIntent(intent) = stored
                            .remove(&message_board::RecordId::from(intent_id.clone()))
                            .ok_or_else(BoardError::board_unavailable)?
                            .decode()
                            .map_err(|_| BoardError::board_unavailable())?
                        else {
                            return Err(BoardError::board_unavailable());
                        };
                        let outcome = self.handle_intent(project, &intent).await?;
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
                        Ok(IntentHandling::Pending) => {
                            pending.insert(intent_id);
                            prefix_is_open = false;
                        }
                        Err(error) => {
                            self.save_intent_cursor(project, contiguous).await?;
                            return Err(error);
                        }
                    }
                }
                if prefix_is_open {
                    contiguous = key.cursor;
                }
            }
            if prefix_is_open {
                contiguous = page_through;
            }
            self.save_intent_cursor(project, contiguous).await?;
            if !has_more {
                return Ok(());
            }
            after = page_through;
            read = store
                .snapshot()
                .await
                .map_err(|_| BoardError::board_unavailable())?;
            if read.high_water().epoch != head.epoch {
                // Retry this frozen-head pass after a line change; saved cursors are
                // independently resolved through cursor_validity on the next pass.
                return Err(BoardError::board_unavailable());
            }
        }
    }
    async fn save_intent_cursor(
        &self,
        project: &ProjectId,
        through: FeedCursor,
    ) -> Result<(), BoardError> {
        self.progress
            .advance_intents_scanned_through(project, through)
            .await
            .map_err(|_| BoardError::board_unavailable())
    }
}
