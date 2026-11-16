use super::{
    attempt_records::{EngineSubject, StoredAttempt},
    attempt_statements::write_attempt,
    attempt_store::load_attempt,
    effects::{ApplyFailure, classify_batch},
    store_adapter::SubjectVersion,
};
use crate::{ObjectHost, WorkspaceRuntime, WorkspaceStoreCalls};
use central_engine_model::{FailureRecord, ItemFailureClass, RetirementThreshold, after_failure};
use central_storage::{AlarmUpdate, StatementBatch, StoreFailure};
use workspace_model::WorkspaceId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FailureOutcome {
    Recorded(FailureRecord),
    Superseded,
    Uncertain,
    StoreDown(StoreFailure),
}

impl<Host: ObjectHost, Stores: WorkspaceStoreCalls> WorkspaceRuntime<Host, Stores> {
    pub(crate) fn observe_engine_store_down(&self, subject: &EngineSubject, failure: StoreFailure) {
        (self.signin_observer)(crate::CentralObservation::EngineStoreDown {
            kind: subject.kind,
            key: subject.key.clone(),
            failure,
        });
    }

    pub(crate) fn observe_alarm_not_published(&self, failure: StoreFailure) {
        (self.signin_observer)(crate::CentralObservation::EngineAlarmNotPublished { failure });
    }
    pub(crate) async fn record_failure(
        &self,
        subject: &EngineSubject,
        failure_sql: &'static str,
        version: &SubjectVersion,
        class: ItemFailureClass,
        threshold: RetirementThreshold,
    ) -> FailureOutcome {
        self.record_failure_naming(subject, failure_sql, version, class, threshold, &[])
            .await
    }

    /// Records a failure whose observation names the workspaces a notice unit
    /// affected; the stored record keeps only its class (P2, P5 change 4).
    pub(crate) async fn record_failure_naming(
        &self,
        subject: &EngineSubject,
        failure_sql: &'static str,
        version: &SubjectVersion,
        class: ItemFailureClass,
        threshold: RetirementThreshold,
        workspaces: &[WorkspaceId],
    ) -> FailureOutcome {
        let prior = match load_attempt(self.host.as_ref(), subject).await {
            Ok(prior) => prior,
            Err(StoreFailure::AnswerLost) => return FailureOutcome::Uncertain,
            Err(failure) => return FailureOutcome::StoreDown(failure),
        };
        self.write_failure_naming(
            subject,
            failure_sql,
            version,
            class,
            threshold,
            prior.as_ref(),
            workspaces,
        )
        .await
    }

    pub(crate) async fn write_failure(
        &self,
        subject: &EngineSubject,
        failure_sql: &'static str,
        version: &SubjectVersion,
        class: ItemFailureClass,
        threshold: RetirementThreshold,
        prior: Option<&StoredAttempt>,
    ) -> FailureOutcome {
        self.write_failure_naming(subject, failure_sql, version, class, threshold, prior, &[])
            .await
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "one guarded failure write needs the subject, its guard and the record inputs together"
    )]
    async fn write_failure_naming(
        &self,
        subject: &EngineSubject,
        failure_sql: &'static str,
        version: &SubjectVersion,
        class: ItemFailureClass,
        threshold: RetirementThreshold,
        prior: Option<&StoredAttempt>,
        workspaces: &[WorkspaceId],
    ) -> FailureOutcome {
        #[cfg(feature = "test-support")]
        self.pauses
            .reached_engine(crate::PausePoint::BeforeEngineFailureWrite, &subject.key.0)
            .await;
        let (record, class) = match prior.map(|stored| &stored.record) {
            Some(Ok(record)) => (Some(record), class),
            Some(Err(corruption)) => (None, corruption.failure_class()),
            None => (None, class),
        };
        if record.is_some_and(|record| {
            matches!(
                record.state,
                central_engine_model::AttemptState::Retired { .. }
            )
        }) {
            return FailureOutcome::Superseded;
        }
        let now = match self.now() {
            Ok(now) => now,
            Err(failure) => return FailureOutcome::StoreDown(failure),
        };
        let record = match after_failure(record, class, now, threshold, self.ids.new_id()) {
            Ok(record) => record,
            Err(_) => {
                return FailureOutcome::StoreDown(crate::workspace_decode_error::corrupt(
                    "central_work_attempts.failure_update",
                ));
            }
        };
        let statement = match write_attempt(
            failure_sql,
            subject,
            &record,
            prior.map(|stored| &stored.guard),
            version.0.clone(),
        ) {
            Ok(statement) => statement,
            Err(failure) => return FailureOutcome::StoreDown(failure),
        };
        match classify_batch(
            self.host
                .run_batch(StatementBatch {
                    statements: vec![statement],
                    alarm: AlarmUpdate::Unchanged,
                })
                .await,
        ) {
            Ok(()) => {
                (self.signin_observer)(crate::CentralObservation::EngineItemFailed {
                    kind: subject.kind,
                    key: subject.key.clone(),
                    failure: record.last_failure.clone(),
                    count: record.failure_count.get(),
                    retired: matches!(
                        record.state,
                        central_engine_model::AttemptState::Retired { .. }
                    ),
                    workspaces: workspaces.to_vec(),
                });
                FailureOutcome::Recorded(record)
            }
            Err(ApplyFailure::Superseded) => FailureOutcome::Superseded,
            Err(ApplyFailure::Uncertain) => FailureOutcome::Uncertain,
            Err(ApplyFailure::StoreDown(failure)) => FailureOutcome::StoreDown(failure),
            // A rejected attempt write cannot charge another attempt or recurse.
            Err(ApplyFailure::Rejected(_)) => FailureOutcome::StoreDown(
                crate::workspace_decode_error::corrupt("central_work_attempts.write"),
            ),
        }
    }
}
