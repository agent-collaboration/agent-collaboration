//! The engine side of P8: health and the retired list read from storage only,
//! retry deletes a retired record, discard runs the machine's Discard event.
use super::{
    attempt_records::{StoredAttempt, decode_attempt},
    handle::HandleOutcome,
    query_registry,
    source::{EventSource, ReceiptStatements, SourcePolicy},
};
use super::{
    machines::{
        automation_timer,
        timer_store::{AutomationStore, WakeStore},
        wake_timer,
    },
    port::{Engine, EngineHost},
};
use crate::ObjectHost as _;
use crate::organization_admin_contract::{EngineFailureClass, EngineKindName};
use crate::{
    guarded_statements::{exact, text},
    workspace_decode_error::corrupt,
    workspace_row_codec::column,
};
use central_engine_model::{AttemptState, EngineTime, InstanceKey, Machine};
use central_storage::{GuardedStatement, ReadOutcome, SqlValue, Statement, StoreFailure};

/// Every kind health lists, zeros included. `engine_source` rows are the due-list
/// control subjects; the notice units are whole units, not machines.
const ENGINE_KINDS: &[(&str, EngineKindName)] = &[
    ("wake_timer", EngineKindName::WakeTimer),
    ("automation_timer", EngineKindName::AutomationTimer),
    ("legacy_notice_windows", EngineKindName::LegacyNoticeWindows),
    (
        "legacy_workspace_drains",
        EngineKindName::LegacyWorkspaceDrains,
    ),
    ("engine_source", EngineKindName::EngineSource),
];
/// Test machines' records exist only in test-support stores and are not shown.
#[cfg(feature = "test-support")]
const HIDDEN_KINDS: &[&str] = &["engine_probe", "owed_reply"];
#[cfg(not(feature = "test-support"))]
const HIDDEN_KINDS: &[&str] = &[];

