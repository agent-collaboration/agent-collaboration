//! The step vocabulary a workerd case sends, mapped onto core's storage conformance
//! statements, so the SQL lives in core and the cases only compose it.

use central_storage::{
    AlarmUpdate, RowExpectation, SafeInteger, SqlValue, Statement, StatementBatch,
};
use collaboration_central_core::PausePoint;
use collaboration_central_core::storage_conformance as conformance;
use serde::{Deserialize, Serialize};
use sqlx_core::migrate::Migrator;

/// Which test-only migration source the case object opens with.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CaseMigrator {
    Widgets,
    FailingSecondStatement,
}

impl CaseMigrator {
    pub(crate) fn migrator(self) -> Migrator {
        match self {
            Self::Widgets => conformance::widgets_migrator(),
            Self::FailingSecondStatement => conformance::failing_second_statement_migrator(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum CaseStep {
    #[serde(rename_all = "camelCase")]
    InsertOwner { owner_id: String },
    #[serde(rename_all = "camelCase")]
    InsertWidget {
        widget_id: String,
        owner_id: String,
        name: SqlValue,
        active: SqlValue,
        /// Refused at decoding when outside ±(2^53−1).
        due_at_ms: Option<SafeInteger>,
    },
    #[serde(rename_all = "camelCase")]
    RetireLiveWidget { widget_id: String },
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CaseAlarm {
    Unchanged,
    EarliestWidgetDue,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CaseBatch {
    pub(crate) steps: Vec<CaseStep>,
    pub(crate) alarm: CaseAlarm,
}

impl CaseBatch {
    pub(crate) fn into_batch(self) -> StatementBatch {
        StatementBatch {
            statements: self
                .steps
                .into_iter()
                .map(|step| match step {
                    CaseStep::InsertOwner { owner_id } => {
                        conformance::insert_owner(&owner_id, RowExpectation::Exactly(1))
                    }
                    CaseStep::InsertWidget {
                        widget_id,
                        owner_id,
                        name,
                        active,
                        due_at_ms,
                    } => conformance::insert_widget(&widget_id, &owner_id, name, active, due_at_ms),
                    CaseStep::RetireLiveWidget { widget_id } => {
                        conformance::retire_live_widget(&widget_id)
                    }
                })
                .collect(),
            alarm: match self.alarm {
                CaseAlarm::Unchanged => AlarmUpdate::Unchanged,
                CaseAlarm::EarliestWidgetDue => conformance::alarm_from_earliest_widget_due(),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CaseRead {
    ListWidgets,
    CountOwners,
    ListTables,
    ListAppliedMigrations,
}

impl CaseRead {
    pub(crate) fn statement(self) -> Statement {
        match self {
            Self::ListWidgets => conformance::list_widgets(),
            Self::CountOwners => conformance::count_owners(),
            Self::ListTables => conformance::list_tables(),
            Self::ListAppliedMigrations => conformance::list_applied_migrations(),
        }
    }
}

/// A store call a case sends through `CentralPlatform::call_store`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum CaseStoreCall {
    Batch(CaseBatch),
    Read(CaseRead),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CasePausePoint {
    AfterReads,
    AfterStoreApplied,
    AfterNoticesCommitted,
    BeforeDrainCommit,
    AfterDrainCommit,
    BeforeTimerCommit,
    AfterTimerCommit,
}

impl From<CasePausePoint> for PausePoint {
    fn from(point: CasePausePoint) -> Self {
        match point {
            CasePausePoint::AfterReads => Self::AfterReads,
            CasePausePoint::AfterStoreApplied => Self::AfterStoreApplied,
            CasePausePoint::AfterNoticesCommitted => Self::AfterNoticesCommitted,
            CasePausePoint::BeforeDrainCommit => Self::BeforeDrainCommit,
            CasePausePoint::AfterDrainCommit => Self::AfterDrainCommit,
            CasePausePoint::BeforeTimerCommit => Self::BeforeTimerCommit,
            CasePausePoint::AfterTimerCommit => Self::AfterTimerCommit,
        }
    }
}
impl From<PausePoint> for CasePausePoint {
    fn from(point: PausePoint) -> Self {
        match point {
            PausePoint::AfterReads => Self::AfterReads,
            PausePoint::AfterStoreApplied => Self::AfterStoreApplied,
            PausePoint::AfterNoticesCommitted => Self::AfterNoticesCommitted,
            PausePoint::BeforeDrainCommit => Self::BeforeDrainCommit,
            PausePoint::AfterDrainCommit => Self::AfterDrainCommit,
            PausePoint::BeforeTimerCommit => Self::BeforeTimerCommit,
            PausePoint::AfterTimerCommit => Self::AfterTimerCommit,
        }
    }
}
