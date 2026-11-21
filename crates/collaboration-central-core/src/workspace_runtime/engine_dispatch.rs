//! Core's concrete timed jobs and whole-unit execution.
use crate::WorkspaceRuntime;
#[cfg(feature = "test-support")]
use crate::engine::query_registry;
use central_engine_model::{DueRow, EngineTime, InstanceKey, MachineKind, RetirementThreshold};
use central_storage::{AlarmUpdate, StatementBatch};
use fleet_contracts::{AlarmResult, EngineSubject, LegacyUnitOutcome, ObservedControl};
use fleet_engine::{
    DueCutoff, Engine, TimedRowOutcome, TimedRows, TimedSource, WholeUnit, WholeUnitOutcome,
};
use fleet_mail::{
    DirectMessage, DirectMessageEvent, DirectMessageStore, ExpireAt, SystemMessageExpiry,
    SystemMessageExpiryStore, insert_message,
};
use fleet_runtime_ports::{CentralStore, WorkspaceStores};
use fleet_schedules::{self as automation_timer, AutomationStore};
use fleet_wakes::{self as wake_timer, WakeStore};

#[derive(Clone, Copy)]
pub enum DueKind {
    Wake,
    Automation,
    DirectMessage,
    SystemMessageExpiry,
    #[cfg(feature = "test-support")]
    Probe,
}
impl DueKind {
    fn kind(self) -> MachineKind {
        match self {
            Self::Wake => MachineKind("wake_timer"),
            Self::Automation => MachineKind("automation_timer"),
            Self::DirectMessage => MachineKind("direct_message"),
            Self::SystemMessageExpiry => {
                <SystemMessageExpiry as central_engine_model::Machine>::KIND
            }
            #[cfg(feature = "test-support")]
            Self::Probe => MachineKind("engine_probe"),
        }
    }
    fn sql(self) -> &'static str {
        match self {
            Self::Wake => fleet_wakes::WAKE_LIST,
            Self::Automation => fleet_schedules::AUTOMATION_LIST,
            Self::DirectMessage => fleet_mail::DIRECT_MESSAGE_LIST,
            Self::SystemMessageExpiry => fleet_mail::SYSTEM_MESSAGE_EXPIRY_LIST,
            #[cfg(feature = "test-support")]
            Self::Probe => query_registry::PROBE_LIST,
        }
    }
}

pub struct CoreTimedRows;
impl<H: CentralStore, S: WorkspaceStores> TimedRows<WorkspaceRuntime<H, S>> for CoreTimedRows {
    type Kind = DueKind;
    fn ordered_kinds(&self) -> &'static [DueKind] {
        &[
            DueKind::Wake,
            DueKind::Automation,
            DueKind::DirectMessage,
            DueKind::SystemMessageExpiry,
            #[cfg(feature = "test-support")]
            DueKind::Probe,
        ]
    }
    fn source(&self, kind: DueKind) -> TimedSource {
        TimedSource {
            machine_kind: kind.kind(),
            list_sql: kind.sql(),
            cutoff: if matches!(kind, DueKind::SystemMessageExpiry) {
                DueCutoff::BatchStart
            } else {
                DueCutoff::LookAhead
            },
        }
    }
    async fn run_row(
        &self,
        engine: &Engine<'_, WorkspaceRuntime<H, S>>,
        runtime: &WorkspaceRuntime<H, S>,
        kind: DueKind,
        row: DueRow,
    ) -> TimedRowOutcome {
        let mut work = AlarmResult::default();
        let (done, down) = match kind {
            #[cfg(feature = "test-support")]
            DueKind::Probe => {
                engine
                    .run_machine_unit::<crate::engine_probe::EngineProbe, _>(
                        &crate::engine::probe_store::ProbeStore(runtime),
                        row,
                        "engine_probe_items.due_at",
                        |occurrence| crate::engine_probe::ProbeEvent::Due { occurrence },
                    )
                    .await
            }
            DueKind::Wake => {
                engine
                    .run_machine_unit::<wake_timer::WakeTimer, _>(
                        &WakeStore::new(runtime, insert_message),
                        row,
                        "automation_wakes.due_at",
                        |occurrence| wake_timer::Event::Due { occurrence },
                    )
                    .await
            }
            DueKind::Automation => {
                engine
                    .run_machine_unit::<automation_timer::AutomationTimer, _>(
                        &AutomationStore::new(runtime, insert_message),
                        row,
                        "automation_schedules.next_due_at",
                        |occurrence| automation_timer::Event::Due { occurrence },
                    )
                    .await
            }
            DueKind::DirectMessage => {
                engine
                    .run_machine_unit::<DirectMessage, _>(
                        &DirectMessageStore::new(runtime),
                        row,
                        "mail_messages.cancel_due_at",
                        |occurrence| DirectMessageEvent::Due { occurrence },
                    )
                    .await
            }
            DueKind::SystemMessageExpiry => {
                engine
                    .run_machine_unit::<SystemMessageExpiry, _>(
                        &SystemMessageExpiryStore::new(runtime),
                        row,
                        "mail_messages.delivery_window_ends_at",
                        ExpireAt,
                    )
                    .await
            }
        };
        match kind {
            DueKind::Wake => work.wakes_fired = usize::from(done),
            DueKind::Automation => work.automations_fired = usize::from(done),
            // Cancels are observed through the messages they change.
            DueKind::DirectMessage | DueKind::SystemMessageExpiry => {}
            #[cfg(feature = "test-support")]
            DueKind::Probe => {}
        }

        if down {
            TimedRowOutcome::CentralDown
        } else {
            TimedRowOutcome::Finished(work)
        }
    }
}

