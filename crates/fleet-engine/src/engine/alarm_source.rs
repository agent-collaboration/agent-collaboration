use super::{
    attempt_statements::CONTROL_WRITE,
    effects::ApplyFailure,
    handle::HandleOutcome,
    port::{Engine, EngineHost},
    store_adapter::corrupt_class,
};
use central_engine_model::{
    DueBatchPolicy, DueRow, EngineTime, InstanceKey, MachineKind, RetirementThreshold,
};
use central_storage::{
    AlarmPublication, AlarmUpdate, BatchOutcome, SqlValue, Statement, StatementBatch, StoreFailure,
};
use fleet_contracts::{
    AlarmNotPublished, AlarmResult, ControlGate, EngineSubject, EventCompletion, FailureOutcome,
    LegacyUnitOutcome, SubjectVersion,
};
use fleet_runtime_ports::CentralStore as _;
use fleet_runtime_ports::{
    stored_values::{column, string},
    workspace_decode_error::corrupt,
};
use futures_util::{StreamExt, stream};
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};

pub enum DueCutoff {
    BatchStart,
    LookAhead,
}
pub struct TimedSource {
    pub machine_kind: MachineKind,
    pub list_sql: &'static str,
    pub cutoff: DueCutoff,
}
pub enum TimedRowOutcome {
    Finished(AlarmResult),
    CentralDown,
}
pub trait TimedRows<R: EngineHost>: Send + Sync {
    type Kind: Copy + Send + Sync + 'static;
    fn ordered_kinds(&self) -> &'static [Self::Kind];
    fn source(&self, kind: Self::Kind) -> TimedSource;
    fn run_row(
        &self,
        engine: &Engine<'_, R>,
        runtime: &R,
        kind: Self::Kind,
        row: DueRow,
    ) -> impl Future<Output = TimedRowOutcome> + Send;
}
pub struct WholeUnit {
    pub subject: EngineSubject,
    pub threshold: RetirementThreshold,
}
pub enum WholeUnitOutcome {
    Skipped,
    Completed(AlarmResult),
}

enum Unit<TimedKind, WholeKind> {
    Machine(
        TimedKind,
        #[cfg_attr(
            not(feature = "test-support"),
            expect(
                dead_code,
                reason = "the source kind labels only the test-support visit receipt"
            )
        )]
        MachineKind,
        DueRow,
    ),
    Whole(WholeKind, WholeUnit),
}
#[cfg(feature = "test-support")]
impl<TimedKind, WholeKind> Unit<TimedKind, WholeKind> {
    fn start_receipt(&self, now: EngineTime) -> fleet_runtime::DueUnitStart {
        let (kind, key) = match self {
            Self::Machine(_, kind, row) => (kind.0, row.key.0.clone()),
            Self::Whole(_, unit) => (unit.subject.kind.0, unit.subject.key.0.clone()),
        };
        fleet_runtime::DueUnitStart {
            batch_start_unix_millis: now.timestamp_millis(),
            machine_kind: kind.to_owned(),
            subject_key: key,
        }
    }
}

