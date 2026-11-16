use super::{
    attempt_records::EngineSubject,
    attempt_statements::CONTROL_WRITE,
    control_gate::ControlGate,
    effects::ApplyFailure,
    failures::FailureOutcome,
    handle::HandleOutcome,
    legacy_units::LegacyUnitOutcome,
    query_registry,
    store_adapter::{SubjectVersion, corrupt_class},
};
use crate::{
    AlarmResult, ObjectHost, WorkspaceRuntime, WorkspaceStoreCalls,
    engine_machines::{
        automation_timer,
        timer_store::{AutomationStore, WakeStore},
        wake_timer,
    },
    workspace_decode_error::corrupt,
    workspace_row_codec::{column, string},
};
use central_engine_model::{
    DueBatchPolicy, DueRow, EngineTime, InstanceKey, MachineKind, RetirementThreshold,
};
use central_storage::{
    AlarmPublication, AlarmUpdate, BatchOutcome, SqlValue, Statement, StatementBatch, StoreFailure,
};
use futures_util::{StreamExt, stream};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EventCompletion {
    pub work: AlarmResult,
    pub publication: AlarmPublication,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("the event's alarm was not published: {failure}")]
pub struct AlarmNotPublished {
    pub failure: StoreFailure,
}

#[derive(Clone, Copy)]
enum DueKind {
    Wake,
    Automation,
}
impl DueKind {
    fn kind(self) -> MachineKind {
        match self {
            Self::Wake => MachineKind("wake_timer"),
            Self::Automation => MachineKind("automation_timer"),
        }
    }
    fn sql(self) -> &'static str {
        match self {
            Self::Wake => query_registry::WAKE_LIST,
            Self::Automation => query_registry::AUTOMATION_LIST,
        }
    }
}
enum Unit {
    Machine(DueKind, DueRow),
    Windows,
    Drains,
}

fn source(kind: MachineKind) -> EngineSubject {
    EngineSubject {
        kind: MachineKind("engine_source"),
        key: InstanceKey(kind.0.into()),
    }
}
fn legacy(kind: &'static str) -> EngineSubject {
    EngineSubject {
        kind: MachineKind(kind),
        key: InstanceKey("all".into()),
    }
}
fn decode_due(rows: central_storage::ReadOutcome) -> Result<Vec<DueRow>, StoreFailure> {
    rows.rows
        .iter()
        .map(|row| {
            let integer = |name| match column(&rows, row, name)? {
                SqlValue::Integer(value) => Ok(value.get()),
                _ => Err(corrupt(name)),
            };
            Ok(DueRow {
                key: InstanceKey(string(&rows, row, "subject_key")?.to_owned()),
                occurrence_ms: integer("occurrence_ms")?,
                effective_due_ms: integer("effective_due_ms")?,
            })
        })
        .collect()
}

impl<Host: ObjectHost, Stores: WorkspaceStoreCalls> WorkspaceRuntime<Host, Stores> {
    /// Every event reaches this after its own work. The claim occurs before the
    /// first await; only individual units perform async work, at most eight wide.
    pub async fn after_event(&self) -> Result<EventCompletion, AlarmNotPublished> {
        let result = self.complete_event().await;
        if let Err(error) = &result {
            self.observe_alarm_not_published(error.failure.clone());
        }
        result
    }

    async fn complete_event(&self) -> Result<EventCompletion, AlarmNotPublished> {
        let now = self
            .now()
            .map_err(|failure| AlarmNotPublished { failure })?;
        let claimed = self
            .due_batches
            .try_claim(now.timestamp_millis(), DueBatchPolicy::DEFAULT.spacing)
            .map_err(|reason| AlarmNotPublished {
                failure: StoreFailure::AlarmUnpublishable { reason },
            })?;
        let provisioned = self
            .alarm_organization_is_provisioned()
            .await
            .map_err(|failure| AlarmNotPublished { failure })?;
        if !provisioned {
            let publication = self
                .publish_engine_query(Statement {
                    sql: "SELECT NULL,NULL",
                    args: vec![],
                })
                .await?;
            return Ok(EventCompletion {
                work: AlarmResult::default(),
                publication,
            });
        }
        let mut work = AlarmResult::default();
        if claimed {
            work = self.run_due_batch(now).await;
        }
        let publication = self
            .publish_engine_query(central_engine_model::next_due_sql(
                query_registry::ALARM_INPUTS,
            ))
            .await?;
        Ok(EventCompletion { work, publication })
    }