const READ_BY_ID: &str = "SELECT attempt_id,machine_kind,subject_key,state,failure_count,next_try_at,last_failure,last_failure_detail,first_failed_at,last_failed_at,retired_at,created_at,updated_at FROM central_work_attempts WHERE attempt_id=?";
const READ_RETIRED_PAGE: &str = "SELECT attempt_id,machine_kind,subject_key,state,failure_count,next_try_at,last_failure,last_failure_detail,first_failed_at,last_failed_at,retired_at,created_at,updated_at FROM central_work_attempts WHERE state='retired' AND (?1 IS NULL OR attempt_id>?1) ORDER BY attempt_id LIMIT ?2";
const READ_HEALTH: &str = "SELECT machine_kind,SUM(CASE WHEN state='failing' THEN 1 ELSE 0 END) AS failing,SUM(CASE WHEN state='retired' THEN 1 ELSE 0 END) AS retired,MIN(first_failed_at) AS oldest,(SELECT b.last_failure FROM central_work_attempts b WHERE b.machine_kind=a.machine_kind ORDER BY b.last_failed_at DESC,b.attempt_id DESC LIMIT 1) AS last_failure FROM central_work_attempts a GROUP BY machine_kind";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct KindHealth {
    pub kind: EngineKindName,
    pub failing: u32,
    pub retired: u32,
    pub oldest_first_failed_at: Option<EngineTime>,
    pub last_failure: Option<EngineFailureClass>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EngineHealthFacts {
    pub kinds: Vec<KindHealth>,
    pub alarm: Option<EngineTime>,
    pub alarm_missing: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RetiredItem {
    pub attempt_id: uuid::Uuid,
    pub kind: EngineKindName,
    pub subject_key: String,
    pub failure_class: EngineFailureClass,
    pub count: u32,
    pub retired_at: EngineTime,
    pub last_failure: Option<String>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AttemptRefusal {
    NotFound,
    NotRetired,
    DiscardUnsupported,
    Stale,
    SubjectUnreadable,
}
pub(crate) enum DiscardOutcome {
    /// The subject's Discard transition, the guarded record delete and the
    /// caller's statements committed in one batch.
    Committed,
    Refused(AttemptRefusal),
}

/// A shown kind, a hidden test kind (None), or a corrupt stored value.
fn registered_kind(raw: &str) -> Result<Option<EngineKindName>, StoreFailure> {
    if let Some((_, kind)) = ENGINE_KINDS.iter().find(|(stored, _)| *stored == raw) {
        return Ok(Some(*kind));
    }
    if HIDDEN_KINDS.contains(&raw) {
        return Ok(None);
    }
    Err(corrupt("central_work_attempts.machine_kind"))
}
fn failure_class(tag: &str) -> Result<EngineFailureClass, StoreFailure> {
    Ok(match tag {
        "corrupt_row" => EngineFailureClass::CorruptRow,
        "constraint_violated" => EngineFailureClass::ConstraintViolated,
        "statement_rejected" => EngineFailureClass::StatementRejected,
        "refused" => EngineFailureClass::Refused,
        "other_store_unavailable" => EngineFailureClass::OtherStoreUnavailable,
        _ => return Err(corrupt("central_work_attempts.last_failure")),
    })
}
fn integer(rows: &ReadOutcome, row: &[SqlValue], name: &str) -> Result<u32, StoreFailure> {
    match column(rows, row, name)? {
        SqlValue::Integer(value) => u32::try_from(value.get()).map_err(|_| corrupt(name)),
        _ => Err(corrupt(name)),
    }
}
fn optional_text(
    rows: &ReadOutcome,
    row: &[SqlValue],
    name: &str,
) -> Result<Option<String>, StoreFailure> {
    match column(rows, row, name)? {
        SqlValue::Null => Ok(None),
        SqlValue::Text(value) => Ok(Some(value.clone())),
        _ => Err(corrupt(name)),
    }
}
fn one_row(rows: &ReadOutcome, row: &[SqlValue]) -> ReadOutcome {
    ReadOutcome {
        columns: rows.columns.clone(),
        rows: vec![row.to_vec()],
    }
}
/// The guarded delete that retry and discard both commit: only a still-retired
/// record with this id goes, so a concurrent second change finds nothing.
fn delete_retired(attempt_id: uuid::Uuid) -> GuardedStatement {
    exact(
        "DELETE FROM central_work_attempts WHERE attempt_id=? AND state='retired'",
        vec![text(attempt_id)],
    )
}

/// Discard's receipt statements are the caller's admission writes and answer
/// receipt plus the guarded delete; a refused transition writes none of them.
struct DiscardSource(Vec<GuardedStatement>);
impl<M: Machine> EventSource<M> for DiscardSource {
    fn policy(&self) -> SourcePolicy {
        SourcePolicy::Discard
    }
    fn accepted(&self, _state: &M::State) -> Result<ReceiptStatements, StoreFailure> {
        Ok(ReceiptStatements::Admin(self.0.clone()))
    }
}

impl<R: EngineHost> Engine<'_, R> {
    pub(crate) async fn engine_health(&self) -> Result<EngineHealthFacts, StoreFailure> {
        let rows = self
            .host()
            .read(Statement {
                sql: READ_HEALTH,
                args: vec![],
            })
            .await?;
        let mut kinds = ENGINE_KINDS
            .iter()
            .map(|(_, kind)| KindHealth {
                kind: *kind,
                failing: 0,
                retired: 0,
                oldest_first_failed_at: None,
                last_failure: None,
            })
            .collect::<Vec<_>>();
        for row in &rows.rows {
            let kind = match column(&rows, row, "machine_kind")? {
                SqlValue::Text(kind) => registered_kind(kind)?,
                _ => return Err(corrupt("central_work_attempts.machine_kind")),
            };
            let Some(kind) = kind else { continue };
            let oldest = optional_text(&rows, row, "oldest")?
                .map(|time| {
                    time.parse::<EngineTime>()
                        .map_err(|_| corrupt("central_work_attempts.first_failed_at"))
                })
                .transpose()?;
            let entry = kinds
                .iter_mut()
                .find(|entry| entry.kind == kind)
                .ok_or_else(|| corrupt("central_work_attempts.machine_kind"))?;
            *entry = KindHealth {
                kind,
                failing: integer(&rows, row, "failing")?,
                retired: integer(&rows, row, "retired")?,
                oldest_first_failed_at: oldest,
                last_failure: optional_text(&rows, row, "last_failure")?
                    .map(|tag| failure_class(&tag))
                    .transpose()?,
            };
        }
        let alarm = self.host().published_alarm().await?;
        let pending = self
            .host()
            .read(central_engine_model::next_due_sql(
                query_registry::ALARM_INPUTS,
            ))
            .await?;
        let timed_work_pending = pending
            .rows
            .first()
            .is_some_and(|row| row.iter().any(|value| *value != SqlValue::Null));
        Ok(EngineHealthFacts {
            kinds,
            alarm,
            alarm_missing: alarm.is_none() && timed_work_pending,
        })
    }

    /// One page ascending by attempt id, `after` exclusive, plus whether more exist.
    pub(crate) async fn engine_retired_page(
        &self,
        after: Option<uuid::Uuid>,
        limit: u16,
    ) -> Result<(Vec<RetiredItem>, Option<uuid::Uuid>), StoreFailure> {
        let rows = self
            .host()
            .read(Statement {
                sql: READ_RETIRED_PAGE,
                args: vec![
                    after.map_or(SqlValue::Null, text),
                    SqlValue::Integer(
                        (i64::from(limit) + 1)
                            .try_into()
                            .map_err(|_| corrupt("limit"))?,
                    ),
                ],
            })
            .await?;
        let mut items = Vec::new();
        for row in &rows.rows {
            let stored = decode_attempt(&one_row(&rows, row))?
                .ok_or_else(|| corrupt("central_work_attempts.attempt_id"))?;
            let record = stored
                .record
                .map_err(|corruption| corrupt(&corruption.column))?;
            let AttemptState::Retired { retired_at } = record.state else {
                return Err(corrupt("central_work_attempts.state"));
            };
            let Some(kind) = (match column(&rows, row, "machine_kind")? {
                SqlValue::Text(kind) => registered_kind(kind)?,
                _ => return Err(corrupt("central_work_attempts.machine_kind")),
            }) else {
                continue;
            };
            items.push(RetiredItem {
                attempt_id: record.attempt_id,
                kind,
                subject_key: match column(&rows, row, "subject_key")? {
                    SqlValue::Text(key) => key.clone(),
                    _ => return Err(corrupt("central_work_attempts.subject_key")),
                },
                failure_class: failure_class(record.last_failure.storage_tag())?,
                count: record.failure_count.get(),
                retired_at,
                last_failure: super::attempt_records::failure_detail(&record.last_failure),
            });
        }
        let next = if items.len() > usize::from(limit) {
            items.truncate(usize::from(limit));
            items.last().map(|item| item.attempt_id)
        } else {
            None
        };
        Ok((items, next))
    }

    async fn retired_attempt(
        &self,
        attempt_id: uuid::Uuid,
    ) -> Result<Result<(StoredAttempt, EngineKindName, String), AttemptRefusal>, StoreFailure> {
        let rows = self
            .host()
            .read(Statement {
                sql: READ_BY_ID,
                args: vec![text(attempt_id)],
            })
            .await?;
        let Some(row) = rows.rows.first() else {
            return Ok(Err(AttemptRefusal::NotFound));
        };
        let Some(kind) = (match column(&rows, row, "machine_kind")? {
            SqlValue::Text(kind) => registered_kind(kind)?,
            _ => return Err(corrupt("central_work_attempts.machine_kind")),
        }) else {
            return Ok(Err(AttemptRefusal::NotFound));
        };
        let key = match column(&rows, row, "subject_key")? {
            SqlValue::Text(key) => key.clone(),
            _ => return Err(corrupt("central_work_attempts.subject_key")),
        };
        let stored = decode_attempt(&one_row(&rows, row))?
            .ok_or_else(|| corrupt("central_work_attempts.attempt_id"))?;
        match &stored.record {
            Ok(record) if matches!(record.state, AttemptState::Retired { .. }) => {
                Ok(Ok((stored, kind, key)))
            }
            Ok(_) | Err(_) => Ok(Err(AttemptRefusal::NotRetired)),
        }
    }

    /// Retry is the guarded delete in the caller's batch, which also re-arms.
    pub(crate) async fn plan_engine_retry(
        &self,
        attempt_id: uuid::Uuid,
    ) -> Result<Result<GuardedStatement, AttemptRefusal>, StoreFailure> {
        Ok(self
            .retired_attempt(attempt_id)
            .await?
            .map(|_| delete_retired(attempt_id)))
    }

    /// Discard runs the machine's own Discard event through `handle`; the
    /// caller's statements and the guarded delete commit in that same batch.
    pub(crate) async fn engine_discard(
        &self,
        attempt_id: uuid::Uuid,
        mut statements: Vec<GuardedStatement>,
    ) -> Result<DiscardOutcome, StoreFailure> {
        let (_, kind, key) = match self.retired_attempt(attempt_id).await? {
            Ok(found) => found,
            Err(refusal) => return Ok(DiscardOutcome::Refused(refusal)),
        };
        statements.insert(0, delete_retired(attempt_id));
        let source = DiscardSource(statements);
        let key = InstanceKey(key);
        let outcome = match kind {
            EngineKindName::WakeTimer => {
                self.discard_subject::<wake_timer::WakeTimer, _>(
                    &WakeStore(self.0),
                    key,
                    wake_timer::Event::Discard,
                    &source,
                )
                .await
            }
            EngineKindName::AutomationTimer => {
                self.discard_subject::<automation_timer::AutomationTimer, _>(
                    &AutomationStore(self.0),
                    key,
                    automation_timer::Event::Discard,
                    &source,
                )
                .await
            }
            EngineKindName::LegacyNoticeWindows
            | EngineKindName::LegacyWorkspaceDrains
            | EngineKindName::EngineSource => {
                Ok(DiscardOutcome::Refused(AttemptRefusal::DiscardUnsupported))
            }
        }?;
        Ok(outcome)
    }

    /// A subject whose row will not decode cannot run its Discard transition:
    /// the answer is SubjectUnreadable and nothing changes (Retry still works).
    async fn discard_subject<M, Adapter>(
        &self,
        adapter: &Adapter,
        key: InstanceKey,
        event: M::Event,
        source: &DiscardSource,
    ) -> Result<DiscardOutcome, StoreFailure>
    where
        M: Machine,
        M::State: Send + Sync,
        M::Event: Send + Sync,
        M::Effect: Send + Sync,
        M::Error: Send + Sync,
        Adapter: super::store_adapter::StoreAdapter<M>,
    {
        match adapter.load(&key).await {
            Ok(Some(_)) => {}
            Ok(None) => return Ok(DiscardOutcome::Refused(AttemptRefusal::Stale)),
            Err(super::store_adapter::LoadFailure::Corrupt { .. }) => {
                return Ok(DiscardOutcome::Refused(AttemptRefusal::SubjectUnreadable));
            }
            Err(super::store_adapter::LoadFailure::Uncertain) => {
                return Err(StoreFailure::AnswerLost);
            }
            Err(super::store_adapter::LoadFailure::StoreDown(failure)) => return Err(failure),
        }
        discard_outcome(
            self.handle::<M, Adapter, _>(adapter, key, event, source)
                .await,
        )
    }
}

fn discard_outcome<State, Error>(
    outcome: HandleOutcome<State, Error>,
) -> Result<DiscardOutcome, StoreFailure> {
    match outcome {
        HandleOutcome::Done(_) => Ok(DiscardOutcome::Committed),
        // The subject moved, finished, vanished or is unreadable: nothing changed.
        HandleOutcome::Superseded
        | HandleOutcome::Refused(_)
        | HandleOutcome::NotFound
        | HandleOutcome::Failed(_)
        | HandleOutcome::RequestFailed(_) => Ok(DiscardOutcome::Refused(AttemptRefusal::Stale)),
        HandleOutcome::Uncertain => Err(StoreFailure::AnswerLost),
        HandleOutcome::StoreDown(failure) => Err(failure),
    }
}
