//! The fixture observes the actual parent wait result before the existing mapper consumes it.
use super::QuotaLoginResult;
use serde::Serialize;
use std::{
    future::Future,
    io,
    os::unix::process::ExitStatusExt,
    process::ExitStatus,
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub(crate) enum LoginWaitStatus {
    Exited {
        code: i32,
        #[serde(rename = "rawStatus")]
        raw_status: i32,
    },
    Signaled {
        signal: i32,
        #[serde(rename = "rawStatus")]
        raw_status: i32,
    },
    WaitFailed {
        message: String,
    },
}
impl LoginWaitStatus {
    fn from_wait(result: &io::Result<ExitStatus>) -> Self {
        match result {
            Ok(status) => match (status.code(), status.signal()) {
                (Some(code), _) => Self::Exited {
                    code,
                    raw_status: status.into_raw(),
                },
                (_, Some(signal)) => Self::Signaled {
                    signal,
                    raw_status: status.into_raw(),
                },
                _ => Self::WaitFailed {
                    message: format!(
                        "child wait returned neither an exit code nor a signal: {status}"
                    ),
                },
            },
            Err(error) => Self::WaitFailed {
                message: error.to_string(),
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoginParentExit {
    pub(crate) child_pid: Option<u32>,
    pub(crate) wait_ordinal: usize,
    pub(crate) status: LoginWaitStatus,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum LoginWaitObservation {
    ParentWaitReturned(LoginParentExit),
    MappingConsumed(LoginWaitStatus),
    MappingReturned(QuotaLoginResult),
}

type ParentExitSink = Arc<dyn Fn(&LoginParentExit) -> Result<(), String> + Send + Sync>;
#[derive(Default)]
struct ObservationState {
    events: Vec<LoginWaitObservation>,
    receipt_errors: Vec<String>,
}
#[derive(Clone, Default)]
pub(crate) struct LoginWaitObserver {
    state: Arc<Mutex<ObservationState>>,
    sink: Option<ParentExitSink>,
}
tokio::task_local! { static ACTIVE_LOGIN_WAIT_OBSERVER: LoginWaitObserver; }

impl LoginWaitObserver {
    pub(crate) fn with_parent_exit_sink(sink: ParentExitSink) -> Self {
        Self {
            sink: Some(sink),
            ..Default::default()
        }
    }
    pub(crate) async fn scope<T>(&self, future: impl Future<Output = T>) -> T {
        ACTIVE_LOGIN_WAIT_OBSERVER.scope(self.clone(), future).await
    }
    pub(crate) fn observations(&self) -> Result<Vec<LoginWaitObservation>, String> {
        Ok(self
            .state
            .lock()
            .map_err(|_| "login wait ledger poisoned")?
            .events
            .clone())
    }
    pub(crate) fn validated_parent_exit(&self) -> Result<LoginParentExit, String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "login wait ledger poisoned")?;
        if !state.receipt_errors.is_empty() {
            return Err(format!(
                "parent exit receipt failed: {:?}",
                state.receipt_errors
            ));
        }
        let [
            LoginWaitObservation::ParentWaitReturned(parent),
            LoginWaitObservation::MappingConsumed(consumed),
            LoginWaitObservation::MappingReturned(_),
        ] = state.events.as_slice()
        else {
            return Err(format!(
                "one parent wait must precede its mapping input and result: {:?}",
                state.events
            ));
        };
        if parent.wait_ordinal != 1 || parent.child_pid.is_none() || &parent.status != consumed {
            return Err(
                "parent wait identity/count/status differs from the actual mapper input".to_owned(),
            );
        }
        Ok(parent.clone())
    }
}

pub(crate) fn parent_wait_returned(child_pid: Option<u32>, result: &io::Result<ExitStatus>) {
    let _ = ACTIVE_LOGIN_WAIT_OBSERVER.try_with(|observer| {
        let parent = {
            let Ok(mut state) = observer.state.lock() else {
                return;
            };
            let wait_ordinal = state
                .events
                .iter()
                .filter(|event| matches!(event, LoginWaitObservation::ParentWaitReturned(_)))
                .count()
                + 1;
            let parent = LoginParentExit {
                child_pid,
                wait_ordinal,
                status: LoginWaitStatus::from_wait(result),
            };
            state
                .events
                .push(LoginWaitObservation::ParentWaitReturned(parent.clone()));
            parent
        };
        if let Some(sink) = &observer.sink
            && let Err(error) = sink(&parent)
            && let Ok(mut state) = observer.state.lock()
        {
            state.receipt_errors.push(error);
        }
    });
}
pub(crate) fn mapping_consumed(result: &io::Result<ExitStatus>) {
    let _ = ACTIVE_LOGIN_WAIT_OBSERVER.try_with(|observer| {
        let Ok(mut state) = observer.state.lock() else {
            return;
        };
        state.events.push(LoginWaitObservation::MappingConsumed(
            LoginWaitStatus::from_wait(result),
        ));
    });
}
pub(crate) fn mapping_returned(result: &QuotaLoginResult) {
    let _ = ACTIVE_LOGIN_WAIT_OBSERVER.try_with(|observer| {
        let Ok(mut state) = observer.state.lock() else {
            return;
        };
        state
            .events
            .push(LoginWaitObservation::MappingReturned(result.clone()));
    });
}

#[path = "quota_login_wait_observer_contract_test.rs"]
mod tests;
