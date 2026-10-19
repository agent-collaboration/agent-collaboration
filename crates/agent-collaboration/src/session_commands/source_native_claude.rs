//! Native source execution returns observations, never Router operation records.
//! The routed native launcher owns arguments/environment; missing output is not
//! evidence that a background job was absent and never triggers another launch.
use super::{SourceRejection, SourceReply, SourceRequest, connect_expected};
use crate::{
    CliContext,
    sessions::{
        claude_launch_target::ClaudeLaunchTarget,
        source_ssh_protocol::{
            SourceNativeBinding, SourceNativeClaudeAction, SourceNativeClaudeIdentity,
            SourceNativeClaudeResult, SourceNativeExitStatus, SourceNativeObservation,
            SourceNativeUnknownReason, SourceOperation,
        },
    },
};
use collaboration_client::protocol::{
    NativeSessionScope, NativeSessionSource, NativeSessionView, ProviderSessionListParams,
    ProviderSessionSummary, SessionRef, UuidIdentity,
};
use std::{
    collections::BTreeSet,
    ffi::OsString,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

pub(super) async fn execute_native_claude(
    request: &SourceRequest,
    context: &CliContext,
) -> Result<SourceReply, SourceRejection> {
    let SourceOperation::NativeClaude {
        service_epoch,
        action,
    } = &request.operation
    else {
        return Err(SourceRejection::InvalidRequest);
    };
    let launch = ClaudeLaunchTarget::resolve_for_service(
        context,
        PathBuf::from(request.service_directory.as_str()),
    )
    .map_err(|_| SourceRejection::SourceUnavailable)?;
    validate_source(&launch, &request.expected_service, service_epoch).await?;
    let (identity, directory, arguments, target) = match action {
        SourceNativeClaudeAction::New {
            directory,
            arguments,
        } => (
            SourceNativeClaudeIdentity::New {
                directory: directory.clone(),
            },
            directory,
            arguments,
            None,
        ),
        SourceNativeClaudeAction::Resume {
            target,
            directory,
            arguments,
        } => (
            SourceNativeClaudeIdentity::Resume {
                target: target.clone(),
                directory: directory.clone(),
            },
            directory,
            arguments,
            Some(target),
        ),
        SourceNativeClaudeAction::Fork {
            target,
            directory,
            arguments,
        } => (
            SourceNativeClaudeIdentity::Fork {
                target: target.clone(),
                directory: directory.clone(),
            },
            directory,
            arguments,
            Some(target),
        ),
    };
    if !Path::new(directory.as_str()).is_dir() {
        return Err(SourceRejection::DirectoryUnavailable);
    }
    if let Some(target) = target {
        validate_native_target(request, service_epoch, target).await?;
    }
    let environment = launch
        .routed_environment()
        .await
        .map_err(|_| SourceRejection::SourceUnavailable)?;
    // A source replacement during preflight cannot retarget this launch.
    validate_source(&launch, &request.expected_service, service_epoch).await?;
    let mut passthrough = Vec::with_capacity(arguments.len() + 1);
    if matches!(action, SourceNativeClaudeAction::Fork { .. }) {
        passthrough.push(OsString::from("--fork-session"));
    }
    passthrough.extend_from_slice(arguments);
    let session_id = target.map(|target| String::from(target.session_id.clone()));
    let command = launch.command(
        session_id.as_deref(),
        &passthrough,
        Some(Path::new(directory.as_str())),
        environment,
    );
    let mut command = tokio::process::Command::from(command);
    command.stdin(Stdio::null()).kill_on_drop(true);
    let observation = match tokio::time::timeout(Duration::from_secs(30), command.output()).await {
        Ok(Ok(output)) => {
            let status = match output.status.code() {
                Some(code) => SourceNativeExitStatus::Exited { code },
                None => SourceNativeExitStatus::Terminated {
                    description: output.status.to_string(),
                },
            };
            if output.status.success() && output.stdout.iter().all(u8::is_ascii_whitespace) {
                SourceNativeObservation::Unknown {
                    reason: SourceNativeUnknownReason::MissingReceipt,
                    status: Some(status),
                    stdout: output.stdout,
                    stderr: output.stderr,
                }
            } else {
                SourceNativeObservation::Observed {
                    status,
                    stdout: output.stdout,
                    stderr: output.stderr,
                }
            }
        }
        Ok(Err(error)) => SourceNativeObservation::Unknown {
            reason: SourceNativeUnknownReason::OutputUnavailable {
                message: error.to_string(),
            },
            status: None,
            stdout: Vec::new(),
            stderr: Vec::new(),
        },
        Err(_) => SourceNativeObservation::Unknown {
            reason: SourceNativeUnknownReason::TimedOut,
            status: None,
            stdout: Vec::new(),
            stderr: Vec::new(),
        },
    };
    Ok(SourceReply::NativeClaude {
        result: Box::new(SourceNativeClaudeResult {
            source: SourceNativeBinding {
                service_id: request.expected_service.clone(),
                service_epoch: service_epoch.clone(),
            },
            action: identity,
            observation,
        }),
    })
}

async fn validate_source(
    launch: &ClaudeLaunchTarget,
    service: &UuidIdentity,
    epoch: &UuidIdentity,
) -> Result<(), SourceRejection> {
    let manifest = launch
        .service_manifest()
        .await
        .map_err(|_| SourceRejection::SourceUnavailable)?;
    if manifest.service_id != *service {
        return Err(SourceRejection::WrongService);
    }
    if manifest.service_epoch != *epoch {
        return Err(SourceRejection::StaleBinding);
    }
    Ok(())
}

async fn validate_native_target(
    request: &SourceRequest,
    epoch: &UuidIdentity,
    target: &SessionRef,
) -> Result<(), SourceRejection> {
    if target.endpoint.service_id != request.expected_service {
        return Err(SourceRejection::WrongService);
    }
    let client = connect_expected(request).await?;
    let inventory = client
        .list_endpoints()
        .await
        .map_err(|_| SourceRejection::SourceUnavailable)?;
    if inventory.service_epoch != *epoch {
        return Err(SourceRejection::StaleBinding);
    }
    let binding = crate::presentation::session_picker::SourceInventoryBinding::from_inventory(
        &request.expected_service,
        &inventory,
    )
    .map_err(|_| SourceRejection::UnavailableEndpoint)?;
    if binding.native_claude_endpoint.as_ref() != Some(&target.endpoint) {
        return Err(SourceRejection::UnavailableEndpoint);
    }
    let mut cursor = None;
    let mut seen_cursors = BTreeSet::new();
    let mut seen_targets = BTreeSet::new();
    let mut found = false;
    loop {
        let page = client
            .list_provider_sessions(ProviderSessionListParams {
                endpoint: target.endpoint.clone(),
                view: NativeSessionView::Loaded,
                scope: NativeSessionScope::Any,
                source: NativeSessionSource::Interactive,
                query: None,
                page_size: 100,
                cursor,
            })
            .await
            .map_err(|_| SourceRejection::SessionUnavailable)?;
        if page.endpoint != target.endpoint
            || page.sessions.len() > 100
            || page.skipped_records.unwrap_or(0) > 0
        {
            return Err(SourceRejection::WrongService);
        }
        for summary in &page.sessions {
            if !matches!(
                summary,
                ProviderSessionSummary::ClaudeCodeInteractive { .. }
            ) || summary.target().endpoint != target.endpoint
                || seen_targets.len() >= 4096
                || !seen_targets.insert(summary.target().clone())
            {
                return Err(SourceRejection::SessionUnavailable);
            }
        }
        found|=page.sessions.iter().any(|summary|matches!(summary,ProviderSessionSummary::ClaudeCodeInteractive {target:actual,..} if actual==target));
        match page.next_cursor {
            Some(next) if seen_cursors.len() < 64 && seen_cursors.insert(next.clone()) => {
                cursor = Some(next)
            }
            Some(_) => return Err(SourceRejection::SessionUnavailable),
            None => break,
        }
    }
    if found {
        Ok(())
    } else {
        Err(SourceRejection::SessionUnavailable)
    }
}

#[cfg(test)]
#[path = "source_native_inventory_tests.rs"]
mod inventory_tests;