fn source(kind: MachineKind) -> EngineSubject {
    EngineSubject {
        kind: MachineKind("engine_source"),
        key: InstanceKey(kind.0.into()),
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
        #[cfg(feature = "test-support")]
        {
            self.0.record_engine_completion_entry();
            self.pauses()
                .reached_central(fleet_runtime::PausePoint::EngineCompletionEntered)
                .await;
        }
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
            .publish_engine_query(central_engine_model::next_due_sql(self.0.alarm_inputs()))
            .await?;
        Ok(EventCompletion { work, publication })
    }

    pub async fn publish_engine_query(
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
        let timed = self.0.timed_rows();
        for &kind in timed.ordered_kinds() {
            let description = timed.source(kind);
            let subject = source(description.machine_kind);
            let observed = match self
                .control_gate(&subject, RetirementThreshold::DEFAULT)
                .await
            {
                ControlGate::Run(observed) => observed,
                ControlGate::Skip | ControlGate::Uncertain => continue,
                ControlGate::CentralDown(failure) => {
                    self.observe_engine_store_down(&subject, failure);
                    central_down = true;
                    break;
                }
            };
            let cutoff = match description.cutoff {
                DueCutoff::BatchStart => now,
                DueCutoff::LookAhead => horizon,
            };
            let query = match central_engine_model::due_list_sql(
                description.list_sql,
                cutoff,
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
                    units.extend(
                        rows.into_iter()
                            .map(|row| Unit::Machine(kind, description.machine_kind, row)),
                    );
                    match self.clear_control(&subject, &observed).await {
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
            units.extend(
                self.0
                    .ordered_whole_units()
                    .iter()
                    .map(|&kind| Unit::Whole(kind, self.0.whole_unit(kind))),
            );
            let stop = AtomicBool::new(false);
            let mut completed = stream::iter(units)
                .map(|unit| {
                    let stop = &stop;
                    async move {
                        if stop.load(Ordering::Acquire) {
                            return AlarmResult::default();
                        }
                        #[cfg(feature = "test-support")]
                        self.due_batches()
                            .record_unit_start(unit.start_receipt(now));
                        let (work, down) = self.run_engine_unit(unit, now).await;
                        if down {
                            stop.store(true, Ordering::Release);
                        }
                        work
                    }
                })
                .buffer_unordered(8);
            while let Some(done) = completed.next().await {
                work += done;
            }
        }
        self.due_batches().finish(now.timestamp_millis(), leftovers);
        work
    }

    async fn run_engine_unit(
        &self,
        unit: Unit<<R::Timed as TimedRows<R>>::Kind, R::WholeKind>,
        now: EngineTime,
    ) -> (AlarmResult, bool) {
        match unit {
            Unit::Machine(kind, _, row) => {
                match self.0.timed_rows().run_row(self, self.0, kind, row).await {
                    TimedRowOutcome::Finished(work) => (work, false),
                    TimedRowOutcome::CentralDown => (AlarmResult::default(), true),
                }
            }
            Unit::Whole(kind, unit) => self.run_whole_unit(kind, unit, now).await,
        }
    }

    async fn run_whole_unit(
        &self,
        kind: R::WholeKind,
        unit: WholeUnit,
        now: EngineTime,
    ) -> (AlarmResult, bool) {
        let WholeUnit { subject, threshold } = unit;
        let empty = AlarmResult::default();
        let observed = match self.control_gate(&subject, threshold).await {
            ControlGate::Run(observed) => observed,
            ControlGate::Skip | ControlGate::Uncertain => return (empty, false),
            ControlGate::CentralDown(failure) => {
                self.observe_engine_store_down(&subject, failure);
                return (empty, true);
            }
        };
        match self.0.run_whole_unit(kind, now, &subject, &observed).await {
            Ok(WholeUnitOutcome::Skipped) => (empty, false),
            Ok(WholeUnitOutcome::Completed(work)) => {
                #[cfg(feature = "test-support")]
                self.pauses()
                    .reached_engine(
                        fleet_runtime::PausePoint::BeforeControlClear,
                        subject.kind.0,
                    )
                    .await;
                match self.clear_control(&subject, &observed).await {
                    Err(ApplyFailure::StoreDown(_) | ApplyFailure::Rejected(_)) => (empty, true),
                    Ok(()) | Err(ApplyFailure::Superseded | ApplyFailure::Uncertain) => {
                        (work, false)
                    }
                }
            }
            Err(LegacyUnitOutcome::CentralDown(failure)) => {
                self.observe_engine_store_down(&subject, failure);
                (empty, true)
            }
            Err(outcome) => match outcome.failure_class() {
                Some(class) => (
                    empty,
                    matches!(
                        self.record_failure_naming(
                            &subject,
                            CONTROL_WRITE,
                            &SubjectVersion(vec![]),
                            class,
                            threshold,
                            outcome.affected_workspaces(),
                        )
                        .await,
                        FailureOutcome::StoreDown(_)
                    ),
                ),
                None => (empty, false),
            },
        }
    }

    /// One due machine instance as its own unit: `(done, central_down)`.
    pub async fn run_machine_unit<M, Adapter>(
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
}
