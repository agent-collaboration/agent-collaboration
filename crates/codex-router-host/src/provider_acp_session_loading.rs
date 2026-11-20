//! Load a recorded provider session only after checking live peer ownership.
use crate::{
    ExternalProviderRuntime, ExternalProviderRuntimeError, ExternalProviderSupervisor,
    LiveSessionOwnership, LiveSessionOwnershipCheck, ProviderSessionActivity,
};
use acp_client_runtime::{RestoreFailure, RestorePhase, acp_transport::TransportRun};
use collaboration_contracts::LoadPolicy;
use collaboration_protocol::{DeliveryCorrelationId, SessionRef};
use collaboration_runtime::ProviderOperationStore;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::Mutex;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ProviderSessionLoadOutcome {
    Ready,
    AlreadyLoaded,
    NotLoaded,
    UnsupportedLoad,
    MissingRecord,
    LiveElsewhere,
    Rejected {
        phase: RestorePhase,
        reason: ProviderSessionLoadRejection,
    },
    Unavailable {
        phase: RestorePhase,
        reason: String,
    },
    /// A close of the session won while this load was in flight: the
    /// connection dropped the restored session, and the work that asked for
    /// the load ends not sent. A later load starts a new run.
    ClosedWhileLoading {
        phase: RestorePhase,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ProviderSessionLoadability {
    AlreadyLoaded,
    /// Loadable in its recorded folder, with the setup its create had
    /// rebuilt from its record (D1 R18; owner Decision 3).
    Loadable {
        working_directory: PathBuf,
        setup: acp_client_runtime::SessionRestoreSetup,
    },
    UnsupportedLoad,
    MissingRecord,
    LiveElsewhere,
    Unavailable {
        reason: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ProviderSessionLoadRejection {
    SessionNotFound {
        code: i64,
        correlation_id: DeliveryCorrelationId,
    },
    ProviderRejected {
        code: i64,
        correlation_id: DeliveryCorrelationId,
    },
}

impl ProviderSessionLoadRejection {
    #[must_use]
    pub(crate) fn code(&self) -> i64 {
        match self {
            Self::SessionNotFound { code, .. } | Self::ProviderRejected { code, .. } => *code,
        }
    }

    #[must_use]
    pub(crate) fn safe_detail(&self) -> String {
        match self {
            Self::SessionNotFound {
                code,
                correlation_id,
            } => {
                format!(
                    "the provider no longer knows this session, so start a new session with the same setup (provider code {code}; reference {})",
                    correlation_id.as_str()
                )
            }
            Self::ProviderRejected {
                code,
                correlation_id,
            } => {
                format!(
                    "the provider refused to restore this session, so check the provider before retrying (provider code {code}; reference {})",
                    correlation_id.as_str()
                )
            }
        }
    }
}

pub(crate) async fn ensure_provider_session_loaded(
    supervisor: &ExternalProviderSupervisor,
    store: &Arc<Mutex<ProviderOperationStore>>,
    ownership: &dyn LiveSessionOwnershipCheck,
    target: &SessionRef,
    load_policy: LoadPolicy,
) -> ProviderSessionLoadOutcome {
    let Some(runtime) = supervisor.runtime_for(&target.endpoint) else {
        return unavailable("provider runtime is unavailable");
    };
    use provider_session_machines::{
        session_lifecycle_machine::SessionLifecycle,
        session_owner::{RunAttempt, SessionOwnerReply},
    };
    if let Some(snapshot) = supervisor.owner_session_snapshot(target) {
        match snapshot.lifecycle {
            SessionLifecycle::Live { .. } => {
                return if load_policy == LoadPolicy::LoadedOnly {
                    ProviderSessionLoadOutcome::AlreadyLoaded
                } else {
                    ProviderSessionLoadOutcome::Ready
                };
            }
            SessionLifecycle::Loading {
                generation,
                attempt,
                ..
            } => {
                if load_policy == LoadPolicy::LoadedOnly {
                    return ProviderSessionLoadOutcome::NotLoaded;
                }
                let Ok(owner) = supervisor.session_owner_for(target) else {
                    return unavailable("provider session owner is unavailable");
                };
                return match owner
                    .join_restore(
                        uuid::Uuid::now_v7(),
                        RunAttempt {
                            generation,
                            attempt,
                        },
                    )
                    .await
                {
                    Ok(result) => owner_restore_outcome(result),
                    Err(error) => unavailable(error.to_string()),
                };
            }
            SessionLifecycle::NotLoaded { .. } | SessionLifecycle::Closing { .. } => {}
        }
    }
    match checked_loadability(supervisor, &runtime, store, ownership, target).await {
        ProviderSessionLoadability::AlreadyLoaded => {
            return if load_policy == LoadPolicy::LoadedOnly {
                ProviderSessionLoadOutcome::AlreadyLoaded
            } else {
                ProviderSessionLoadOutcome::Ready
            };
        }
        ProviderSessionLoadability::Loadable { .. } if load_policy == LoadPolicy::LoadedOnly => {
            return ProviderSessionLoadOutcome::NotLoaded;
        }
        ProviderSessionLoadability::Loadable { .. } => {}
        ProviderSessionLoadability::UnsupportedLoad => {
            return ProviderSessionLoadOutcome::UnsupportedLoad;
        }
        ProviderSessionLoadability::MissingRecord => {
            return ProviderSessionLoadOutcome::MissingRecord;
        }
        ProviderSessionLoadability::LiveElsewhere => {
            return ProviderSessionLoadOutcome::LiveElsewhere;
        }
        ProviderSessionLoadability::Unavailable { reason } => return unavailable(reason),
    }
    let Ok(owner) = supervisor.session_owner_for(target) else {
        return unavailable("provider session owner is unavailable");
    };
    match owner.restore_for_delivery(uuid::Uuid::now_v7()).await {
        Ok(settled) => match settled.reply {
            SessionOwnerReply::RestoreFinished(result) => owner_restore_outcome(result),
            _ => unavailable("provider session owner returned another result"),
        },
        Err(error) => unavailable(error.to_string()),
    }
}

async fn checked_loadability(
    supervisor: &ExternalProviderSupervisor,
    runtime: &ExternalProviderRuntime,
    store: &Arc<Mutex<ProviderOperationStore>>,
    ownership: &dyn LiveSessionOwnershipCheck,
    target: &SessionRef,
) -> ProviderSessionLoadability {
    let clock = runtime.owner_clock();
    router_clock::timeout(
        clock.as_ref(),
        std::time::Duration::from_secs(10),
        inspect_provider_session_loadability(supervisor, runtime, store, ownership, target),
    )
    .await
    .unwrap_or_else(|_| ProviderSessionLoadability::Unavailable {
        reason: "the Router could not read the session setup in time".to_owned(),
    })
}

#[derive(Clone, Copy)]
pub(crate) enum SessionQueuePlan {
    OnRun(TransportRun),
    Restore {
        run: TransportRun,
        attempt: provider_session_machines::session_lifecycle_machine::LoadAttempt,
    },
}

impl SessionQueuePlan {
    pub(crate) fn run(self) -> TransportRun {
        match self {
            Self::OnRun(run) | Self::Restore { run, .. } => run,
        }
    }
}

pub(crate) async fn session_queue_plan(
    supervisor: &ExternalProviderSupervisor,
    store: &Arc<Mutex<ProviderOperationStore>>,
    ownership: &dyn LiveSessionOwnershipCheck,
    target: &SessionRef,
    policy: LoadPolicy,
) -> Result<SessionQueuePlan, ProviderSessionLoadOutcome> {
    use provider_session_machines::session_lifecycle_machine::{LoadAttempt, SessionLifecycle};
    if let Some(snapshot) = supervisor.owner_session_snapshot(target)
        && let Some(run) = queue_run(&snapshot, policy)
    {
        return Ok(SessionQueuePlan::OnRun(run));
    }
    if policy == LoadPolicy::LoadedOnly {
        return Err(ProviderSessionLoadOutcome::NotLoaded);
    }
    let runtime = supervisor
        .runtime_for(&target.endpoint)
        .ok_or_else(|| unavailable("provider runtime is unavailable"))?;
    match checked_loadability(supervisor, &runtime, store, ownership, target).await {
        ProviderSessionLoadability::Loadable { .. } | ProviderSessionLoadability::AlreadyLoaded => {
        }
        ProviderSessionLoadability::UnsupportedLoad => {
            return Err(ProviderSessionLoadOutcome::UnsupportedLoad);
        }
        ProviderSessionLoadability::MissingRecord => {
            return Err(ProviderSessionLoadOutcome::MissingRecord);
        }
        ProviderSessionLoadability::LiveElsewhere => {
            return Err(ProviderSessionLoadOutcome::LiveElsewhere);
        }
        ProviderSessionLoadability::Unavailable { reason } => return Err(unavailable(reason)),
    }
    let owner = supervisor
        .session_owner_for(target)
        .map_err(|error| unavailable(error.to_string()))?;
    let before = owner.snapshots().borrow().clone();
    if let Some(run) = queue_run(&before, policy) {
        return Ok(SessionQueuePlan::OnRun(run));
    }
    let SessionLifecycle::NotLoaded { last } = before.lifecycle else {
        return Err(ProviderSessionLoadOutcome::NotLoaded);
    };
    let generation = last
        .number()
        .checked_add(1)
        .map(provider_session_machines::SessionGeneration::from_number)
        .ok_or_else(|| unavailable("the session has no remaining generation"))?;
    Ok(SessionQueuePlan::Restore {
        run: TransportRun {
            incarnation: before.owner.incarnation,
            generation,
        },
        attempt: LoadAttempt(uuid::Uuid::now_v7()),
    })
}

fn queue_run(
    snapshot: &provider_session_machines::session_owner::SessionSnapshot,
    policy: LoadPolicy,
) -> Option<TransportRun> {
    use provider_session_machines::session_lifecycle_machine::SessionLifecycle;
    let generation = match snapshot.lifecycle {
        SessionLifecycle::Live { generation } => generation,
        SessionLifecycle::Loading { generation, .. } if policy == LoadPolicy::MayLoad => generation,
        _ => return None,
    };
    Some(TransportRun {
        incarnation: snapshot.owner.incarnation,
        generation,
    })
}

fn owner_restore_outcome(
    result: provider_session_machines::session_owner::RestoreResult,
) -> ProviderSessionLoadOutcome {
    use provider_session_machines::session_owner::{
        RestoreDisposition, RestoreFailure as OwnerFailure, RestorePhase as OwnerPhase,
    };
    let phase = match result.phase {
        OwnerPhase::Preflight => RestorePhase::Preflight,
        OwnerPhase::Attempted => RestorePhase::Attempted,
    };
    match result.disposition {
        RestoreDisposition::Restored { .. } => ProviderSessionLoadOutcome::Ready,
        RestoreDisposition::Retired {
            failure: OwnerFailure::Closed,
            ..
        } => ProviderSessionLoadOutcome::ClosedWhileLoading { phase },
        RestoreDisposition::Retired { failure, .. }
            if let Some(coded) = failure.coded_evidence() =>
        {
            load_error_outcome(&RestoreFailure::attempted(coded.into()))
        }
        RestoreDisposition::Retired { failure, .. } => load_failure(phase, failure.next_step()),
    }
}
/// What a failed load means for the work that asked for it.
fn load_error_outcome(failure: &RestoreFailure) -> ProviderSessionLoadOutcome {
    let phase = failure.phase;
    match failure.error() {
        // A close won while this load was in flight: the connection
        // dropped the restored session.
        ExternalProviderRuntimeError::SessionReleased => {
            ProviderSessionLoadOutcome::ClosedWhileLoading { phase }
        }
        ExternalProviderRuntimeError::ProviderSessionNotFound {
            code,
            correlation_id,
        } => {
            let correlation_id = match DeliveryCorrelationId::try_from(correlation_id.to_string()) {
                Ok(correlation_id) => correlation_id,
                Err(_) => {
                    return load_failure(
                        phase,
                        "provider error reference was invalid, so retry this session",
                    );
                }
            };
            ProviderSessionLoadOutcome::Rejected {
                phase,
                reason: ProviderSessionLoadRejection::SessionNotFound {
                    code: *code,
                    correlation_id,
                },
            }
        }
        ExternalProviderRuntimeError::RestoreRefused {
            code,
            correlation_id,
        }
        | ExternalProviderRuntimeError::AuthenticationRequired {
            code,
            correlation_id,
        }
        | ExternalProviderRuntimeError::ProviderRejected {
            code,
            correlation_id,
        } => {
            let correlation_id = match DeliveryCorrelationId::try_from(correlation_id.to_string()) {
                Ok(correlation_id) => correlation_id,
                Err(_) => {
                    return load_failure(
                        phase,
                        "provider error reference was invalid, so retry this session",
                    );
                }
            };
            ProviderSessionLoadOutcome::Rejected {
                phase,
                reason: ProviderSessionLoadRejection::ProviderRejected {
                    code: *code,
                    correlation_id,
                },
            }
        }
        ExternalProviderRuntimeError::ResourceNotFound {
            code,
            correlation_id,
        } => coded_load_failure(
            phase,
            "provider resource was not found, so check the provider before retrying",
            *code,
            *correlation_id,
        ),
        ExternalProviderRuntimeError::RestoreFailed {
            code,
            correlation_id,
        } => coded_load_failure(
            phase,
            "the provider failed while restoring this session, so retry it",
            *code,
            *correlation_id,
        ),
        ExternalProviderRuntimeError::UnsupportedMethod {
            code,
            correlation_id,
        } => coded_load_failure(
            phase,
            "provider ACP method is unsupported, so check the provider's restore support before retrying",
            *code,
            *correlation_id,
        ),
        ExternalProviderRuntimeError::InvalidParams {
            code,
            correlation_id,
        } => coded_load_failure(
            phase,
            "provider ACP parameters are invalid, so check the session setup before retrying",
            *code,
            *correlation_id,
        ),
        ExternalProviderRuntimeError::RequestCancelled {
            code,
            correlation_id,
        } => coded_load_failure(
            phase,
            "provider ACP request was cancelled, so retry the restore when ready",
            *code,
            *correlation_id,
        ),
        error => {
            let reason = match (error, phase) {
                (ExternalProviderRuntimeError::SinkClosed | ExternalProviderRuntimeError::HistoryReplayUnavailable, RestorePhase::Attempted) =>
                    "the Router could not publish the restored session's replay or settings, so reconnect and retry this session".to_owned(),
                (ExternalProviderRuntimeError::RestoreNoAnswer | ExternalProviderRuntimeError::RestoreDisconnected, _)
                | (_, RestorePhase::Preflight) => error.to_string(),
                (_, RestorePhase::Attempted) => format!("{error}, so retry this session"),
            };
            load_failure(phase, reason)
        }
    }
}

/// Shared recovery wording for explicit restores and inputs waiting on a load.
pub(crate) fn load_error_reason(error: &RestoreFailure) -> String {
    match load_error_outcome(error) {
        ProviderSessionLoadOutcome::Ready | ProviderSessionLoadOutcome::AlreadyLoaded => {
            "the session couldn't be loaded".to_owned()
        }
        ProviderSessionLoadOutcome::NotLoaded => {
            collaboration_contracts::NOT_LOADED_REASON.to_owned()
        }
        ProviderSessionLoadOutcome::UnsupportedLoad => "unsupported: load".to_owned(),
        ProviderSessionLoadOutcome::MissingRecord => {
            "provider session record is missing".to_owned()
        }
        ProviderSessionLoadOutcome::LiveElsewhere => "session is live elsewhere".to_owned(),
        ProviderSessionLoadOutcome::Unavailable { reason, .. } => reason,
        ProviderSessionLoadOutcome::Rejected { reason, .. } => reason.safe_detail(),
        ProviderSessionLoadOutcome::ClosedWhileLoading { .. } => "session closed".to_owned(),
    }
}

pub(crate) fn closed_restore_reason(phase: RestorePhase) -> &'static str {
    match phase {
        RestorePhase::Preflight => "session closed",
        RestorePhase::Attempted => {
            "the session closed during restore, so retry this session when ready"
        }
    }
}

pub(crate) async fn inspect_provider_session_loadability(
    supervisor: &ExternalProviderSupervisor,
    runtime: &ExternalProviderRuntime,
    store: &Arc<Mutex<ProviderOperationStore>>,
    ownership: &dyn LiveSessionOwnershipCheck,
    target: &SessionRef,
) -> ProviderSessionLoadability {
    let provider_session_id = String::from(target.session_id.clone());
    match supervisor.owner_session_activity(target) {
        ProviderSessionActivity::Idle | ProviderSessionActivity::Running => {
            return ProviderSessionLoadability::AlreadyLoaded;
        }
        ProviderSessionActivity::NotLoaded => {}
    }
    if !runtime
        .capability_report(&provider_session_id)
        .await
        .supports_load
    {
        return ProviderSessionLoadability::UnsupportedLoad;
    }
    match ownership.check(target).await {
        Ok(LiveSessionOwnership::NotLive) => {}
        Ok(LiveSessionOwnership::LiveWritable | LiveSessionOwnership::LiveUnsupported) => {
            return ProviderSessionLoadability::LiveElsewhere;
        }
        Err(error) => return unavailable_loadability(error.to_string()),
    }
    let record = match store.lock().await.session_record(target).await {
        Ok(Some(record)) => record,
        Ok(None) => return ProviderSessionLoadability::MissingRecord,
        Err(error) => return unavailable_loadability(error.to_string()),
    };
    let working_directory = PathBuf::from(String::from(record.working_directory));
    let setup = runtime
        .session_setup()
        .restore_setup(&working_directory, &record.resume);
    ProviderSessionLoadability::Loadable {
        working_directory,
        setup,
    }
}

fn unavailable(reason: impl Into<String>) -> ProviderSessionLoadOutcome {
    load_failure(RestorePhase::Preflight, reason)
}

fn coded_load_failure(
    phase: RestorePhase,
    explanation: &'static str,
    provider_code: i64,
    correlation_id: acp_client_runtime::ProviderErrorCorrelationId,
) -> ProviderSessionLoadOutcome {
    let correlation_id = match DeliveryCorrelationId::try_from(correlation_id.to_string()) {
        Ok(correlation_id) => correlation_id,
        Err(_) => {
            return load_failure(
                phase,
                "provider error reference was invalid, so retry this session",
            );
        }
    };
    load_failure(
        phase,
        format!(
            "{explanation} (ACP code {provider_code}; reference {})",
            correlation_id.as_str()
        ),
    )
}

fn load_failure(phase: RestorePhase, reason: impl Into<String>) -> ProviderSessionLoadOutcome {
    ProviderSessionLoadOutcome::Unavailable {
        phase,
        reason: reason.into(),
    }
}

fn unavailable_loadability(reason: impl Into<String>) -> ProviderSessionLoadability {
    ProviderSessionLoadability::Unavailable {
        reason: reason.into(),
    }
}

#[cfg(test)]
#[path = "provider_acp_session_loading_test.rs"]
mod tests;
