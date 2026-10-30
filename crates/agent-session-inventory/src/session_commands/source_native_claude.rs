//! Native source execution returns observations, never Router operation records.
//! The routed native launcher owns arguments/environment; missing output is not
//! evidence that a background job was absent and never triggers another launch.
use super::{SourceRejection, SourceReply, SourceRequest, connect_expected};
use crate::sessions::{
    claude_launch_target::ClaudeLaunchTarget,
    source_ssh_protocol::{
        SourceNativeBinding, SourceNativeClaudeAction, SourceNativeClaudeIdentity,
        SourceNativeClaudeResult, SourceNativeExitStatus, SourceNativeObservation,
        SourceNativeUnknownReason, SourceOperation,
    },
};
use collaboration_client::protocol::{
    NativeSessionScope, NativeSessionSource, ProviderSessionListParams, ProviderSessionSummary,
    SessionRef, UuidIdentity,
};
use std::{
    collections::BTreeSet,
    ffi::OsString,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

pub async fn execute_native_claude(
    request: &SourceRequest,
    home_directory: Option<&Path>,
) -> Result<SourceReply, SourceRejection> {
    let SourceOperation::NativeClaude {
        service_epoch,
        action,
    } = &request.operation
    else {
        return Err(SourceRejection::InvalidRequest);
    };
    let launch = ClaudeLaunchTarget::resolve_for_service(
        home_directory,
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
        validate_native_target(request, service_epoch, target, directory.as_str()).await?;
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
    directory: &str,
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
    let binding = crate::source_inventory_binding::SourceInventoryBinding::from_inventory(
        &request.expected_service,
        &inventory,
    )
    .map_err(|_| SourceRejection::UnavailableEndpoint)?;
    if binding.native_claude_endpoint.as_ref() != Some(&target.endpoint) {
        return Err(SourceRejection::UnavailableEndpoint);
    }
    UuidIdentity::try_from(String::from(target.session_id.clone()))
        .map_err(|_| SourceRejection::SessionUnavailable)?;
    for view in [
        crate::picker_runtime_status::NativeCatalogueView::Loaded,
        crate::picker_runtime_status::NativeCatalogueView::Stored,
    ] {
        if native_target_in_view(&client, target, directory, view).await? {
            return Ok(());
        }
    }
    Err(SourceRejection::SessionUnavailable)
}

async fn native_target_in_view(
    client: &collaboration_client::CollaborationClient,
    target: &SessionRef,
    directory: &str,
    view: crate::picker_runtime_status::NativeCatalogueView,
) -> Result<bool, SourceRejection> {
    use crate::sessions::source_inventory_failure::InventoryReadFailure;
    let mut cursor = None;
    let mut seen_cursors = BTreeSet::new();
    let mut seen_targets = BTreeSet::new();
    let mut found = false;
    loop {
        let page = match client
            .list_provider_sessions(ProviderSessionListParams {
                endpoint: target.endpoint.clone(),
                view: view.request_view(),
                scope: NativeSessionScope::Any,
                source: NativeSessionSource::Interactive,
                query: None,
                page_size: 100,
                cursor,
            })
            .await
        {
            Ok(page) => page,
            Err(error) => {
                return match InventoryReadFailure::from_client(&error) {
                    InventoryReadFailure::Unavailable(_) => Ok(false),
                    InventoryReadFailure::Invalid { .. }
                    | InventoryReadFailure::InvalidNative { .. } => {
                        Err(SourceRejection::SessionUnavailable)
                    }
                };
            }
        };
        if page.native_coverage.as_ref().is_some_and(|coverage| {
            coverage.conflicts.iter().any(|conflict| {
                conflict.target.endpoint != target.endpoint || conflict.target == *target
            })
        }) {
            return Err(SourceRejection::SessionUnavailable);
        }
        crate::picker_runtime_status::NativeLiveReadState::from_observation(
            view,
            page.native_coverage,
        )
        .map_err(|_| SourceRejection::SessionUnavailable)?;
        if page.endpoint != target.endpoint
            || page.sessions.len() > 100
            || page.skipped_records.unwrap_or(0) > 0
        {
            return Err(SourceRejection::SessionUnavailable);
        }
        for summary in &page.sessions {
            if !crate::sessions::SessionPickerRecord::validate_native_summary(summary, view)
                || summary.target().endpoint != target.endpoint
                || seen_targets.len() >= 4096
                || !seen_targets.insert(summary.target().clone())
            {
                return Err(SourceRejection::SessionUnavailable);
            }
            if summary.target() == target {
                let (working_directory, eligible) = match summary {
                    ProviderSessionSummary::ClaudeCodeInteractive {
                        working_directory, ..
                    } => (working_directory, true),
                    ProviderSessionSummary::ClaudeCodeBackground {
                        working_directory,
                        state,
                        peer,
                        ..
                    } => (
                        peer.as_ref()
                            .map_or(working_directory, |peer| &peer.working_directory),
                        !matches!(
                            state,
                            collaboration_client::protocol::NativeClaudeBackgroundState::Unknown { .. }
                        ) || peer.is_some(),
                    ),
                    ProviderSessionSummary::HostedProvider { .. } => {
                        return Err(SourceRejection::SessionUnavailable);
                    }
                };
                if String::from(working_directory.clone()) != directory || !eligible {
                    return Err(SourceRejection::SessionUnavailable);
                }
                found = true;
            }
        }
        match page.next_cursor {
            Some(next)
                if !next.is_empty()
                    && next.len() <= 1024
                    && seen_cursors.len() < 64
                    && seen_cursors.insert(next.clone()) =>
            {
                cursor = Some(next)
            }
            Some(_) => return Err(SourceRejection::SessionUnavailable),
            None => return Ok(found),
        }
    }
}

#[cfg(test)]
#[path = "source_native_inventory_tests.rs"]
mod inventory_tests;
