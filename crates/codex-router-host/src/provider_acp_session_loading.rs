//! Load a recorded provider session only after checking live peer ownership.
use crate::{
    ExternalProviderRuntime, ExternalProviderRuntimeError, ExternalProviderSupervisor,
    LiveSessionOwnership, LiveSessionOwnershipCheck, ProviderSessionActivity,
};
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
        reason: ProviderSessionLoadRejection,
    },
    Unavailable {
        reason: String,
    },
    /// The admitted restore failed, so the original input ends unsent.
    RestoreFailed {
        reason: String,
    },
    /// A close of the session won while this load was in flight: the
    /// connection dropped the restored session, and the work that asked for
    /// the load ends not sent. A later load starts a new run.
    ClosedWhileLoading,
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
    let provider_session_id = String::from(target.session_id.clone());
    // One mode operation at a time per session: the record read and the mode
    // the restore puts back wait for any explicit change still recording,
    // and a change waits for the restore (review F3).
    let _mode_turn = runtime
        .session_setup()
        .mode_order()
        .enter(&provider_session_id)
        .await;
    let (working_directory, setup) = match inspect_provider_session_loadability(
        runtime.as_ref(),
        store,
        ownership,
        target,
    )
    .await
    {
        ProviderSessionLoadability::AlreadyLoaded => {
            return match load_policy {
                LoadPolicy::MayLoad => ProviderSessionLoadOutcome::Ready,
                LoadPolicy::LoadedOnly => ProviderSessionLoadOutcome::AlreadyLoaded,
            };
        }
        ProviderSessionLoadability::Loadable { .. } if load_policy == LoadPolicy::LoadedOnly => {
            return ProviderSessionLoadOutcome::NotLoaded;
        }
        ProviderSessionLoadability::Loadable {
            working_directory,
            setup,
        } => (working_directory, setup),
        ProviderSessionLoadability::UnsupportedLoad => {
            return ProviderSessionLoadOutcome::UnsupportedLoad;
        }
        ProviderSessionLoadability::MissingRecord => {
            return ProviderSessionLoadOutcome::MissingRecord;
        }
        ProviderSessionLoadability::LiveElsewhere => {
            return ProviderSessionLoadOutcome::LiveElsewhere;
        }
        ProviderSessionLoadability::Unavailable { reason } => {
            return ProviderSessionLoadOutcome::Unavailable { reason };
        }
    };
    match runtime
        .load_session_with_setup(provider_session_id, working_directory, setup)
        .await
    {
        Ok(()) => ProviderSessionLoadOutcome::Ready,
        Err(error) => load_error_outcome(error),
    }
}

