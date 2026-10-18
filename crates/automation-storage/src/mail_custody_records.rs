//! Private Local mail rows. Payload and public receipt are distinct persisted responsibilities.
use crate::{
    StorageError,
    mail_custody::{LocalMailSnapshot, LocalMessageProjection},
};
use chrono::{DateTime, Utc};
use collaboration_mail_protocol::{MailDigest, MailItem, MailItemId, MailStatus, StatusVersion};
use collaboration_protocol::PushMessageSendResult;
use sqlx::SqliteConnection;

pub(crate) struct StoredLocalMail {
    pub snapshot: LocalMailSnapshot,
    pub item: Option<MailItem>,
    pub projection: Option<LocalMessageProjection>,
}
struct MailItemRow {
    item_id: String,
    digest: String,
    direction: String,
    item_json: Option<String>,
    status_json: String,
    status_version: i64,
    payload_bytes: i64,
    projection_json: Option<String>,
    result_json: Option<String>,
    submit_attempted_at: Option<String>,
    created_at: String,
    terminal_at: Option<String>,
}
fn invalid(field: &'static str) -> StorageError {
    StorageError::InvalidMail { field }
}
impl MailItemRow {
    fn decode(self) -> Result<StoredLocalMail, StorageError> {
        let id: MailItemId = self.item_id.try_into().map_err(|_| invalid("itemId"))?;
        let digest: MailDigest = self.digest.try_into().map_err(|_| invalid("digest"))?;
        if self.direction != "local" {
            return Err(invalid("direction"));
        }
        let item: Option<MailItem> = self
            .item_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|_| invalid("payload"))?;
        if item
            .as_ref()
            .is_some_and(|item| item.id() != &id || item.digest() != &digest)
        {
            return Err(invalid("digest"));
        }
        let payload_bytes = self
            .item_json
            .as_ref()
            .map(|text| i64::try_from(text.len()).map_err(|_| invalid("payloadBytes")))
            .transpose()?
            .unwrap_or(0);
        if self.payload_bytes != payload_bytes {
            return Err(invalid("payloadBytes"));
        }
        let status: MailStatus =
            serde_json::from_str(&self.status_json).map_err(|_| invalid("status"))?;
        let version =
            StatusVersion::new(u64::try_from(self.status_version).map_err(|_| invalid("version"))?);
        let result: Option<PushMessageSendResult> = self
            .result_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|_| invalid("result"))?;
        let projection: Option<LocalMessageProjection> = self
            .projection_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|_| invalid("projection"))?;
        let terminal_at = self
            .terminal_at
            .map(crate::push_record_rows::decode_timestamp)
            .transpose()?;
        crate::push_record_rows::decode_timestamp(self.created_at)?;
        self.submit_attempted_at
            .map(crate::push_record_rows::decode_timestamp)
            .transpose()?;
        if terminal_at.is_some() != status.is_terminal()
            || (item.is_none()
                && (!status.is_terminal() || result.is_none() || projection.is_some()))
            || (item.is_some() && projection.is_none())
        {
            return Err(invalid("lifecycle"));
        }
        if let Some(answer) = &result {
            let link = collaboration_protocol::RouterLink::parse(&answer.link)
                .map_err(|_| invalid("result"))?;
            if status_for_result(answer)? != status {
                return Err(invalid("result"));
            }
            if answer.push_id.as_str() != id.as_str()
                || link.push_id() != &answer.push_id
                || link.machine_id().as_str()
                    != String::from(answer.target.endpoint.service_id.clone())
                || answer.target_identity.is_empty()
            {
                return Err(invalid("result"));
            }
            if let Some(item) = &item {
                let target: message_board::SessionRef = answer
                    .target
                    .clone()
                    .try_into()
                    .map_err(|_| invalid("result"))?;
                if &target != item.recipient() {
                    return Err(invalid("result"));
                }
            }
        }
        if status.is_terminal() && result.is_none() {
            return Err(invalid("result"));
        }
        if !matches!(
            status,
            MailStatus::AtMachine {}
                | MailStatus::Held {}
                | MailStatus::Delivered { .. }
                | MailStatus::Uncertain {}
                | MailStatus::Refused { .. }
        ) {
            return Err(invalid("status"));
        }
        Ok(StoredLocalMail {
            snapshot: LocalMailSnapshot {
                item_id: id,
                digest,
                status,
                version,
                terminal_at,
                compacted: item.is_none(),
                result,
            },
            item,
            projection,
        })
    }
}
pub(crate) async fn read(
    connection: &mut SqliteConnection,
    id: &MailItemId,
) -> Result<Option<StoredLocalMail>, StorageError> {
    let row = sqlx::query_as!(MailItemRow, "SELECT item_id,digest,direction,item_json,status_json,status_version,payload_bytes,projection_json,result_json,submit_attempted_at,created_at,terminal_at FROM mail_items WHERE item_id=?", id.as_str()).fetch_optional(connection).await?;
    row.map(MailItemRow::decode).transpose()
}
pub(crate) async fn insert(
    connection: &mut SqliteConnection,
    item: &MailItem,
    projection: &LocalMessageProjection,
    created_at: DateTime<Utc>,
) -> Result<(), StorageError> {
    let payload = serde_json::to_string(item).map_err(|_| invalid("payload"))?;
    let projection = serde_json::to_string(projection).map_err(|_| invalid("projection"))?;
    let length = i64::try_from(payload.len()).map_err(|_| invalid("payloadBytes"))?;
    let status = serde_json::to_string(&MailStatus::AtMachine {}).map_err(|_| invalid("status"))?;
    let created_at = crate::push_record_rows::serialize_timestamp(created_at);
    sqlx::query!("INSERT INTO mail_items(item_id,digest,direction,item_json,status_json,status_version,payload_bytes,projection_json,result_json,submit_attempted_at,created_at,terminal_at) VALUES (?,?,'local',?,?,0,?,?,NULL,NULL,?,NULL)", item.id().as_str(), item.digest().as_str(), payload,status,length,projection,created_at).execute(connection).await?;
    Ok(())
}

