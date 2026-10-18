//! The one permitted terminal decision change: accepted requests become voided.
use crate::{ProjectStorageError, ProjectWriteTransaction};
use chrono::{DateTime, Utc};
use message_board::{FeedCursor, Record, RecordedActor, RequestDecision, VoidCause};
impl ProjectWriteTransaction {
    pub async fn void_accepted_requests(
        &mut self,
        cause: VoidCause,
        actor: RecordedActor,
        at: DateTime<Utc>,
    ) -> Result<Vec<FeedCursor>, ProjectStorageError> {
        let mut positions = Vec::new();
        let mut after = String::new();
        loop {
            let mut keys = self.accepted_request_page_keys(&after, 101).await?;
            let more = keys.len() > 100;
            keys.truncate(100);
            if let Some(last) = keys.last() {
                after = last.id.as_str().to_owned();
            }
            for stored in self.record_batch(&keys).await? {
                let key = stored.reference().clone();
                let Record::Request(mut ledger) = stored.decode()? else {
                    return Err(ProjectStorageError::InvalidRecord {
                        record: key,
                        field: "decision",
                    });
                };
                if !matches!(ledger.decision, RequestDecision::Accepted { .. }) {
                    return Err(ProjectStorageError::InvalidRecord {
                        record: key,
                        field: "decision",
                    });
                }
                ledger.decision = RequestDecision::Voided {
                    cause: cause.clone(),
                };
                ledger.envelope.actor = actor.clone();
                ledger.envelope.router_time = at;
                positions.push(self.append_record(Record::Request(ledger)).await?);
            }
            if !more {
                break;
            }
        }
        Ok(positions)
    }
}
