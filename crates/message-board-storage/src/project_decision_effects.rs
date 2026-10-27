//! Request receipts use immutable feed metadata, independently of present bodies.
use crate::{ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction};
use message_board::{
    ActorOrigin, DecisionEffect, DecisionEffectsPage, EpochNumber, FeedCursor, FeedPosition,
    MAX_EFFECT_PAGE_ENTRIES, MachineLabel, ProjectFeedEntry, Record, RecordId, RecordKind,
    RecordPlacement, RecordRef, RequestDecision, RequestEffectsCursor, RequestEffectsRead,
    RequestEffectsRefusal, RequestEffectsRequest, RequestEffectsScope, RequestLedgerEntry,
    ServiceId, StoredDecision,
};
use sqlx::Executor;
use sqlx_turso::Turso;

struct StoredDecisionEffect {
    record_kind: String,
    record_id: String,
    epoch: i64,
    position: i64,
    origin_installation: String,
    origin_machine: String,
}

impl StoredDecisionEffect {
    fn decode(self) -> Result<DecisionEffect, ProjectStorageError> {
        let invalid = || ProjectStorageError::InvalidFeed;
        let kind = serde_json::from_value(serde_json::Value::String(self.record_kind))
            .map_err(|_| invalid())?;
        if matches!(
            kind,
            RecordKind::Request
                | RecordKind::ReadMark
                | RecordKind::Workspace
                | RecordKind::Board
                | RecordKind::Adr
        ) {
            return Err(invalid());
        }
        let machine = MachineLabel::try_from(self.origin_machine.clone()).map_err(|_| invalid())?;
        if machine.as_str() != self.origin_machine {
            return Err(invalid());
        }
        Ok((
            RecordRef {
                kind,
                id: RecordId::try_from(self.record_id).map_err(|_| invalid())?,
            },
            RecordPlacement::Project(ProjectFeedEntry {
                cursor: FeedCursor {
                    epoch: EpochNumber::try_from(u64::try_from(self.epoch).map_err(|_| invalid())?)
                        .map_err(|_| invalid())?,
                    position: FeedPosition::try_from(
                        u64::try_from(self.position).map_err(|_| invalid())?,
                    )
                    .map_err(|_| invalid())?,
                },
                origin: ActorOrigin {
                    installation: ServiceId::try_from(self.origin_installation)
                        .map_err(|_| invalid())?,
                    machine,
                },
            }),
        ))
    }
}

pub(crate) async fn verify_decision<TScope>(
    scope: &mut TScope,
    ledger: &RequestLedgerEntry,
) -> Result<(), ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let request = ledger.request_id.as_str();
    let rows = sqlx_turso::query_as!(StoredDecisionEffect,
        r#"SELECT record_kind AS "record_kind!: String", record_id AS "record_id!: String", epoch AS "epoch!: i64", position AS "position!: i64", origin_installation AS "origin_installation!: String", origin_machine AS "origin_machine!: String" FROM project_feed INDEXED BY feed_by_request_position WHERE request_id=? AND record_kind <> 'request' ORDER BY position"#,
        request).fetch_all(&mut *scope).await?;
    let effects = rows
        .into_iter()
        .map(StoredDecisionEffect::decode)
        .collect::<Result<Vec<_>, _>>()?;
    let valid = match &ledger.decision {
        StoredDecision::Accepted {
            effects: header, ..
        } => header.matches_full(&effects),
        StoredDecision::Rejected { .. } => effects.is_empty(),
    };
    if !valid {
        return Err(invalid_receipt(ledger));
    }
    Ok(())
}

