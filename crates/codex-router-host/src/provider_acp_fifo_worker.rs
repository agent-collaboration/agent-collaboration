//! One session line's worker (D1 state machines, design note §11.2, §12.2,
//! §13.1): takes the lowest-numbered payload, waits for the door's load,
//! claims the message, admits a queued delivery and commits once the
//! session's cell allows, then dispatches it and waits for its settlement
//! before the next.

use super::entries::{QueuedEntries, mark_entry_not_sent};
use super::{
    CONNECTION_CLOSED, MAX_RETRY_DELAY, MIN_RETRY_DELAY, ProviderQueuedPrompt, ROUTER_SHUTDOWN,
    SESSION_CLOSED, SETTLEMENT_WAIT_SECONDS, SessionLine, lock,
};
use crate::external_provider_supervisor::QueuedSend;
use crate::{
    ExternalProviderSupervisor, external_provider_supervisor::ProviderLineRuntime,
    external_provider_supervisor::ProviderPromptDispatch,
};
use acp_client_runtime::QueueRefusal;
use collaboration_contracts::ProviderConversationBackend;
use collaboration_protocol::{
    ConversationOperationWaitOutput, ConversationOperationWaitRequest, OperationId,
    PositiveSeconds, ProviderOperationStage, SessionRef,
};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio_util::sync::CancellationToken;

pub(super) struct SessionLineWorker {
    pub(super) target: SessionRef,
    pub(super) line: Arc<SessionLine>,
    pub(super) entries: Arc<QueuedEntries>,
    pub(super) supervisor: Arc<ExternalProviderSupervisor>,
    #[cfg(test)]
    pub(super) wait_started: Arc<tokio::sync::Notify>,
}

/// One session's worker: takes the lowest-numbered payload, waits for the
/// door's load, claims the message and commits it once the session's cell
/// allows, then dispatches it and waits for its settlement before the next.
pub(super) async fn run_session_line(worker: SessionLineWorker) {
    let SessionLineWorker {
        target,
        line,
        entries,
        supervisor,
        #[cfg(test)]
        wait_started,
    } = worker;
    let session_id = String::from(target.session_id.clone());
    loop {
        line.holding.store(false, Ordering::SeqCst);
        let Some((sequence, operation_id)) = next_payload(&line).await else {
            for operation_id in entries.waiting_for(&target) {
                entries.settle_any_not_sent(&operation_id, ROUTER_SHUTDOWN);
            }
            return;
        };
        line.holding.store(true, Ordering::SeqCst);
        let Some(record) = entries.get(&operation_id) else {
            // A move already ended it; a failed load's messages settle here
            // with the load's reason.
            entries.settle_load_failed(&operation_id, &line.load_failure());
            continue;
        };
        let runtime = record.runtime.clone();
        if let Some(load) = line.current_load() {
            let loaded = tokio::select! {
                () = line.stop.cancelled() => continue,
                loaded = load => loaded,
            };
            if let Err(reason) = loaded {
                entries.settle_any_not_sent(&operation_id, &reason);
                continue;
            }
        }
        #[cfg(test)]
        wait_started.notify_one();
        match runtime.queue_claim(&session_id, sequence) {
            Ok(()) => {}
            Err(QueueRefusal::NotYet) => {
                // An earlier message's payload hasn't arrived: wait for it.
                lock(&line.pending).insert(sequence, operation_id);
                tokio::select! {
                    () = line.stop.cancelled() => {}
                    () = line.arrived.notified() => {}
                }
                continue;
            }
            Err(QueueRefusal::EntryEnded) => {
                entries.settle_load_failed(&operation_id, &line.load_failure());
                continue;
            }
            Err(QueueRefusal::NotLoaded { answer } | QueueRefusal::Refused { answer }) => {
                entries.settle_not_sent(&operation_id, &answer);
                continue;
            }
        }
        match commit_when_allowed(
            &line,
            &entries,
            &supervisor,
            &runtime,
            &session_id,
            sequence,
            &operation_id,
        )
        .await
        {
            CommitOutcome::Committed => {}
            CommitOutcome::Ended => continue,
            CommitOutcome::AdmittedBefore => {
                // Admitted before under this ID: this message sends nothing
                // new; the line waits for that operation's own settlement.
                let _cancelled =
                    runtime.queue_cancel(&session_id, &String::from(operation_id.clone()));
                if entries.take(&operation_id).is_some() {
                    await_settlement(&line, &runtime, &supervisor, &operation_id).await;
                }
                continue;
            }
        }
        // From the commit on, this worker's dispatch owns the message.
        let Some(record) = entries.take(&operation_id) else {
            continue;
        };
        let queued_send = QueuedSend::new(runtime.clone(), session_id.clone(), sequence);
        if !supervisor
            .queued_operation_registry()
            .try_start(&operation_id)
        {
            queued_send.mark_not_written(false);
            continue;
        }
        let retirement = runtime.retirement();
        let submitted = tokio::select! {
            () = line.stop.cancelled() => continue,
            () = retirement.cancelled() => continue,
            submitted = async {
                match record.prompt.clone() {
                    ProviderQueuedPrompt::Contents { request, admitted: Some(admitted), .. } if record.admitted_at_head =>
                        supervisor.dispatch_queued_delivery(request, &admitted, queued_send).await,
                    ProviderQueuedPrompt::Contents { request, admitted: Some(admitted), .. } =>
                        supervisor.dispatch_queued_api_prompt(request, &admitted, queued_send).await,
                    // Admitted at the head before the commit; never reached.
                    ProviderQueuedPrompt::Contents { request, admitted: None, .. } => {
                        queued_send.mark_not_written(false);
                        Err(Box::new(crate::external_provider_supervisor::unadmitted_queued_failure(
                            request.operation_id,
                            request.target,
                        )))
                    }
                }
            } => submitted,
        };
        let operation_admitted = match submitted {
            // Nothing reached the agent: the line records why, so a caller
            // waiting on this input learns it now, and the record stays for
            // show instead of being cleared as if it ran (D1 R14).
            Ok(ProviderPromptDispatch::Refused(reason)) => {
                let reason = crate::external_provider_supervisor::dispatch_refusal_reason(
                    reason,
                    operation_id.clone(),
                    target.clone(),
                );
                supervisor
                    .queued_operation_registry()
                    .mark_not_submitted(&operation_id, reason);
                false
            }
            Ok(ProviderPromptDispatch::NotSubmitted) => {
                supervisor
                    .queued_operation_registry()
                    .mark_not_submitted(&operation_id, "provider input was not submitted");
                false
            }
            // This ID has been admitted. From here on, observe this exact
            // operation; never submit its logical message under the same ID.
            Ok(
                ProviderPromptDispatch::Submitted
                | ProviderPromptDispatch::Existing
                | ProviderPromptDispatch::Uncertain,
            ) => true,
            Err(failure) => {
                mark_entry_not_sent(&supervisor, &record.prompt, String::from(failure.message));
                false
            }
        };
        if operation_admitted {
            await_settlement(&line, &runtime, &supervisor, &operation_id).await;
        }
    }
}

