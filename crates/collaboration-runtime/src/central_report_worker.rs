//! A sparse report queue wakes at its oldest durable deadline, or immediately at 100 rows.
use crate::{CentralAutomationClock, CentralAutomationError, CentralAutomationTransport};
use automation_storage::{AutomationStore, CentralReportBatch};
use collaboration_protocol::{AutomationAnswer, AutomationCall, ReportWorkItems};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub struct CentralReportWorker {
    store: Arc<Mutex<AutomationStore>>,
    transport: Arc<dyn CentralAutomationTransport>,
    clock: Arc<dyn CentralAutomationClock>,
}
impl CentralReportWorker {
    pub fn new(
        store: Arc<Mutex<AutomationStore>>,
        transport: Arc<dyn CentralAutomationTransport>,
        clock: Arc<dyn CentralAutomationClock>,
    ) -> Self {
        Self {
            store,
            transport,
            clock,
        }
    }
    pub async fn flush_due(&self) -> Result<Option<i64>, CentralAutomationError> {
        loop {
            let pending = self
                .store
                .lock()
                .await
                .central_report_batch(self.clock.now_ms())
                .await?;
            match pending {
                CentralReportBatch::Empty => return Ok(None),
                CentralReportBatch::Waiting { due_at_ms } => return Ok(Some(due_at_ms)),
                CentralReportBatch::Ready(reports) => {
                    let envelope = ReportWorkItems::new(
                        reports.iter().map(|owed| owed.report.clone()).collect(),
                    )
                    .map_err(|_| CentralAutomationError::InvalidWork)?;
                    let AutomationAnswer::Reports(acknowledgements) = self
                        .transport
                        .call(AutomationCall::ReportWorkItems(envelope))
                        .await?
                    else {
                        return Err(CentralAutomationError::InvalidReply);
                    };
                    #[cfg(debug_assertions)]
                    {
                        let faults = self.store.lock().await.automation_faults();
                        if let Some(faults) = faults {
                            for owed in &reports {
                                let evidence = match &owed.report {
                                    collaboration_protocol::WorkItemReport::Firing(report) => {
                                        automation_storage::AutomationFaultEvidence {
                                            firing_id: Some(report.firing_id.clone()),
                                            ..Default::default()
                                        }
                                    }
                                    collaboration_protocol::WorkItemReport::SessionCreate {
                                        work_item_id,
                                        ..
                                    } => automation_storage::AutomationFaultEvidence {
                                        work_item_id: Some(work_item_id.clone()),
                                        ..Default::default()
                                    },
                                };
                                faults.hold(automation_storage::AutomationFaultPoint::ReportSentBeforeAcknowledgement,evidence)
                                    .await;
                            }
                        }
                    }
                    if acknowledgements.accepted.is_empty()
                        || acknowledgements.accepted.iter().any(|ack| {
                            !reports
                                .iter()
                                .any(|owed| match (&ack.subject, &owed.report) {
                                    (
                                        collaboration_protocol::WorkReportSubject::Firing {
                                            firing_id,
                                        },
                                        collaboration_protocol::WorkItemReport::Firing(report),
                                    ) => firing_id == &report.firing_id && ack.seq == report.seq,
                                    (
                                        collaboration_protocol::WorkReportSubject::SessionCreate {
                                            work_item_id,
                                        },
                                        collaboration_protocol::WorkItemReport::SessionCreate {
                                            work_item_id: reported,
                                            seq,
                                            ..
                                        },
                                    ) => work_item_id == reported && ack.seq == *seq,
                                    _ => false,
                                })
                        })
                    {
                        return Err(CentralAutomationError::InvalidReply);
                    }
                    self.store
                        .lock()
                        .await
                        .acknowledge_central_reports(&acknowledgements.accepted)
                        .await?;
                }
            }
        }
    }
    pub async fn run(self, shutdown: CancellationToken) {
        let changed = self.store.lock().await.central_report_changes();
        loop {
            let notified = changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let flushed = tokio::select! {
                biased;
                _=shutdown.cancelled()=>return,
                result=self.flush_due()=>result,
            };
            let deadline = match flushed {
                Ok(deadline) => deadline,
                Err(error) => {
                    tracing::warn!(%error,"central reports remain owed");
                    // A bounded transport retry changes no report identity or durable sequence.
                    Some(self.clock.now_ms().saturating_add(1000))
                }
            };
            tokio::select! {
                _ = shutdown.cancelled() => return,
                _ = &mut notified => {},
                _ = async { match deadline { Some(deadline) => self.clock.wait_until(deadline).await, None => std::future::pending().await } } => {},
            }
        }
    }
}