pub(crate) async fn reserve_capacity(
    connection: &mut SqliteConnection,
    item: &MailItem,
) -> Result<(), StorageError> {
    let row = sqlx::query!("SELECT COALESCE(SUM(payload_bytes+2048),0) AS \"reserved!: i64\" FROM mail_items WHERE terminal_at IS NULL").fetch_one(connection).await?;
    let encoded = serde_json::to_vec(item).map_err(|_| invalid("payload"))?;
    check_capacity(row.reserved, encoded.len())
}
fn check_capacity(reserved: i64, payload: usize) -> Result<(), StorageError> {
    const INBOX_CAP: i64 = 256 * 1024 * 1024;
    let size = i64::try_from(payload).map_err(|_| invalid("payloadBytes"))?;
    if reserved < 0 {
        return Err(invalid("capacity"));
    }
    if reserved
        .checked_add(size)
        .and_then(|size| size.checked_add(2048))
        .is_none_or(|size| size > INBOX_CAP)
    {
        return Err(StorageError::MailRefused(
            collaboration_mail_protocol::MailRefusal::MailboxFull {},
        ));
    }
    Ok(())
}

pub(crate) fn status_for_result(
    result: &PushMessageSendResult,
) -> Result<MailStatus, StorageError> {
    use collaboration_mail_protocol::{CarrierAcceptance as Accepted, MailRefusal};
    use collaboration_protocol::{
        DeliveryOutcome as Outcome, DeliveryRejectionReason as Reason, PushDeliveryState as State,
    };
    Ok(match (&result.delivery_state, &result.receipt.outcome) {
        (
            State::Held,
            Outcome::NotSubmitted {
                retryable: true, ..
            },
        ) => MailStatus::Held {},
        (State::OutcomeUnknown, Outcome::Unknown) => MailStatus::Uncertain {},
        (State::Delivered, outcome) => MailStatus::Delivered {
            acceptance: match outcome {
                Outcome::Started => Accepted::Started,
                Outcome::Steered => Accepted::Steered,
                Outcome::StartedOrSteered => Accepted::StartedOrSteered,
                Outcome::Queued => Accepted::Queued,
                Outcome::PeerMessageWritten => Accepted::PeerMessageWritten,
                _ => return Err(invalid("result")),
            },
        },
        (State::Rejected, Outcome::Rejected(reason)) => MailStatus::Refused {
            reason: match reason.reason {
                Reason::NoRoute | Reason::ProviderSessionNotFound => {
                    MailRefusal::RecipientUnknown {}
                }
                _ => MailRefusal::RecipientUnsupported {},
            },
        },
        (State::Rejected, Outcome::NotSubmitted { .. }) => MailStatus::Refused {
            reason: MailRefusal::DroppedByPolicy {},
        },
        _ => return Err(invalid("result")),
    })
}