    pub(crate) async fn publish_engine_query(
        &self,
        query: Statement,
    ) -> Result<AlarmPublication, AlarmNotPublished> {
        match self
            .host
            .run_batch(StatementBatch {
                statements: vec![],
                alarm: AlarmUpdate::SetEngineFromQuery { query },
            })
            .await
        {
            Ok(BatchOutcome::Applied { publication, .. }) => match publication {
                AlarmPublication::Set { .. } | AlarmPublication::Cleared => Ok(publication),
                AlarmPublication::Failed { reason } => Err(AlarmNotPublished {
                    failure: StoreFailure::AlarmUnpublishable { reason },
                }),
                AlarmPublication::Unchanged => Err(AlarmNotPublished {
                    failure: corrupt("engine.alarm_publication"),
                }),
            },
            Ok(
                BatchOutcome::GuardFailed { .. }
                | BatchOutcome::ConstraintViolated { .. }
                | BatchOutcome::StatementRejected { .. },
            ) => Err(AlarmNotPublished {
                failure: corrupt("engine.alarm_batch"),
            }),
            Err(failure) => Err(AlarmNotPublished { failure }),
        }
    }

    async fn run_due_batch(&self, now: EngineTime) -> AlarmResult {
        let policy = DueBatchPolicy::DEFAULT;
        let Some(horizon) = now
            .checked_add_signed(policy.look_ahead)
            .filter(|time| central_engine_model::supported_time(*time))
        else {
            return AlarmResult::default();
        };
        let mut units = Vec::new();
        let mut leftovers = false;
        let mut central_down = false;
        for kind in [DueKind::Wake, DueKind::Automation] {
            let subject = source(kind.kind());
            match self
                .control_gate(&subject, RetirementThreshold::DEFAULT)
                .await
            {
                ControlGate::Run => {}
                ControlGate::Skip | ControlGate::Uncertain => continue,
                ControlGate::CentralDown(failure) => {
                    self.observe_engine_store_down(&subject, failure);
                    central_down = true;
                    break;
                }
            }
            let query = match central_engine_model::due_list_sql(
                kind.sql(),
                horizon,
                policy.per_kind_cap + 1,
            ) {
                Ok(query) => query,
                Err(_) => {
                    central_down = true;
                    break;
                }
            };
            match self.host.read(query).await.and_then(decode_due) {
                Ok(mut rows) => {
                    leftovers |= rows.len() > policy.per_kind_cap;
                    rows.truncate(policy.per_kind_cap);
                    units.extend(rows.into_iter().map(|row| Unit::Machine(kind, row)));
                    match self.clear_control(&subject).await {
                        Ok(()) | Err(ApplyFailure::Superseded | ApplyFailure::Uncertain) => {}
                        Err(ApplyFailure::StoreDown(_) | ApplyFailure::Rejected(_)) => {
                            central_down = true;
                            break;
                        }
                    }
                }
                Err(StoreFailure::AnswerLost) => {}
                Err(StoreFailure::Corrupt { reason }) => {
                    if matches!(
                        self.record_failure(
                            &subject,
                            CONTROL_WRITE,
                            &SubjectVersion(vec![]),
                            corrupt_class(reason),
                            RetirementThreshold::DEFAULT
                        )
                        .await,
                        FailureOutcome::StoreDown(_)
                    ) {
                        central_down = true;
                        break;
                    }
                }
                Err(
                    failure @ (StoreFailure::Unavailable | StoreFailure::AlarmUnpublishable { .. }),
                ) => {
                    self.observe_engine_store_down(&subject, failure);
                    central_down = true;
                    break;
                }
            }
        }
        let mut work = AlarmResult::default();
        if !central_down {
            units.extend([Unit::Windows, Unit::Drains]);
            let stop = AtomicBool::new(false);
            let mut completed = stream::iter(units)
                .map(|unit| {
                    let stop = &stop;
                    async move {
                        if stop.load(Ordering::Acquire) {
                            return AlarmResult::default();
                        }
                        let (work, down) = self.run_engine_unit(unit, now).await;
                        if down {
                            stop.store(true, Ordering::Release);
                        }
                        work
                    }
                })
                .buffer_unordered(8);
            while let Some(done) = completed.next().await {
                work.wakes_fired += done.wakes_fired;
                work.automations_fired += done.automations_fired;
                work.windows_fired += done.windows_fired;
                work.workspaces_drained += done.workspaces_drained;
            }
        }
        self.due_batches.finish(now.timestamp_millis(), leftovers);
        work
    }

