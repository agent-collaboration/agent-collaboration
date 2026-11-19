//! The Worker's log sink for the org runtime's closed observations.

use collaboration_central_core::CentralObservation;

/// Modern org runtime sink; only the core closed classification crosses here.
pub(crate) fn observe_signin(observation: CentralObservation) {
    match observation {
        CentralObservation::EngineAlarmNotPublished { failure } => {
            worker::console_log!("central engine alarm not published: {failure:?}");
        }
        CentralObservation::EngineStoreDown { kind, key, failure } => {
            worker::console_log!(
                "central engine store down: kind={} key={} failure={failure:?}",
                kind.0,
                log_safe(&key.0)
            );
        }
        CentralObservation::EngineItemFailed {
            kind,
            key,
            failure,
            count,
            retired,
            workspaces,
        } => {
            worker::console_log!(
                "central engine: kind={} key={} class={} count={} retired={} workspaces={workspaces:?}",
                kind.0,
                log_safe(&key.0),
                failure.storage_tag(),
                count,
                retired
            );
        }
        CentralObservation::OrganizationStatementRefused {
            organization,
            refusal,
        } => {
            worker::console_log!(
                "central organization: organization={} refusal={refusal:?}",
                organization.as_str()
            );
        }
    }
}

fn log_safe(value: &str) -> &str {
    if value.is_empty() {
        "-"
    } else if value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        value
    } else {
        "unprintable"
    }
}