async fn decision_page<TScope>(
    scope: &mut TScope,
    ledger: &RequestLedgerEntry,
    after: Option<&RequestEffectsCursor>,
) -> Result<RequestEffectsRead, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let (header, change) = match &ledger.decision {
        StoredDecision::Rejected { reasons } => {
            return Ok(if after.is_none() {
                RequestEffectsRead::Decision {
                    decision: RequestDecision::Rejected {
                        reasons: reasons.clone(),
                    },
                }
            } else {
                refused(RequestEffectsRefusal::CursorInvalid)
            });
        }
        StoredDecision::Accepted { effects, change } => (effects, *change),
    };
    let after_position = match after {
        None => 0,
        Some(RequestEffectsCursor::Project { position }) => {
            i64::try_from(position.get()).map_err(|_| invalid_receipt(ledger))?
        }
        Some(_) => return Ok(refused(RequestEffectsRefusal::CursorInvalid)),
    };
    let request = ledger.request_id.as_str();
    let seen = if after.is_some() {
        let prefix = sqlx_turso::query!(
            r#"SELECT COUNT(*) AS "count!: i64", COALESCE(MAX(position), 0) AS "last!: i64" FROM project_feed INDEXED BY feed_by_request_position WHERE request_id=? AND record_kind <> 'request' AND position <= ?"#,
            request, after_position).fetch_one(&mut *scope).await?;
        if prefix.count == 0 || prefix.last != after_position {
            return Ok(refused(RequestEffectsRefusal::CursorInvalid));
        }
        u64::try_from(prefix.count).map_err(|_| invalid_receipt(ledger))?
    } else {
        0
    };
    let mut rows = sqlx_turso::query_as!(StoredDecisionEffect,
        r#"SELECT record_kind AS "record_kind!: String", record_id AS "record_id!: String", epoch AS "epoch!: i64", position AS "position!: i64", origin_installation AS "origin_installation!: String", origin_machine AS "origin_machine!: String" FROM project_feed INDEXED BY feed_by_request_position WHERE request_id=? AND record_kind <> 'request' AND position > ? ORDER BY position LIMIT 257"#,
        request, after_position).fetch_all(&mut *scope).await?;
    let more = rows.len() > MAX_EFFECT_PAGE_ENTRIES;
    rows.truncate(MAX_EFFECT_PAGE_ENTRIES);
    let entries = rows
        .into_iter()
        .map(StoredDecisionEffect::decode)
        .collect::<Result<Vec<_>, _>>()?;
    let through = seen
        .checked_add(u64::try_from(entries.len()).map_err(|_| invalid_receipt(ledger))?)
        .ok_or_else(|| invalid_receipt(ledger))?;
    if (more && through >= header.count) || (!more && through != header.count) {
        return Err(invalid_receipt(ledger));
    }
    let next = if more {
        Some(RequestEffectsCursor::Project {
            position: entries
                .last()
                .and_then(|(_, placement)| placement.project_cursor())
                .ok_or_else(|| invalid_receipt(ledger))?
                .position,
        })
    } else {
        None
    };
    let effects = DecisionEffectsPage::from_page(entries, header, next)
        .map_err(|_| invalid_receipt(ledger))?;
    Ok(RequestEffectsRead::Decision {
        decision: RequestDecision::Accepted { effects, change },
    })
}

fn refused(reason: RequestEffectsRefusal) -> RequestEffectsRead {
    RequestEffectsRead::Refused { reason }
}

fn invalid_receipt(ledger: &RequestLedgerEntry) -> ProjectStorageError {
    ProjectStorageError::InvalidRecord {
        record: ledger.request_id.clone().into(),
        field: "effects",
    }
}

pub(crate) async fn decision_reply<TScope>(
    scope: &mut TScope,
    ledger: &RequestLedgerEntry,
) -> Result<RequestDecision, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    match decision_page(scope, ledger, None).await? {
        RequestEffectsRead::Decision { decision } => Ok(decision),
        RequestEffectsRead::Refused { .. } => Err(invalid_receipt(ledger)),
    }
}

impl ProjectSnapshot {
    pub async fn request_decision(
        &mut self,
        ledger: &RequestLedgerEntry,
    ) -> Result<RequestDecision, ProjectStorageError> {
        crate::project_record_io::validate_project_record(
            &Record::Request(ledger.clone()),
            &self.project_id,
        )?;
        decision_reply(&mut self.transaction, ledger).await
    }

    pub async fn request_effects(
        &mut self,
        request: &RequestEffectsRequest,
    ) -> Result<RequestEffectsRead, ProjectStorageError> {
        if !matches!(&request.scope, RequestEffectsScope::Project { project_id } if project_id == &self.project_id)
        {
            return Ok(refused(RequestEffectsRefusal::RequestNotInScope));
        }
        let Some(Record::Request(ledger)) = self.record(&request.request.clone().into()).await?
        else {
            return Ok(refused(RequestEffectsRefusal::UnknownRequest));
        };
        decision_page(&mut self.transaction, &ledger, request.after.as_ref()).await
    }
}

impl ProjectWriteTransaction {
    pub async fn request_decision(
        &mut self,
        ledger: &RequestLedgerEntry,
    ) -> Result<RequestDecision, ProjectStorageError> {
        crate::project_record_io::validate_project_record(
            &Record::Request(ledger.clone()),
            &self.project_id,
        )?;
        decision_reply(&mut self.transaction, ledger).await
    }
}
