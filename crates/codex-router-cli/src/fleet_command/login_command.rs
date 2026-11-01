//! The CLI displays the Host's code; credentials and polling stay inside that Host.
use std::{io::Write, path::Path, time::Duration};

use codex_router_host::{
    HostCoordinationPaths, OperatorFrame, OperatorProtocolError, OperatorRequest,
    fleet_machine_login::{
        FleetLoginEvent, FleetLoginRefusal, FleetLoginUnknown, RefreshCredentialKeeping,
    },
};
use thiserror::Error;

use super::arguments::LoginArguments;
use crate::host_command::operator_client::{OperatorClientError, send_operator_request_streaming};

#[derive(Debug, Error)]
pub enum FleetLoginCommandError {
    #[error("{0}")]
    InvalidInput(&'static str),
    #[error(transparent)]
    Operator(#[from] OperatorClientError),
    #[error("{}", refusal_message(.0))]
    Refused(FleetLoginRefusal),
    #[error("fleet login outcome unknown; run codex-router fleet login again")]
    OutcomeUnknown(FleetLoginUnknown),
    #[error("login interrupted, run it again")]
    Interrupted,
    #[error("could not write Fleet login output")]
    Output,
    #[error("the Host returned an unexpected Fleet login response")]
    UnexpectedFrame,
}

pub(super) async fn run(
    arguments: LoginArguments,
    stdout: &mut (impl Write + Send),
) -> Result<(), FleetLoginCommandError> {
    let root = crate::router_root_or_default(arguments.router_root).map_err(|_| {
        FleetLoginCommandError::InvalidInput("Router root is unavailable; pass --router-root")
    })?;
    if !root.is_absolute() {
        return Err(FleetLoginCommandError::InvalidInput(
            "--router-root must be absolute",
        ));
    }
    let organization_id = arguments.organization.try_into().map_err(|_| {
        FleetLoginCommandError::InvalidInput("--organization must be a canonical organization ID")
    })?;
    let label = arguments.label.try_into().map_err(|_| {
        FleetLoginCommandError::InvalidInput("--label must be a valid machine label")
    })?;
    let paths = HostCoordinationPaths::for_router_root(&root);
    exchange(
        paths.operator_socket(),
        OperatorRequest::FleetLogin {
            organization_id,
            label,
        },
        stdout,
    )
    .await
}

async fn exchange(
    socket: &Path,
    request: OperatorRequest,
    stdout: &mut (impl Write + Send),
) -> Result<(), FleetLoginCommandError> {
    let status = send_operator_request_streaming(
        socket,
        OperatorRequest::Status,
        Duration::from_secs(30),
        |_| {},
    )
    .await;
    let status = match status {
        Err(OperatorClientError::Protocol(OperatorProtocolError::VersionMismatch { .. })) => {
            return Err(unsupported());
        }
        other => other?,
    };
    match status.last() {
        Some(OperatorFrame::Terminal(response)) if response.snapshot().supports_fleet_login() => {}
        Some(OperatorFrame::Terminal(_)) => return Err(unsupported()),
        _ => return Err(FleetLoginCommandError::UnexpectedFrame),
    }
    let mut output_failed = false;
    // Codes live at most ten minutes; the exchange bound leaves room for the
    // final issuer response. It does not own the Host's polling lifetime.
    let frames =
        send_operator_request_streaming(socket, request, Duration::from_secs(660), |frame| {
            if let OperatorFrame::FleetLogin(event) = frame
                && render_display(stdout, event).is_err()
            {
                output_failed = true;
            }
        })
        .await
        .map_err(|error| match error {
            OperatorClientError::MissingTerminal => FleetLoginCommandError::Interrupted,
            other => FleetLoginCommandError::Operator(other),
        })?;
    if output_failed {
        return Err(FleetLoginCommandError::Output);
    }
    match frames.last() {
        Some(OperatorFrame::FleetLogin(FleetLoginEvent::LoggedIn { .. })) => Ok(()),
        Some(OperatorFrame::FleetLogin(FleetLoginEvent::Refused { reason })) => {
            Err(FleetLoginCommandError::Refused(reason.clone()))
        }
        Some(OperatorFrame::FleetLogin(FleetLoginEvent::OutcomeUnknown { context })) => {
            Err(FleetLoginCommandError::OutcomeUnknown(context.clone()))
        }
        _ => Err(FleetLoginCommandError::UnexpectedFrame),
    }
}

fn unsupported() -> FleetLoginCommandError {
    FleetLoginCommandError::Refused(FleetLoginRefusal::HostUnsupported {})
}

fn render_display(stdout: &mut impl Write, event: &FleetLoginEvent) -> std::io::Result<()> {
    match event {
        FleetLoginEvent::Code {
            user_code,
            verification_uri,
            keeping,
        } => {
            writeln!(stdout, "Open {}", verification_uri.as_str())?;
            writeln!(stdout, "Enter code: {}", user_code.as_str())?;
            writeln!(stdout, "{}", keeping_message(*keeping))?;
        }
        FleetLoginEvent::LoggedIn {
            organization_id,
            enrollment_id,
            keeping,
        } => {
            writeln!(
                stdout,
                "Logged in to organization {organization_id} (enrollment {enrollment_id})."
            )?;
            writeln!(stdout, "{}", keeping_message(*keeping))?;
        }
        FleetLoginEvent::Refused { .. } | FleetLoginEvent::OutcomeUnknown { .. } => {}
    }
    stdout.flush()
}

fn keeping_message(keeping: RefreshCredentialKeeping) -> &'static str {
    match keeping {
        RefreshCredentialKeeping::EncryptedStore => {
            "Refresh credential kept in the encrypted store."
        }
        RefreshCredentialKeeping::MemoryOnly => {
            "Kept in memory: approve this machine again after a restart."
        }
    }
}

fn refusal_message(reason: &FleetLoginRefusal) -> String {
    match reason {
        FleetLoginRefusal::HostUnsupported {} => {
            "this host doesn't support fleet login; upgrade codex-router".to_owned()
        }
        FleetLoginRefusal::NotConfigured {} => {
            "fleet login is not configured on this Host".to_owned()
        }
        FleetLoginRefusal::StoreUnusable { cause } => {
            format!("fleet login refused: encrypted credential store is unusable ({cause:?})")
        }
        FleetLoginRefusal::AlreadyInOrganization { organization_id } => {
            format!("this installation is already saved in organization {organization_id}")
        }
        FleetLoginRefusal::AccessDenied {} => "fleet login was denied".to_owned(),
        FleetLoginRefusal::CodeExpired {} => {
            "fleet login code expired; run codex-router fleet login again".to_owned()
        }
        FleetLoginRefusal::IssuerUnavailable {} => {
            "the sign-in service is unavailable; run codex-router fleet login again".to_owned()
        }
        FleetLoginRefusal::InvalidIssuerResponse {} => {
            "the sign-in service returned an invalid login response".to_owned()
        }
        FleetLoginRefusal::CredentialWriteFailed {} => {
            "the Host could not keep the refresh credential".to_owned()
        }
        FleetLoginRefusal::ConnectionUnavailable {} => {
            "the Host machine connection is unavailable".to_owned()
        }
    }
}
