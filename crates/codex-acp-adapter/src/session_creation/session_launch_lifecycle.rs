//! Native transport consumes Host admission; the registrar retains every lifecycle decision.
use super::{NativeConnectionError, SessionSetupError, Value, map_native_failure};
use collaboration_contracts::{
    HarnessLocalSetupRefusal, HarnessRegistrationOutcome, HarnessSessionBinding,
    HarnessSessionSetup, PreparedHarnessSessionStart,
};
use collaboration_protocol::OperationId;
use message_board::SessionRef;

pub(super) fn prepared_start(
    setup: &HarnessSessionSetup,
    operation: Option<&OperationId>,
) -> Result<Option<PreparedHarnessSessionStart>, SessionSetupError> {
    match setup {
        HarnessSessionSetup::AccessEnrollment => Ok(None),
        HarnessSessionSetup::Start(prepared) if Some(&prepared.operation_id) == operation => {
            Ok(Some(prepared.clone()))
        }
        HarnessSessionSetup::Start(_) | HarnessSessionSetup::Restore(_) => {
            Err(SessionSetupError::InvalidParameters)
        }
    }
}

pub(super) fn restored_binding(
    setup: &HarnessSessionSetup,
    session_id: &str,
) -> Result<Option<HarnessSessionBinding>, SessionSetupError> {
    match setup {
        HarnessSessionSetup::AccessEnrollment => Ok(None),
        HarnessSessionSetup::Restore(restored)
            if restored.session.session_id.as_str() == session_id =>
        {
            Ok(Some(restored.binding.clone()))
        }
        HarnessSessionSetup::Start(_) | HarnessSessionSetup::Restore(_) => {
            Err(SessionSetupError::InvalidParameters)
        }
    }
}

pub(super) async fn mark_dispatched(
    prepared: Option<&PreparedHarnessSessionStart>,
) -> Result<(), SessionSetupError> {
    if let Some(prepared) = prepared {
        prepared
            .port
            .mark_dispatched(&prepared.operation_id)
            .await
            .map_err(|_| SessionSetupError::Unavailable)?;
    }
    Ok(())
}

pub(super) async fn native_failure(
    prepared: Option<&PreparedHarnessSessionStart>,
    failure: NativeConnectionError,
) -> SessionSetupError {
    let Some(prepared) = prepared else {
        return map_native_failure(failure);
    };
    match failure {
        NativeConnectionError::InvalidInput
        | NativeConnectionError::Unavailable
        | NativeConnectionError::Rejected { .. } => {
            if prepared
                .port
                .creation_refused(&prepared.operation_id)
                .await
                .is_err()
            {
                return SessionSetupError::OutcomeUnknown;
            }
            map_native_failure(failure)
        }
        NativeConnectionError::UnavailableWithCause(_)
        | NativeConnectionError::OutcomeUnknown
        | NativeConnectionError::OutcomeUnknownWithCause(_)
        | NativeConnectionError::Protocol => SessionSetupError::OutcomeUnknown,
    }
}

pub(super) async fn record_created(
    prepared: Option<&PreparedHarnessSessionStart>,
    response: &Value,
) -> Result<Option<String>, SessionSetupError> {
    let Some(prepared) = prepared else {
        return Ok(None);
    };
    let session_id = response
        .pointer("/thread/id")
        .and_then(Value::as_str)
        .ok_or(SessionSetupError::OutcomeUnknown)?;
    let session = SessionRef {
        endpoint: prepared.endpoint.clone(),
        session_id: session_id
            .to_owned()
            .try_into()
            .map_err(|_| SessionSetupError::OutcomeUnknown)?,
    };
    prepared
        .port
        .record_created(&prepared.operation_id, &session)
        .await
        .map_err(|_| SessionSetupError::OutcomeUnknown)?;
    Ok(Some(session_id.to_owned()))
}

pub(super) async fn refuse_setup(
    prepared: Option<&PreparedHarnessSessionStart>,
    cause: HarnessLocalSetupRefusal,
) -> Result<(), SessionSetupError> {
    if let Some(prepared) = prepared {
        prepared
            .port
            .refuse_setup(&prepared.operation_id, cause)
            .await
            .map_err(|_| SessionSetupError::OutcomeUnknown)?;
    }
    Ok(())
}

pub(super) async fn complete_setup(
    prepared: Option<&PreparedHarnessSessionStart>,
) -> Result<(), SessionSetupError> {
    let Some(prepared) = prepared else {
        return Ok(());
    };
    match prepared.port.complete_setup(&prepared.operation_id).await {
        Ok(HarnessRegistrationOutcome::Registered) => Ok(()),
        Ok(HarnessRegistrationOutcome::Refused(_)) => Err(SessionSetupError::RegistrationRefused),
        Ok(HarnessRegistrationOutcome::Pending) | Err(_) => Err(SessionSetupError::OutcomeUnknown),
    }
}