#[derive(Clone, Copy)]
pub enum WholeKind {
    Windows,
    Drains,
}
impl WholeKind {
    pub(crate) fn description(self) -> WholeUnit {
        let kind = match self {
            Self::Windows => "legacy_notice_windows",
            Self::Drains => "legacy_workspace_drains",
        };
        WholeUnit {
            subject: EngineSubject {
                kind: MachineKind(kind),
                key: InstanceKey("all".into()),
            },
            threshold: RetirementThreshold::NOTICE_UNITS,
        }
    }
}

impl<H: CentralStore, S: WorkspaceStores> WorkspaceRuntime<H, S> {
    pub(crate) async fn execute_whole_unit(
        &self,
        kind: WholeKind,
        now: EngineTime,
        subject: &EngineSubject,
        observed: &ObservedControl,
    ) -> Result<WholeUnitOutcome, LegacyUnitOutcome> {
        let mut work = AlarmResult::default();
        match kind {
            WholeKind::Windows => {
                let Some(_turn) = self.execution.try_central_turn() else {
                    return Ok(WholeUnitOutcome::Skipped);
                };
                #[cfg(feature = "test-support")]
                self.execution
                    .count_legacy_invocation(|count| count.windows += 1);
                work.windows_fired = self.run_window_unit(now, observed, subject).await?;
            }
            WholeKind::Drains => {
                #[cfg(feature = "test-support")]
                self.execution
                    .count_legacy_invocation(|count| count.drains += 1);
                work.workspaces_drained = self.drain_due_workspaces(now).await?;
            }
        }
        Ok(WholeUnitOutcome::Completed(work))
    }

    /// Runs under the central turn its caller took without waiting (change 7).
    async fn run_window_unit(
        &self,
        now: EngineTime,
        observed: &ObservedControl,
        subject: &EngineSubject,
    ) -> Result<usize, LegacyUnitOutcome> {
        let (mut statements, deliveries) = self.plan_due_windows(now).await?;
        let count = deliveries.len();
        match observed {
            ObservedControl::Absent => {}
            ObservedControl::Present(guard) => {
                statements.push(fleet_engine::delete_observed_attempt(subject, guard))
            }
        }
        // The kept notice tests hold the commit that delivers windows at the
        // existing timer-commit points; an empty window unit passes them by.
        #[cfg(feature = "test-support")]
        if count > 0 {
            self.execution
                .pause_points()
                .reached_central(fleet_runtime::PausePoint::BeforeTimerCommit)
                .await;
        }
        LegacyUnitOutcome::batch(
            self.execution
                .statement_host()
                .run_batch(StatementBatch {
                    statements,
                    alarm: AlarmUpdate::Unchanged,
                })
                .await,
        )?;
        for delivery in deliveries {
            (self.mail_delivery_ready)(delivery);
        }
        #[cfg(feature = "test-support")]
        if count > 0 {
            self.execution
                .pause_points()
                .reached_central(fleet_runtime::PausePoint::AfterTimerCommit)
                .await;
        }
        Ok(count)
    }
}
