//! Interval pagination chooses native keys before bodies and current-holder anchors.
use super::board_paging::{fingerprint, invalid_cursor};
use super::{BoardOperations, admission_error};
use message_board::{
    BoardError, ImplementerHolder, OrchestratorHolder, Page, Participant, ParticipantRole,
    RecordedParticipantRole, ThreadParticipantListRequest, ThreadParticipantListResult,
};
use message_board_storage::{
    ParticipantIntervalBatch, ParticipantIntervalKey, decode_board_cursor, encode_board_cursor,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ParticipantPageCursor {
    operation: String,
    filter: String,
    last_key: ParticipantIntervalKey,
}
fn decode_interval(
    batch: &mut ParticipantIntervalBatch,
    decoded: &mut Vec<(ParticipantIntervalKey, Participant)>,
    key: &ParticipantIntervalKey,
) -> Result<Participant, BoardError> {
    if let Some((_, participant)) = decoded.iter().find(|(stored, _)| stored == key) {
        return Ok(participant.clone());
    }
    let participant = batch
        .take(key)
        .map_err(|error| admission_error(error.into()))?;
    decoded.push((key.clone(), participant.clone()));
    Ok(participant)
}
impl BoardOperations<'_> {
    pub async fn thread_participant_list(
        &self,
        request: ThreadParticipantListRequest,
    ) -> Result<ThreadParticipantListResult, BoardError> {
        const OPERATION: &str = "thread_participant_list";
        let filter = fingerprint(&request)?;
        let registry = &self.admission()?.registry;
        let signing_key = registry
            .cursor_key()
            .await
            .map_err(|error| admission_error(error.into()))?;
        let after = request
            .page
            .cursor
            .as_ref()
            .map(|cursor| {
                let cursor: ParticipantPageCursor =
                    decode_board_cursor(signing_key, cursor).map_err(|_| invalid_cursor())?;
                if cursor.operation != OPERATION || cursor.filter != filter {
                    return Err(invalid_cursor());
                }
                Ok(cursor.last_key)
            })
            .transpose()?;
        let project = self.thread_project(&request.thread).await?;
        let store = registry
            .read_project(&project)
            .await
            .map_err(|error| admission_error(error.into()))?
            .ok_or_else(BoardError::board_unavailable)?;
        let mut snapshot = store
            .snapshot()
            .await
            .map_err(|error| admission_error(error.into()))?;
        let limit = usize::try_from(request.page.limit.get()).map_err(|_| invalid_cursor())?;
        let thread = &request.thread.thread;
        let mut page_keys = snapshot
            .participant_page_keys(
                thread,
                after.as_ref(),
                u64::from(request.page.limit.get()) + 1,
            )
            .await
            .map_err(|error| admission_error(error.into()))?;
        let mut has_more = page_keys.len() > limit;
        page_keys.truncate(limit);
        let holder_keys = snapshot
            .participant_holder_keys(thread)
            .await
            .map_err(|error| admission_error(error.into()))?;
        let mut operands = page_keys.clone();
        for key in &holder_keys {
            if !operands.contains(key) {
                operands.push(key.clone());
            }
        }
        let mut batch = snapshot
            .participant_interval_batch(thread, &operands)
            .await
            .map_err(|error| admission_error(error.into()))?;
        let mut decoded = Vec::new();
        let mut records = Vec::new();
        let mut bytes = 2_usize;
        for key in &page_keys {
            let participant = decode_interval(&mut batch, &mut decoded, key)?;
            let size = serde_json::to_vec(&participant)
                .map_err(|_| BoardError::board_unavailable())?
                .len();
            if bytes.saturating_add(size).saturating_add(1) > 900 * 1024 {
                if records.is_empty() {
                    return Err(BoardError::board_unavailable());
                }
                has_more = true;
                break;
            }
            bytes += size + 1;
            records.push(participant);
        }
        let mut holders = Vec::new();
        for key in holder_keys {
            let participant = decode_interval(&mut batch, &mut decoded, &key)?;
            if !participant.is_open()
                || !matches!(
                    participant.role,
                    RecordedParticipantRole::Recorded(
                        ParticipantRole::Orchestrator | ParticipantRole::Implementer
                    )
                )
            {
                return Err(BoardError::board_unavailable());
            }
            holders.push(participant);
        }
        // Includes at most one interval past the byte budget, with full validation.
        let participants = BTreeMap::from([(
            thread.clone(),
            decoded.into_iter().map(|(_, row)| row).collect(),
        )]);
        snapshot
            .validate_participant_anchors(&participants)
            .await
            .map_err(|error| admission_error(error.into()))?;
        snapshot
            .finish()
            .await
            .map_err(|error| admission_error(error.into()))?;
        let next_cursor = if has_more {
            let last = records.last().ok_or_else(BoardError::board_unavailable)?;
            Some(
                encode_board_cursor(
                    signing_key,
                    &ParticipantPageCursor {
                        operation: OPERATION.to_owned(),
                        filter,
                        last_key: ParticipantIntervalKey::of(last),
                    },
                )
                .map_err(|_| invalid_cursor())?,
            )
        } else {
            None
        };
        Ok(ThreadParticipantListResult {
            project_id: project,
            orchestrator: holders
                .iter()
                .find(|row| {
                    row.role == RecordedParticipantRole::Recorded(ParticipantRole::Orchestrator)
                })
                .map(|row| OrchestratorHolder {
                    identity: row.identity.clone(),
                    last_seen_position: row.last_seen_position,
                }),
            implementer: holders
                .iter()
                .find(|row| {
                    row.role == RecordedParticipantRole::Recorded(ParticipantRole::Implementer)
                })
                .map(|row| ImplementerHolder {
                    identity: row.identity.clone(),
                    last_seen_position: row.last_seen_position,
                }),
            page: Page {
                records,
                next_cursor,
            },
        })
    }
}