/// The lowest-numbered payload in the line, once one has arrived; none once
/// the line is stopped.
async fn next_payload(line: &SessionLine) -> Option<(u64, OperationId)> {
    loop {
        let arrived = line.arrived.notified();
        tokio::pin!(arrived);
        arrived.as_mut().enable();
        if line.stop.is_cancelled() {
            return None;
        }
        if let Some(next) = lock(&line.pending).pop_first() {
            return Some(next);
        }
        tokio::select! {
            () = line.stop.cancelled() => return None,
            () = arrived => {}
        }
    }
}

/// Whether the held message may go on to its commit.
enum HeadAdmission {
    Ready,
    /// A move ended it, or its admission failed and it settled not sent.
    Ended,
    /// Its operation was admitted before under the same ID.
    AdmittedBefore,
}

/// A queued delivery is admitted when it reaches the head, as an API prompt
/// is before it is queued; while an unresolved operation still blocks
/// admission (Busy) the line tries again, so a transient blocker only delays
/// it.
async fn admit_delivery_at_head(
    line: &SessionLine,
    entries: &QueuedEntries,
    supervisor: &ExternalProviderSupervisor,
    runtime: &ProviderLineRuntime,
    operation_id: &OperationId,
) -> HeadAdmission {
    let Some(record) = entries.get(operation_id) else {
        return HeadAdmission::Ended;
    };
    let ProviderQueuedPrompt::Contents {
        request,
        admitted: None,
    } = &record.prompt
    else {
        return HeadAdmission::Ready;
    };
    let retirement = runtime.retirement();
    let session_id = String::from(request.target.session_id.clone());
    let operation = String::from(request.operation_id.clone());
    let mut retry_delay = MIN_RETRY_DELAY;
    loop {
        if line.stop.is_cancelled() {
            entries.settle_any_not_sent(&request.operation_id, ROUTER_SHUTDOWN);
            return HeadAdmission::Ended;
        }
        if retirement.is_cancelled() {
            entries.settle_any_not_sent(&request.operation_id, CONNECTION_CLOSED);
            return HeadAdmission::Ended;
        }
        match supervisor.admit_queued_delivery(request).await {
            Ok(Some(admitted)) => {
                if entries.admit(&request.operation_id, admitted.clone()) {
                    return HeadAdmission::Ready;
                }
                // The message ended while it was admitted: its operation
                // settles not sent too.
                supervisor.complete_unsent_api_entry(
                    &admitted,
                    &request.operation_id,
                    &request.target,
                    SESSION_CLOSED,
                );
                return HeadAdmission::Ended;
            }
            Ok(None) => return HeadAdmission::AdmittedBefore,
            Err(failure)
                if failure.kind
                    == collaboration_protocol::ConversationOperationFailureKind::Busy => {}
            Err(failure) => {
                let _cancelled = runtime.queue_cancel(&session_id, &operation);
                entries.settle_not_sent(&request.operation_id, &String::from(failure.message));
                return HeadAdmission::Ended;
            }
        }
        if !wait_before_retry(line.clock.as_ref(), &line.stop, &retirement, retry_delay).await {
            continue;
        }
        retry_delay = next_retry_delay(retry_delay);
        if entries.get(&request.operation_id).is_none() {
            return HeadAdmission::Ended;
        }
    }
}

