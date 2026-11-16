use super::attempt_records::EngineSubject;
use crate::workspace_decode_error::corrupt;
use central_engine_model::{InstanceKey, ItemFailureClass, Machine};
use central_storage::{CorruptReason, GuardedStatement, ReadOutcome, SqlValue, StoreFailure};
use std::future::Future;

/// Raw comparison values survive a decode failure, so recording it still checks
/// the exact subject that was read. The adapter owns their SQL interpretation.
#[derive(Clone, Debug)]
pub(crate) struct SubjectVersion(pub Vec<SqlValue>);

pub(crate) struct Versioned<State> {
    pub state: State,
    pub version: SubjectVersion,
}

pub(crate) enum LoadFailure {
    Corrupt {
        class: ItemFailureClass,
        version: SubjectVersion,
    },
    Uncertain,
    StoreDown(StoreFailure),
}

/// One machine's row codec and static statements. Reducers never see SQL.
pub(crate) trait StoreAdapter<M: Machine>: Sync {
    fn load(
        &self,
        key: &InstanceKey,
    ) -> impl Future<Output = Result<Option<Versioned<M::State>>, LoadFailure>> + Send;
    fn save(
        &self,
        key: &InstanceKey,
        prior: &Versioned<M::State>,
        next: &M::State,
    ) -> Result<GuardedStatement, StoreFailure>;
    fn effect_statements(
        &self,
        key: &InstanceKey,
        effect: &M::Effect,
    ) -> Result<Vec<GuardedStatement>, StoreFailure>;
    fn failure_sql(&self) -> &'static str;
    fn subject(&self, key: &InstanceKey) -> EngineSubject {
        EngineSubject {
            kind: M::KIND,
            key: key.clone(),
        }
    }
}

pub(crate) fn corrupt_class(reason: CorruptReason) -> ItemFailureClass {
    let (CorruptReason::MissingColumn { column }
    | CorruptReason::UnknownKind { column }
    | CorruptReason::BadValue { column }) = reason;
    ItemFailureClass::CorruptRow {
        column,
        raw_value: None,
    }
}

pub(crate) fn load_store_failure(failure: StoreFailure) -> LoadFailure {
    match failure {
        StoreFailure::AnswerLost => LoadFailure::Uncertain,
        failure @ (StoreFailure::Unavailable
        | StoreFailure::Corrupt { .. }
        | StoreFailure::AlarmUnpublishable { .. }) => LoadFailure::StoreDown(failure),
    }
}
pub(crate) fn decode_load_failure(failure: StoreFailure, version: SubjectVersion) -> LoadFailure {
    match failure {
        StoreFailure::Corrupt { reason } => LoadFailure::Corrupt {
            class: corrupt_class(reason),
            version,
        },
        failure @ (StoreFailure::Unavailable
        | StoreFailure::AnswerLost
        | StoreFailure::AlarmUnpublishable { .. }) => load_store_failure(failure),
    }
}
pub(crate) fn raw_version(rows: &ReadOutcome) -> Result<Option<SubjectVersion>, LoadFailure> {
    match rows.rows.as_slice() {
        [] => Ok(None),
        [row] => Ok(Some(SubjectVersion(row.clone()))),
        _ => Err(load_store_failure(corrupt("engine.subject_key"))),
    }
}
