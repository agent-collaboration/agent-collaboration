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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ProviderSessionLoadability {
    AlreadyLoaded,
    Loadable { working_directory: PathBuf },
    UnsupportedLoad,
    MissingRecord,
    LiveElsewhere,
    Unavailable { reason: String },
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
                    "this session never started a turn and did not survive the provider restart; create a new conversation (provider code {code}; reference {})",
                    correlation_id.as_str()
                )
            }
            Self::ProviderRejected {
                code,
                correlation_id,
            } => {
                format!(
                    "provider rejected the ACP operation (provider code {code}; reference {})",
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
    let working_directory = match inspect_provider_session_loadability(
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
        ProviderSessionLoadability::Loadable { working_directory } => working_directory,
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
        .load_session(provider_session_id, working_directory)
        .await
    {
        Ok(()) => ProviderSessionLoadOutcome::Ready,
        Err(ExternalProviderRuntimeError::ProviderSessionNotFound {
            code,
            correlation_id,
        }) => {
            let correlation_id = match DeliveryCorrelationId::try_from(correlation_id.to_string()) {
                Ok(correlation_id) => correlation_id,
                Err(_) => return unavailable("provider error reference was invalid"),
            };
            ProviderSessionLoadOutcome::Rejected {
                reason: ProviderSessionLoadRejection::SessionNotFound {
                    code,
                    correlation_id,
                },
            }
        }
        Err(ExternalProviderRuntimeError::AuthenticationRequired {
            code,
            correlation_id,
        })
        | Err(ExternalProviderRuntimeError::ProviderRejected {
            code,
            correlation_id,
        }) => {
            let correlation_id = match DeliveryCorrelationId::try_from(correlation_id.to_string()) {
                Ok(correlation_id) => correlation_id,
                Err(_) => return unavailable("provider error reference was invalid"),
            };
            ProviderSessionLoadOutcome::Rejected {
                reason: ProviderSessionLoadRejection::ProviderRejected {
                    code,
                    correlation_id,
                },
            }
        }
        Err(ExternalProviderRuntimeError::ResourceNotFound {
            code,
            correlation_id,
        }) => provider_error_unavailable("provider resource was not found", code, correlation_id),
        Err(ExternalProviderRuntimeError::UnsupportedMethod {
            code,
            correlation_id,
        }) => {
            provider_error_unavailable("provider ACP method is unsupported", code, correlation_id)
        }
        Err(ExternalProviderRuntimeError::InvalidParams {
            code,
            correlation_id,
        }) => {
            provider_error_unavailable("provider ACP parameters are invalid", code, correlation_id)
        }
        Err(ExternalProviderRuntimeError::RequestCancelled {
            code,
            correlation_id,
        }) => {
            provider_error_unavailable("provider ACP request was cancelled", code, correlation_id)
        }
        Err(error) => unavailable(error.to_string()),
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
    ProviderSessionLoadability::Loadable {
        working_directory: PathBuf::from(String::from(record.working_directory)),
    }
}

fn unavailable(reason: impl Into<String>) -> ProviderSessionLoadOutcome {
    ProviderSessionLoadOutcome::Unavailable {
        reason: reason.into(),
    }
}

fn provider_error_unavailable(
    explanation: &'static str,
    provider_code: i64,
    correlation_id: acp_client_runtime::ProviderErrorCorrelationId,
) -> ProviderSessionLoadOutcome {
    let correlation_id = match DeliveryCorrelationId::try_from(correlation_id.to_string()) {
        Ok(correlation_id) => correlation_id,
        Err(_) => return unavailable("provider error reference was invalid"),
    };
    unavailable(format!(
        "{explanation} (ACP code {provider_code}; reference {})",
        correlation_id.as_str()
    ))
}

fn unavailable_loadability(reason: impl Into<String>) -> ProviderSessionLoadability {
    ProviderSessionLoadability::Unavailable {
        reason: reason.into(),
    }
}

#[cfg(test)]
#[path = "provider_acp_session_loading_test.rs"]
mod tests;
