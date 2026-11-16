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
use super::{
    machines::{
        automation_timer,
        timer_store::{AutomationStore, WakeStore},
        wake_timer,
    },
    port::{Engine, EngineHost},
};
use crate::ObjectHost as _;
use crate::{
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

/// What one due batch did, counted per unit kind.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AlarmResult {
    pub wakes_fired: usize,
    pub automations_fired: usize,
    pub windows_fired: usize,
    pub workspaces_drained: usize,
}

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
    #[cfg(feature = "test-support")]
    Probe,
}
impl DueKind {
    fn kind(self) -> MachineKind {
        match self {
            Self::Wake => MachineKind("wake_timer"),
            Self::Automation => MachineKind("automation_timer"),
            #[cfg(feature = "test-support")]
            Self::Probe => MachineKind("engine_probe"),
        }
    }
    fn sql(self) -> &'static str {
        match self {
            Self::Wake => query_registry::WAKE_LIST,
            Self::Automation => query_registry::AUTOMATION_LIST,
            #[cfg(feature = "test-support")]
            Self::Probe => query_registry::PROBE_LIST,
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

impl<R: EngineHost> Engine<'_, R> {
    /// Every event reaches this after its own work. The claim occurs before the
    /// first await; only individual units perform async work, at most eight wide.
    pub(crate) async fn after_event(&self) -> Result<EventCompletion, AlarmNotPublished> {
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
            .due_batches()
            .try_claim(now.timestamp_millis(), DueBatchPolicy::DEFAULT.spacing)
            .map_err(|reason| AlarmNotPublished {
                failure: StoreFailure::AlarmUnpublishable { reason },
            })?;
        let provisioned = self
            .0
            .organization_is_provisioned()
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
            .host()
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
        for kind in [
            DueKind::Wake,
            DueKind::Automation,
            #[cfg(feature = "test-support")]
            DueKind::Probe,
        ] {
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
            match self.host().read(query).await.and_then(decode_due) {
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
        self.due_batches().finish(now.timestamp_millis(), leftovers);
        work
    }

    async fn run_engine_unit(&self, unit: Unit, now: EngineTime) -> (AlarmResult, bool) {
        let mut work = AlarmResult::default();
        match unit {
            Unit::Machine(kind, row) => {
                let (done, down) = match kind {
                    #[cfg(feature = "test-support")]
                    DueKind::Probe => {
                        self.run_machine_unit::<crate::engine_probe::EngineProbe, _>(
                            &super::probe_store::ProbeStore(self.0),
                            row,
                            "engine_probe_items.due_at",
                            |occurrence| crate::engine_probe::ProbeEvent::Due { occurrence },
                        )
                        .await
                    }
                    DueKind::Wake => {
                        self.run_machine_unit::<wake_timer::WakeTimer, _>(
                            &WakeStore(self.0),
                            row,
                            "wakes.due_at",
                            |occurrence| wake_timer::Event::Due { occurrence },
                        )
                        .await
                    }
                    DueKind::Automation => {
                        self.run_machine_unit::<automation_timer::AutomationTimer, _>(
                            &AutomationStore(self.0),
                            row,
                            "automations.next_due_at",
                            |occurrence| automation_timer::Event::Due { occurrence },
                        )
                        .await
                    }
                };
                match kind {
                    DueKind::Wake => work.wakes_fired = usize::from(done),
                    DueKind::Automation => work.automations_fired = usize::from(done),
                    #[cfg(feature = "test-support")]
                    DueKind::Probe => {}
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
                #[cfg(feature = "test-support")]
                self.0.count_legacy_invocation(&|count| {
                    if is_windows {
                        count.windows += 1;
                    } else {
                        count.drains += 1;
                    }
                });
                let result = if is_windows {
                    self.run_window_unit(now, &subject).await
                } else {
                    self.0.drain_due_workspaces(now).await
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
                                self.record_failure_naming(
                                    &subject,
                                    CONTROL_WRITE,
                                    &SubjectVersion(vec![]),
                                    class,
                                    RetirementThreshold::NOTICE_UNITS,
                                    outcome.affected_workspaces(),
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

    /// One due machine instance as its own unit: `(done, central_down)`.
    async fn run_machine_unit<M, Adapter>(
        &self,
        adapter: &Adapter,
        row: DueRow,
        due_column: &'static str,
        due_event: fn(EngineTime) -> M::Event,
    ) -> (bool, bool)
    where
        M: central_engine_model::Machine,
        M::State: Send + Sync,
        M::Event: Send + Sync,
        M::Effect: Send + Sync,
        M::Error: Send + Sync,
        Adapter: super::store_adapter::StoreAdapter<M>,
    {
        let subject = adapter.subject(&row.key);
        let outcome = match EngineTime::from_timestamp_millis(row.occurrence_ms) {
            Some(occurrence) => {
                self.handle::<M, Adapter, _>(
                    adapter,
                    row.key.clone(),
                    due_event(occurrence),
                    &super::source::DueSource,
                )
                .await
            }
            // The due SQL clamps unreadable times, so this is defensive: the item
            // fails alone as CorruptRow and the batch continues (P3).
            None => {
                self.record_unrepresentable_due(adapter, &row.key, due_column)
                    .await
            }
        };
        match outcome {
            HandleOutcome::Done(_) => (true, false),
            HandleOutcome::StoreDown(failure) => {
                self.observe_engine_store_down(&subject, failure);
                (false, true)
            }
            HandleOutcome::Failed(_)
            | HandleOutcome::RequestFailed(_)
            | HandleOutcome::NotFound
            | HandleOutcome::Refused(_)
            | HandleOutcome::Superseded
            | HandleOutcome::Uncertain => (false, false),
        }
    }

    async fn record_unrepresentable_due<M, Adapter>(
        &self,
        adapter: &Adapter,
        key: &InstanceKey,
        due_column: &'static str,
    ) -> HandleOutcome<M::State, M::Error>
    where
        M: central_engine_model::Machine,
        Adapter: super::store_adapter::StoreAdapter<M>,
    {
        let version = match adapter.load(key).await {
            Ok(Some(loaded)) => loaded.version,
            Ok(None) => return HandleOutcome::NotFound,
            Err(super::store_adapter::LoadFailure::Corrupt { version, .. }) => version,
            Err(super::store_adapter::LoadFailure::Uncertain) => return HandleOutcome::Uncertain,
            Err(super::store_adapter::LoadFailure::StoreDown(failure)) => {
                return HandleOutcome::StoreDown(failure);
            }
        };
        self.record_failure(
            &adapter.subject(key),
            adapter.failure_sql(),
            &version,
            central_engine_model::ItemFailureClass::CorruptRow {
                column: due_column.into(),
                raw_value: None,
            },
            M::retirement_threshold(),
        )
        .await
        .into()
    }

    async fn run_window_unit(
        &self,
        now: EngineTime,
        subject: &EngineSubject,
    ) -> Result<usize, LegacyUnitOutcome> {
        let _turn = self.0.central_turn().await;
        let (mut statements, count) = self.0.plan_due_windows(now).await?;
        statements.push(super::attempt_statements::delete_attempt(subject));
        // The kept notice tests hold the commit that delivers windows at the
        // existing timer-commit points; an empty window unit passes them by.
        #[cfg(feature = "test-support")]
        if count > 0 {
            self.pauses()
                .reached_central(crate::PausePoint::BeforeTimerCommit)
                .await;
        }
        LegacyUnitOutcome::batch(
            self.host()
                .run_batch(StatementBatch {
                    statements,
                    alarm: AlarmUpdate::Unchanged,
                })
                .await,
        )?;
        #[cfg(feature = "test-support")]
        if count > 0 {
            self.pauses()
                .reached_central(crate::PausePoint::AfterTimerCommit)
                .await;
        }
        Ok(count)
    }
}