    async fn run_engine_unit(&self, unit: Unit, now: EngineTime) -> (AlarmResult, bool) {
        let mut work = AlarmResult::default();
        match unit {
            Unit::Machine(kind, row) => {
                let Some(occurrence) = EngineTime::from_timestamp_millis(row.occurrence_ms) else {
                    return (work, true);
                };
                let (done, down) = match kind {
                    DueKind::Wake => match self
                        .handle::<wake_timer::WakeTimer, _>(
                            &WakeStore(self),
                            row.key.clone(),
                            wake_timer::Event::Due { occurrence },
                        )
                        .await
                    {
                        HandleOutcome::Done(_) => (true, false),
                        HandleOutcome::StoreDown(failure) => {
                            self.observe_engine_store_down(
                                &EngineSubject {
                                    kind: kind.kind(),
                                    key: row.key.clone(),
                                },
                                failure,
                            );
                            (false, true)
                        }
                        HandleOutcome::Failed
                        | HandleOutcome::Superseded
                        | HandleOutcome::Uncertain => (false, false),
                    },
                    DueKind::Automation => match self
                        .handle::<automation_timer::AutomationTimer, _>(
                            &AutomationStore(self),
                            row.key.clone(),
                            automation_timer::Event::Due { occurrence },
                        )
                        .await
                    {
                        HandleOutcome::Done(_) => (true, false),
                        HandleOutcome::StoreDown(failure) => {
                            self.observe_engine_store_down(
                                &EngineSubject {
                                    kind: kind.kind(),
                                    key: row.key.clone(),
                                },
                                failure,
                            );
                            (false, true)
                        }
                        HandleOutcome::Failed
                        | HandleOutcome::Superseded
                        | HandleOutcome::Uncertain => (false, false),
                    },
                };
                match kind {
                    DueKind::Wake => work.wakes_fired = usize::from(done),
                    DueKind::Automation => work.automations_fired = usize::from(done),
                }
                (work, down)
            }
            Unit::Windows | Unit::Drains => {
                let is_windows = matches!(unit, Unit::Windows);
                let subject = legacy(if is_windows {
                    "legacy_notice_windows"
                } else {
                    "legacy_workspace_drains"
                });
                match self
                    .control_gate(&subject, RetirementThreshold::NOTICE_UNITS)
                    .await
                {
                    ControlGate::Run => {}
                    ControlGate::Skip | ControlGate::Uncertain => return (work, false),
                    ControlGate::CentralDown(failure) => {
                        self.observe_engine_store_down(&subject, failure);
                        return (work, true);
                    }
                }
                let result = if is_windows {
                    self.run_window_unit(now, &subject).await
                } else {
                    self.drain_due_workspaces(now).await
                };
                match result {
                    Ok(count) => {
                        match self.clear_control(&subject).await {
                            Err(ApplyFailure::StoreDown(_) | ApplyFailure::Rejected(_)) => {
                                return (work, true);
                            }
                            Ok(()) | Err(ApplyFailure::Superseded | ApplyFailure::Uncertain) => {}
                        }
                        if is_windows {
                            work.windows_fired = count;
                        } else {
                            work.workspaces_drained = count;
                        }
                        (work, false)
                    }
                    Err(LegacyUnitOutcome::CentralDown(failure)) => {
                        self.observe_engine_store_down(&subject, failure);
                        (work, true)
                    }
                    Err(outcome) => match outcome.failure_class() {
                        Some(class) => (
                            work,
                            matches!(
                                self.record_failure(
                                    &subject,
                                    CONTROL_WRITE,
                                    &SubjectVersion(vec![]),
                                    class,
                                    RetirementThreshold::NOTICE_UNITS
                                )
                                .await,
                                FailureOutcome::StoreDown(_)
                            ),
                        ),
                        None => (work, false),
                    },
                }
            }
        }
    }

    async fn run_window_unit(
        &self,
        now: EngineTime,
        subject: &EngineSubject,
    ) -> Result<usize, LegacyUnitOutcome> {
        let _turn = self.take_central_turn().await;
        let (mut statements, count) = self.plan_due_windows(now).await?;
        statements.push(super::attempt_statements::delete_attempt(subject));
        LegacyUnitOutcome::batch(
            self.host
                .run_batch(StatementBatch {
                    statements,
                    alarm: AlarmUpdate::Unchanged,
                })
                .await,
        )?;
        Ok(count)
    }
}