/// What a failed load means for the work that asked for it.
fn load_error_outcome(error: ExternalProviderRuntimeError) -> ProviderSessionLoadOutcome {
    match error {
        // A close won while this load was in flight: the connection
        // dropped the restored session.
        ExternalProviderRuntimeError::SessionReleased => {
            ProviderSessionLoadOutcome::ClosedWhileLoading
        }
        ExternalProviderRuntimeError::ProviderSessionNotFound {
            code,
            correlation_id,
        } => {
            let correlation_id = match DeliveryCorrelationId::try_from(correlation_id.to_string()) {
                Ok(correlation_id) => correlation_id,
                Err(_) => return restore_failed("provider error reference was invalid"),
            };
            ProviderSessionLoadOutcome::Rejected {
                reason: ProviderSessionLoadRejection::SessionNotFound {
                    code,
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
                Err(_) => return restore_failed("provider error reference was invalid"),
            };
            ProviderSessionLoadOutcome::Rejected {
                reason: ProviderSessionLoadRejection::ProviderRejected {
                    code,
                    correlation_id,
                },
            }
        }
        ExternalProviderRuntimeError::ResourceNotFound {
            code,
            correlation_id,
        } => provider_error_restore_failed("provider resource was not found", code, correlation_id),
        ExternalProviderRuntimeError::RestoreFailed {
            code,
            correlation_id,
        } => provider_error_restore_failed(
            "the provider failed while restoring this session, so retry it",
            code,
            correlation_id,
        ),
        ExternalProviderRuntimeError::UnsupportedMethod {
            code,
            correlation_id,
        } => provider_error_restore_failed(
            "provider ACP method is unsupported, so check the provider's restore support before retrying",
            code,
            correlation_id,
        ),
        ExternalProviderRuntimeError::InvalidParams {
            code,
            correlation_id,
        } => provider_error_restore_failed(
            "provider ACP parameters are invalid, so check the session setup before retrying",
            code,
            correlation_id,
        ),
        ExternalProviderRuntimeError::RequestCancelled {
            code,
            correlation_id,
        } => provider_error_restore_failed(
            "provider ACP request was cancelled, so retry the restore when ready",
            code,
            correlation_id,
        ),
        error @ (ExternalProviderRuntimeError::RestoreNoAnswer
        | ExternalProviderRuntimeError::RestoreDisconnected
        | ExternalProviderRuntimeError::HistoryReplayUnavailable) => {
            restore_failed(error.to_string())
        }
        // A local admission refusal (for example, an overlapping load or
        // unavailable history sink) keeps the pre-dispatch disposition.
        error => unavailable(error.to_string()),
    }
}

/// The reason a queued message ends not sent when its load failed.
pub(crate) fn load_error_reason(error: ExternalProviderRuntimeError) -> String {
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
        ProviderSessionLoadOutcome::Unavailable { reason }
        | ProviderSessionLoadOutcome::RestoreFailed { reason } => reason,
        ProviderSessionLoadOutcome::Rejected { reason } => reason.safe_detail(),
        ProviderSessionLoadOutcome::ClosedWhileLoading => {
            crate::provider_acp_message_fifo::SESSION_CLOSED.to_owned()
        }
    }
}

pub(crate) async fn inspect_provider_session_loadability(
    runtime: &ExternalProviderRuntime,
    store: &Arc<Mutex<ProviderOperationStore>>,
    ownership: &dyn LiveSessionOwnershipCheck,
    target: &SessionRef,
) -> ProviderSessionLoadability {
    let provider_session_id = String::from(target.session_id.clone());
    match runtime.session_activity(provider_session_id.clone()).await {
        Ok(ProviderSessionActivity::Idle | ProviderSessionActivity::Running) => {
            return ProviderSessionLoadability::AlreadyLoaded;
        }
        Ok(ProviderSessionActivity::NotLoaded) => {}
        Err(error) => return unavailable_loadability(error.to_string()),
    }
    if !runtime
        .capability_report(&provider_session_id)
        .await
        .supports_load
    {
        return ProviderSessionLoadability::UnsupportedLoad;
    }
    let record = match store.lock().await.session_record(target).await {
        Ok(Some(record)) => record,
        Ok(None) => return ProviderSessionLoadability::MissingRecord,
        Err(error) => return unavailable_loadability(error.to_string()),
    };
    match ownership.check(target).await {
        Ok(LiveSessionOwnership::NotLive) => {}
        Ok(LiveSessionOwnership::LiveWritable | LiveSessionOwnership::LiveUnsupported) => {
            return ProviderSessionLoadability::LiveElsewhere;
        }
        Err(error) => return unavailable_loadability(error.to_string()),
    }
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
    ProviderSessionLoadOutcome::Unavailable {
        reason: reason.into(),
    }
}

fn provider_error_restore_failed(
    explanation: &'static str,
    provider_code: i64,
    correlation_id: acp_client_runtime::ProviderErrorCorrelationId,
) -> ProviderSessionLoadOutcome {
    let correlation_id = match DeliveryCorrelationId::try_from(correlation_id.to_string()) {
        Ok(correlation_id) => correlation_id,
        Err(_) => return restore_failed("provider error reference was invalid"),
    };
    restore_failed(format!(
        "{explanation} (ACP code {provider_code}; reference {})",
        correlation_id.as_str()
    ))
}

fn restore_failed(reason: impl Into<String>) -> ProviderSessionLoadOutcome {
    ProviderSessionLoadOutcome::RestoreFailed {
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
