//! Captured client effects share the existing local/source owners. This adapter
//! chooses no fallback and performs no automatic operation reconciliation.
use super::{
    router_connection_registry::RouterConnectionTransport,
    source_ssh_protocol::{
        SourceManagedAction, SourceNativeClaudeAction, SourceOperation, SourceReadError,
        SourceRejection, SourceReply, SourceRequest,
    },
};
use crate::source_inventory_request::PickerSourceContext;
use collaboration_client::protocol::{OperationId, ProviderBindingIdentity, UuidIdentity};
use std::{future::Future, pin::Pin, sync::Arc};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionClientEffect {
    Native {
        service_epoch: UuidIdentity,
        action: SourceNativeClaudeAction,
    },
    Managed {
        operation_id: OperationId,
        binding: ProviderBindingIdentity,
        action: SourceManagedAction,
    },
    ReadManaged {
        service_epoch: UuidIdentity,
        operation_id: OperationId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapturedClientAction {
    pub attempt: u64,
    pub source: PickerSourceContext,
    pub service_id: UuidIdentity,
    pub effect: SessionClientEffect,
}

impl CapturedClientAction {
    pub fn source_label(&self) -> String {
        match &self.source {
            PickerSourceContext::ConfiguredHosted(profile) => profile.name.as_str().to_owned(),
            PickerSourceContext::DefaultHosted => "This machine".to_owned(),
            PickerSourceContext::LocalCodex => "Local Codex".to_owned(),
        }
    }
    pub fn unknown_description(&self) -> String {
        let source = self.source_label();
        match &self.effect {
            SessionClientEffect::Native { action, .. } => {
                let identity = match action {
                    SourceNativeClaudeAction::New { directory, .. } => {
                        format!("New in {}", directory.as_str())
                    }
                    SourceNativeClaudeAction::Resume {
                        target, directory, ..
                    } => format!(
                        "Resume {} in {}",
                        String::from(target.session_id.clone()),
                        directory.as_str()
                    ),
                    SourceNativeClaudeAction::Fork {
                        target, directory, ..
                    } => format!(
                        "Fork {} in {}",
                        String::from(target.session_id.clone()),
                        directory.as_str()
                    ),
                };
                format!(
                    "Native Claude {identity} on {source} (service {}) has unknown effect. Inspect native status on that source before starting again; no Router operation ID exists.",
                    String::from(self.service_id.clone())
                )
            }
            SessionClientEffect::Managed { operation_id, .. }
            | SessionClientEffect::ReadManaged { operation_id, .. } => {
                let operation_id = operation_id.as_str();
                format!(
                    "Managed action on {source} (service {}) has unknown effect. Retained operation {operation_id}; use read-only operation-show on that source. The action was not retried.",
                    String::from(self.service_id.clone())
                )
            }
        }
    }
}

pub type SessionClientExecutor = Arc<
    dyn Fn(
            CapturedClientAction,
        ) -> Pin<Box<dyn Future<Output = Result<SourceReply, SourceReadError>> + Send>>
        + Send
        + Sync,
>;

/// The caller has already ended the picker render loop and restored its terminal.
pub fn run_local_claude_terminal(
    request: CapturedClientAction,
    service_directory: &std::path::Path,
    home_directory: Option<&std::path::Path>,
    global_arguments: &[std::ffi::OsString],
) -> Result<(), super::SessionsCommandError> {
    use super::SessionsCommandError;
    if request.source != PickerSourceContext::DefaultHosted {
        return Err(SessionsCommandError::SessionSourceUnqualified);
    }
    let SessionClientEffect::Native {
        service_epoch,
        action,
    } = request.effect
    else {
        return Err(SessionsCommandError::SessionSourceUnqualified);
    };
    let resume = matches!(action, SourceNativeClaudeAction::Resume { .. });
    let request = SourceRequest {
        service_directory: service_directory
            .to_string_lossy()
            .into_owned()
            .try_into()
            .map_err(|_| SessionsCommandError::SessionSourceUnavailable)?,
        expected_service: request.service_id,
        operation: SourceOperation::NativeClaude {
            service_epoch,
            action,
        },
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(SessionsCommandError::Runtime)?;
    let command = runtime.block_on(super::source_ssh_executor::prepare_local_claude_terminal(
        &request,
        home_directory,
    ))?;
    drop(runtime);
    // Match the existing Codex terminal owner: the native child inherits this TTY.
    super::claude_terminal_launch::run_claude_terminal(command)?;
    if resume && !global_arguments.is_empty() {
        println!(
            "Opened with its saved settings; options given to agent-sessions apply to new sessions only."
        );
    }
    Ok(())
}

pub async fn execute_client_action(
    service_directory: Option<&std::path::Path>,
    codex_home: Option<&std::path::Path>,
    home_directory: Option<&std::path::Path>,
    executable_path: Option<&str>,
    request: CapturedClientAction,
) -> Result<SourceReply, SourceReadError> {
    if let PickerSourceContext::ConfiguredHosted(profile) = &request.source {
        if profile.service_id != request.service_id {
            return Err(SourceReadError::Rejected(SourceRejection::WrongService));
        }
        if !matches!(profile.transport, RouterConnectionTransport::Ssh(_)) {
            return Err(SourceReadError::Rejected(
                SourceRejection::UnsupportedConnection,
            ));
        }
        return match request.effect {
            SessionClientEffect::Native {
                service_epoch,
                action,
            } => {
                super::source_action_transport::submit_native_source(profile, service_epoch, action)
                    .await
                    .map(|result| SourceReply::NativeClaude {
                        result: Box::new(result),
                    })
            }
            SessionClientEffect::Managed {
                operation_id,
                binding,
                action,
            } => super::source_action_transport::submit_managed_source(
                profile,
                operation_id,
                binding,
                action,
            )
            .await
            .map(|result| SourceReply::ProviderAction {
                result: Box::new(result),
            }),
            SessionClientEffect::ReadManaged {
                service_epoch,
                operation_id,
            } => super::source_action_transport::read_managed_operation(
                profile,
                service_epoch,
                operation_id.clone(),
            )
            .await
            .map(|result| SourceReply::ProviderOperation {
                operation_id,
                result: Box::new(result),
            }),
        };
    }
    if request.source != PickerSourceContext::DefaultHosted {
        return Err(SourceReadError::Rejected(
            SourceRejection::UnsupportedConnection,
        ));
    }
    let directory = service_directory.ok_or(SourceReadError::Rejected(
        SourceRejection::SourceUnavailable,
    ))?;
    let operation = match request.effect {
        SessionClientEffect::Native {
            service_epoch,
            action,
        } => SourceOperation::NativeClaude {
            service_epoch,
            action,
        },
        SessionClientEffect::Managed {
            operation_id,
            binding,
            action,
        } => SourceOperation::ProviderAction {
            operation_id,
            binding,
            action,
        },
        SessionClientEffect::ReadManaged {
            service_epoch,
            operation_id,
        } => SourceOperation::ProviderOperationShow {
            service_epoch,
            operation_id,
        },
    };
    let request = SourceRequest {
        service_directory: directory
            .to_string_lossy()
            .into_owned()
            .try_into()
            .map_err(|_| SourceReadError::Rejected(SourceRejection::InvalidRequest))?,
        expected_service: request.service_id,
        operation,
    };
    if matches!(
        request.operation,
        SourceOperation::ProviderOperationShow { .. }
    ) {
        super::source_ssh_executor::execute_read(
            &request,
            codex_home,
            home_directory,
            executable_path,
        )
        .await
        .map_err(SourceReadError::Rejected)
    } else {
        super::source_ssh_executor::execute_client_mutation(&request, home_directory)
            .await
            .map_err(SourceReadError::Rejected)
    }
}

/// The selected source owns Stored/capability inspection; the UI never reads its store.
pub async fn read_provider_details(
    service_directory: Option<&std::path::Path>,
    codex_home: Option<&std::path::Path>,
    home_directory: Option<&std::path::Path>,
    executable_path: Option<&str>,
    source: &PickerSourceContext,
    binding: ProviderBindingIdentity,
    target: collaboration_client::protocol::SessionRef,
) -> Result<SourceReply, SourceReadError> {
    let expected_service = binding.endpoint.service_id.clone();
    if target.endpoint != binding.endpoint {
        return Err(SourceReadError::Rejected(SourceRejection::WrongService));
    }
    let operation = SourceOperation::ProviderInspect { binding, target };
    match source {
        PickerSourceContext::ConfiguredHosted(profile) => {
            if profile.service_id != expected_service {
                return Err(SourceReadError::Rejected(SourceRejection::WrongService));
            }
            super::source_ssh_transport::read_source(profile, operation).await
        }
        PickerSourceContext::DefaultHosted => {
            let directory = service_directory.ok_or(SourceReadError::Rejected(
                SourceRejection::SourceUnavailable,
            ))?;
            let request = SourceRequest {
                expected_service,
                operation,
                service_directory: directory
                    .to_string_lossy()
                    .into_owned()
                    .try_into()
                    .map_err(|_| SourceReadError::Rejected(SourceRejection::InvalidRequest))?,
            };
            super::source_ssh_executor::execute_read(
                &request,
                codex_home,
                home_directory,
                executable_path,
            )
            .await
            .map_err(SourceReadError::Rejected)
        }
        PickerSourceContext::LocalCodex => Err(SourceReadError::Rejected(
            SourceRejection::UnsupportedConnection,
        )),
    }
}
