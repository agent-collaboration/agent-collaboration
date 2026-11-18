use central_engine_model::{
    AttemptState, EngineTime, FailureRecord, InstanceKey, ItemFailureClass, MachineKind,
    supported_time,
};
use central_storage::{ConstraintKind, ReadOutcome, SqlValue, StoreFailure};
use fleet_runtime::stored_values::column;
use fleet_runtime::workspace_decode_error::corrupt;
use std::num::NonZeroU32;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EngineSubject {
    pub kind: MachineKind,
    pub key: InstanceKey,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AttemptGuard {
    pub attempt_id: SqlValue,
    pub state: SqlValue,
    pub failure_count: SqlValue,
    pub last_failure: SqlValue,
    pub updated_at: SqlValue,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("central attempt holds an invalid value in {column}")]
pub(crate) struct AttemptCorruption {
    pub column: String,
    pub raw_value: String,
}

impl AttemptCorruption {
    pub fn failure_class(&self) -> ItemFailureClass {
        ItemFailureClass::CorruptRow {
            column: self.column.clone(),
            raw_value: Some(self.raw_value.clone()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StoredAttempt {
    pub guard: AttemptGuard,
    pub record: Result<FailureRecord, AttemptCorruption>,
}

fn corrupt_attempt_field(name: &str, raw: Option<&SqlValue>) -> AttemptCorruption {
    let raw_value = match raw {
        Some(SqlValue::Text(value)) => value.clone(),
        Some(value) => format!("{value:?}"),
        None => "<missing column>".into(),
    };
    AttemptCorruption {
        column: format!("central_work_attempts.{name}"),
        raw_value,
    }
}

struct AttemptRow<'a> {
    outcome: &'a ReadOutcome,
    values: &'a [SqlValue],
}
impl AttemptRow<'_> {
    fn value(&self, name: &str) -> Result<&SqlValue, AttemptCorruption> {
        column(self.outcome, self.values, name).map_err(|_| corrupt_attempt_field(name, None))
    }
    fn text(&self, name: &str) -> Result<&str, AttemptCorruption> {
        match self.value(name)? {
            SqlValue::Text(value) => Ok(value),
            value => Err(corrupt_attempt_field(name, Some(value))),
        }
    }
    fn optional_text(&self, name: &str) -> Result<Option<&str>, AttemptCorruption> {
        match self.value(name)? {
            SqlValue::Null => Ok(None),
            SqlValue::Text(value) => Ok(Some(value)),
            value => Err(corrupt_attempt_field(name, Some(value))),
        }
    }
    fn time(&self, name: &str) -> Result<EngineTime, AttemptCorruption> {
        self.text(name)?
            .parse::<EngineTime>()
            .ok()
            .filter(|time| supported_time(*time))
            .ok_or_else(|| corrupt_attempt_field(name, self.value(name).ok()))
    }
    fn optional_time(&self, name: &str) -> Result<Option<EngineTime>, AttemptCorruption> {
        match self.value(name)? {
            SqlValue::Null => Ok(None),
            _ => self.time(name).map(Some),
        }
    }
    fn failure_class(&self) -> Result<ItemFailureClass, AttemptCorruption> {
        let detail = self.optional_text("last_failure_detail")?;
        let bad_detail = || {
            corrupt_attempt_field(
                "last_failure_detail",
                self.value("last_failure_detail").ok(),
            )
        };
        match self.text("last_failure")? {
            "corrupt_row" => {
                let detail = detail
                    .filter(|value| !value.is_empty())
                    .ok_or_else(bad_detail)?;
                let (column, raw_value) = detail
                    .split_once('\n')
                    .map_or((detail, None), |(column, raw)| {
                        (column, Some(raw.to_owned()))
                    });
                if column.is_empty() {
                    return Err(bad_detail());
                }
                Ok(ItemFailureClass::CorruptRow {
                    column: column.to_owned(),
                    raw_value,
                })
            }
            "constraint_violated" => {
                let constraint = match detail {
                    Some("unique") => ConstraintKind::Unique,
                    Some("foreign_key") => ConstraintKind::ForeignKey,
                    Some("not_null") => ConstraintKind::NotNull,
                    Some("check_boolean") => ConstraintKind::CheckBoolean,
                    _ => return Err(bad_detail()),
                };
                Ok(ItemFailureClass::ConstraintViolated { constraint })
            }
            "statement_rejected" if detail.is_none() => Ok(ItemFailureClass::StatementRejected),
            "other_store_unavailable" if detail.is_none() => {
                Ok(ItemFailureClass::OtherStoreUnavailable)
            }
            "refused" => Ok(ItemFailureClass::Refused {
                refusal: detail
                    .filter(|value| !value.is_empty())
                    .ok_or_else(bad_detail)?
                    .to_owned(),
            }),
            "statement_rejected" | "other_store_unavailable" => Err(bad_detail()),
            _ => Err(corrupt_attempt_field(
                "last_failure",
                self.value("last_failure").ok(),
            )),
        }
    }
    fn decode(&self) -> Result<FailureRecord, AttemptCorruption> {
        let attempt_id = self
            .text("attempt_id")?
            .parse::<uuid::Uuid>()
            .ok()
            .filter(|id| id.get_version_num() == 7)
            .ok_or_else(|| corrupt_attempt_field("attempt_id", self.value("attempt_id").ok()))?;
        let failure_count = match self.value("failure_count")? {
            SqlValue::Integer(value) => u32::try_from(value.get())
                .ok()
                .and_then(NonZeroU32::new)
                .ok_or_else(|| {
                corrupt_attempt_field("failure_count", self.value("failure_count").ok())
            })?,
            value => return Err(corrupt_attempt_field("failure_count", Some(value))),
        };
        let next_try = self.optional_time("next_try_at")?;
        let retired = self.optional_time("retired_at")?;
        let state = match self.text("state")? {
            "failing" => {
                if retired.is_some() {
                    return Err(corrupt_attempt_field(
                        "retired_at",
                        self.value("retired_at").ok(),
                    ));
                }
                AttemptState::Failing {
                    next_try_at: next_try.ok_or_else(|| {
                        corrupt_attempt_field("next_try_at", self.value("next_try_at").ok())
                    })?,
                }
            }
            "retired" => {
                if next_try.is_some() {
                    return Err(corrupt_attempt_field(
                        "next_try_at",
                        self.value("next_try_at").ok(),
                    ));
                }
                AttemptState::Retired {
                    retired_at: retired.ok_or_else(|| {
                        corrupt_attempt_field("retired_at", self.value("retired_at").ok())
                    })?,
                }
            }
            _ => return Err(corrupt_attempt_field("state", self.value("state").ok())),
        };
        Ok(FailureRecord {
            attempt_id,
            failure_count,
            state,
            last_failure: self.failure_class()?,
            first_failed_at: self.time("first_failed_at")?,
            last_failed_at: self.time("last_failed_at")?,
            created_at: self.time("created_at")?,
            updated_at: self.time("updated_at")?,
        })
    }
}

pub(crate) fn decode_attempt(outcome: &ReadOutcome) -> Result<Option<StoredAttempt>, StoreFailure> {
    let values = match outcome.rows.as_slice() {
        [] => return Ok(None),
        [row] => row,
        _ => return Err(corrupt("central_work_attempts.subject")),
    };
    let get = |name| column(outcome, values, name).cloned();
    let guard = AttemptGuard {
        attempt_id: get("attempt_id")?,
        state: get("state")?,
        failure_count: get("failure_count")?,
        last_failure: get("last_failure")?,
        updated_at: get("updated_at")?,
    };
    Ok(Some(StoredAttempt {
        guard,
        record: AttemptRow { outcome, values }.decode(),
    }))
}

pub(crate) fn failure_detail(failure: &ItemFailureClass) -> Option<String> {
    match failure {
        ItemFailureClass::CorruptRow { column, raw_value } => Some(
            raw_value
                .as_ref()
                .map_or_else(|| column.clone(), |raw| format!("{column}\n{raw}")),
        ),
        ItemFailureClass::ConstraintViolated { constraint } => Some(
            match constraint {
                ConstraintKind::Unique => "unique",
                ConstraintKind::ForeignKey => "foreign_key",
                ConstraintKind::NotNull => "not_null",
                ConstraintKind::CheckBoolean => "check_boolean",
            }
            .into(),
        ),
        ItemFailureClass::Refused { refusal } => Some(refusal.clone()),
        ItemFailureClass::StatementRejected | ItemFailureClass::OtherStoreUnavailable => None,
    }
}

#[cfg(test)]
#[path = "attempt_records_test.rs"]
mod tests;
