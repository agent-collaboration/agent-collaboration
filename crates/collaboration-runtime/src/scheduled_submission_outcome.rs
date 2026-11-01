//! Persist each submitted run outcome and its owed push settlement in automation.
use super::{
    CodexGeneration, DeliveryOutcome, DeliveryReceipt, EndpointRef, PushDeliveryState,
    RouteEffectEvidence, RunExecution, RunId, RunSubmission, RunSubmissionOutcome,
    RunSubmissionResult, SchedulePushSettlementContext, ScheduleRunPushUpdate, ScheduledRunWorker,
    SessionRef, StorageError,
};

impl ScheduledRunWorker {
    pub(super) async fn persist_submission(
        &self,
        id: RunId,
        effects: RouteEffectEvidence<SessionRef, CodexGeneration>,
        submission: RunSubmission,
        schedule_push: SchedulePushSettlementContext,
    ) -> Result<(), StorageError> {
        let (outcome, push_receipt, push_state) = match submission {
            RunSubmission::NotStartedBusy => {
                if let Some((push_id, _, _)) = schedule_push {
                    self.interactions
                        .restore_schedule_push_pending(&push_id)
                        .await
                        .map_err(crate::map_interaction_storage_error)?;
                }
                return Ok(());
            }
            RunSubmission::Started(acceptance) => {
                let run_receipt = crate::stored_run_receipt::StoredRunReceipt::Current(
                    acceptance.receipt.clone(),
                );
                let push_receipt = acceptance.receipt;
                let outcome = match acceptance.execution {
                    RunExecution::CodexAppServer(native) => RunSubmissionOutcome::Accepted {
                        turn_id: native.turn_id,
                        receipt: run_receipt,
                    },
                    RunExecution::ProviderAcp { .. } => RunSubmissionOutcome::ProviderAdmitted {
                        receipt: run_receipt,
                    },
                    RunExecution::ClaudeCodePeer { written_at, .. } => {
                        let timestamp =
                            chrono::DateTime::parse_from_rfc3339(&String::from(written_at))
                                .map_err(|_| StorageError::InvalidRecord)?
                                .timestamp_millis();
                        RunSubmissionOutcome::PeerWritten {
                            written_at_ms: timestamp,
                            receipt: run_receipt,
                        }
                    }
                };
                (outcome, push_receipt, PushDeliveryState::Delivered)
            }
            RunSubmission::Rejected(rejection) => {
                let receipt = DeliveryReceipt {
                    outcome: DeliveryOutcome::Rejected(rejection.clone()),
                    reachability: None,
                    client: None,
                };
                (
                    RunSubmissionOutcome::Rejected {
                        explanation: rejection
                            .detail
                            .unwrap_or_else(|| format!("{:?}", rejection.reason)),
                    },
                    receipt,
                    PushDeliveryState::Pending,
                )
            }
            RunSubmission::Unknown => (
                RunSubmissionOutcome::Unknown {
                    explanation: "Scheduled start outcome unknown; no automatic resend.".into(),
                },
                DeliveryReceipt {
                    outcome: DeliveryOutcome::Unknown,
                    reachability: None,
                    client: None,
                },
                PushDeliveryState::OutcomeUnknown,
            ),
        };
        let result = RunSubmissionResult {
            run_id: id,
            effects,
            outcome,
        };
        let push_update = schedule_push.map(|(push_id, origin_router_ref, recovered_outcome)| {
            ScheduleRunPushUpdate {
                push_id,
                origin_router_ref,
                delivery_state: recovered_outcome
                    .as_ref()
                    .map_or(push_state, |(state, _)| *state),
                receipt: recovered_outcome.map_or(push_receipt, |(_, receipt)| receipt),
                settled_at: chrono::Utc::now(),
            }
        });
        let mut delay = std::time::Duration::from_millis(25);
        loop {
            let write = {
                let mut store = self.store.lock().await;
                match &push_update {
                    Some(push_update) => {
                        store
                            .record_run_submission_with_push::<_, EndpointRef, message_board::ProjectId, _, _>(
                                result.clone(),
                                push_update.clone(),
                            )
                            .await
                    }
                    None => {
                        store
                            .record_run_submission::<_, EndpointRef, message_board::ProjectId, _, _>(result.clone())
                            .await
                    }
                }
            };
            match write {
                Ok(()) => return Ok(()),
                Err(StorageError::Database(_)) if push_update.is_some() => {
                    tokio::time::sleep(delay).await;
                    delay = delay
                        .saturating_mul(2)
                        .min(std::time::Duration::from_secs(1));
                }
                Err(error) => return Err(error),
            }
        }
    }
}