pub(crate) async fn record_carrier(
    connection: &mut SqliteConnection,
    record: &collaboration_protocol::PushRecord,
) -> Result<(), StorageError> {
    use collaboration_protocol::PushDeliveryState as State;
    let id: MailItemId = record
        .push_id
        .clone()
        .try_into()
        .map_err(|_| invalid("itemId"))?;
    let Some(stored) = read(connection, &id).await? else {
        return Ok(());
    };
    if stored.snapshot.status.is_terminal() {
        return Err(invalid("terminalOutcome"));
    }
    let item = stored.item.ok_or_else(|| invalid("payload"))?;
    let draft = crate::push_records::draft_for_record(record);
    if crate::mail_custody::local_item(&draft)?.digest() != item.digest() {
        return Err(invalid("carrier"));
    }
    let result = if record.last_outcome.is_some() {
        Some(crate::mail_custody::result_for(
            record,
            &stored.projection.ok_or_else(|| invalid("projection"))?,
        )?)
    } else {
        stored.snapshot.result
    };
    let status = if matches!(record.delivery_state, State::Pending | State::Attempted) {
        stored.snapshot.status.clone()
    } else {
        status_for_result(result.as_ref().ok_or_else(|| invalid("result"))?)?
    };
    let terminal = status.is_terminal();
    let terminal_at = if terminal {
        Some(crate::push_record_rows::serialize_timestamp(
            record.settled_at.ok_or_else(|| invalid("terminalAt"))?,
        ))
    } else {
        None
    };
    let version = stored
        .snapshot
        .version
        .get()
        .checked_add(u64::from(status != stored.snapshot.status))
        .and_then(|value| i64::try_from(value).ok())
        .ok_or_else(|| invalid("version"))?;
    let status = serde_json::to_string(&status).map_err(|_| invalid("status"))?;
    let result = result
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|_| invalid("result"))?;
    let attempted_at = (record.delivery_state == State::Attempted)
        .then(|| crate::push_record_rows::serialize_timestamp(Utc::now()));
    sqlx::query!("UPDATE mail_items SET status_json=?,status_version=?,result_json=?,submit_attempted_at=COALESCE(submit_attempted_at,?),terminal_at=?,item_json=CASE WHEN ? THEN NULL ELSE item_json END,projection_json=CASE WHEN ? THEN NULL ELSE projection_json END,payload_bytes=CASE WHEN ? THEN 0 ELSE payload_bytes END WHERE item_id=?", status,version,result,attempted_at,terminal_at,terminal,terminal,terminal,id.as_str()).execute(connection).await?;
    Ok(())
}

#[cfg(test)]
mod capacity_tests {
    use super::*;
    #[test]
    fn admission_reserves_settlement_room_and_refuses_overflow() {
        let cap = 256 * 1024 * 1024;
        assert!(check_capacity(cap - 2049, 1).is_ok());
        assert!(matches!(
            check_capacity(cap - 2048, 1),
            Err(StorageError::MailRefused(
                collaboration_mail_protocol::MailRefusal::MailboxFull {}
            ))
        ));
        assert!(check_capacity(i64::MAX, 1).is_err());
        assert!(matches!(
            check_capacity(-1, 1),
            Err(StorageError::InvalidMail { field: "capacity" })
        ));
    }
}