/// How waiting for the held message's commit ended.
enum CommitOutcome {
    Committed,
    /// It ended instead, settled by whoever ended it.
    Ended,
    /// Its operation was admitted before under the same ID.
    AdmittedBefore,
}

/// Waits until the session's cell commits the held message: the run is
/// Live, the message is the head, and nothing else is in flight. A queued
/// delivery is admitted just before its commit, once the session is idle.
async fn commit_when_allowed(
    line: &SessionLine,
    entries: &QueuedEntries,
    supervisor: &ExternalProviderSupervisor,
    runtime: &ProviderLineRuntime,
    session_id: &str,
    sequence: u64,
    operation_id: &OperationId,
) -> CommitOutcome {
    let retirement = runtime.retirement();
    let mut retry_delay = MIN_RETRY_DELAY;
    loop {
        if line.stop.is_cancelled() {
            entries.settle_any_not_sent(operation_id, ROUTER_SHUTDOWN);
            return CommitOutcome::Ended;
        }
        if retirement.is_cancelled() {
            entries.settle_any_not_sent(operation_id, CONNECTION_CLOSED);
            return CommitOutcome::Ended;
        }
        let idle = tokio::select! {
            () = line.stop.cancelled() => continue,
            () = retirement.cancelled() => continue,
            idle = runtime.wait_session_idle(session_id.to_owned()) => idle,
        };
        if idle.is_ok() {
            match admit_delivery_at_head(line, entries, supervisor, runtime, operation_id).await {
                HeadAdmission::Ready => {}
                HeadAdmission::Ended => return CommitOutcome::Ended,
                HeadAdmission::AdmittedBefore => return CommitOutcome::AdmittedBefore,
            }
            match runtime.queue_commit(session_id, sequence) {
                Ok(()) => return CommitOutcome::Committed,
                Err(QueueRefusal::NotYet) => {}
                Err(QueueRefusal::EntryEnded) => return CommitOutcome::Ended,
                Err(QueueRefusal::NotLoaded { answer } | QueueRefusal::Refused { answer }) => {
                    entries.settle_not_sent(operation_id, &answer);
                    return CommitOutcome::Ended;
                }
            }
        }
        if !wait_before_retry(line.clock.as_ref(), &line.stop, &retirement, retry_delay).await {
            continue;
        }
        retry_delay = next_retry_delay(retry_delay);
    }
}

/// Waits for a dispatched message's operation to reach its terminal state,
/// then clears its queue record.
async fn await_settlement(
    line: &SessionLine,
    runtime: &ProviderLineRuntime,
    supervisor: &ExternalProviderSupervisor,
    operation_id: &OperationId,
) {
    let Ok(wait_seconds) = PositiveSeconds::try_from(SETTLEMENT_WAIT_SECONDS) else {
        return;
    };
    let retirement = runtime.retirement();
    loop {
        let settled = tokio::select! {
            () = line.stop.cancelled() => return,
            () = retirement.cancelled() => return,
            result = supervisor.wait(ConversationOperationWaitRequest {
                operation_id: operation_id.clone(), timeout_seconds: wait_seconds,
            }) => result,
        };
        match settled {
            Ok(result)
                if result.operation.stage == ProviderOperationStage::Terminal
                    && !matches!(result.output, ConversationOperationWaitOutput::Pending) =>
            {
                supervisor.queued_operation_registry().clear(operation_id);
                return;
            }
            Ok(_) => {}
            Err(_failure) => {
                // The operation's live cell answers; its durable row only
                // once the live index no longer holds it.
                if supervisor.operation_reached_terminal(operation_id).await {
                    supervisor.queued_operation_registry().clear(operation_id);
                    return;
                }
                if !wait_before_retry(
                    line.clock.as_ref(),
                    &line.stop,
                    &retirement,
                    MIN_RETRY_DELAY,
                )
                .await
                {
                    return;
                }
            }
        }
    }
}

async fn wait_before_retry(
    clock: &dyn router_clock::RuntimeClock,
    stop: &CancellationToken,
    retirement: &CancellationToken,
    delay: std::time::Duration,
) -> bool {
    tokio::select! {
        () = stop.cancelled() => false,
        () = retirement.cancelled() => false,
        () = clock.sleep_for(delay) => true,
    }
}

fn next_retry_delay(current: std::time::Duration) -> std::time::Duration {
    current.saturating_mul(2).min(MAX_RETRY_DELAY)
}
