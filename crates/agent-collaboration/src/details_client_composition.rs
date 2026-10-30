use crate::CliContext;
use agent_session_inventory::sessions::source_ssh_protocol::{
    SourceNativeBinding, SourceReadError,
};
use agent_session_inventory::source_inventory_binding::SourceInventoryBinding;
use agent_session_picker::session_picker::details_client_inventory::{
    DetailsClientCapabilities, DetailsClientInspector, ManagedRestoreActions,
};
use collaboration_client::protocol::ProviderBindingIdentity;
use std::sync::Arc;
pub(crate) fn production_inspector(context: CliContext) -> DetailsClientInspector {
    let service_directory =
        crate::session_environment::collaboration_service_directory(&context).ok();
    let codex_home = crate::session_environment::codex_home(&context).ok();
    let home_directory = context.env_var("HOME").map(std::path::PathBuf::from);
    let executable_path = context.env_var("PATH").map(str::to_owned);

    Arc::new(move |query| {
        let service_directory = service_directory.clone();
        let codex_home = codex_home.clone();
        let home_directory = home_directory.clone();
        let executable_path = executable_path.clone();
        Box::pin(async move {
            use agent_session_inventory::sessions::{
                SessionPickerIdentity,
                source_ssh_protocol::{SourceNativeLaunchAvailability, SourceReply},
            };
            use collaboration_client::protocol::{
                ChannelDescription, CodexGeneration, ProviderCapabilityName,
                ProviderCapabilityStatus, ProviderSessionState,
            };
            let (expected, inventory, native_claude) =
                crate::session_client_composition::read_client_inventory(
                    service_directory.as_deref(),
                    codex_home.as_deref(),
                    home_directory.as_deref(),
                    executable_path.as_deref(),
                    query.source.clone(),
                )
                .await?;
            let membership = SourceInventoryBinding::from_inventory(&expected, &inventory)
                .map_err(|_| SourceReadError::InvalidReply)?;
            match &query.record.identity {
                SessionPickerIdentity::NativeClaude(target) => {
                    if !query.record.native_restore_eligible() {
                        return Ok(DetailsClientCapabilities::Unavailable(
                            "Native job lifecycle is unknown; restore unavailable".to_owned(),
                        ));
                    }
                    if membership.native_claude_endpoint.as_ref() != Some(&target.endpoint) {
                        return Err(SourceReadError::InvalidReply);
                    }
                    if query.record.cwd.as_ref().is_none_or(|directory| {
                        agent_session_inventory::sessions::router_connection_registry::AbsoluteRemoteCwd::try_from(
                            directory.clone(),
                        )
                        .is_err()
                    }) {
                        return Ok(DetailsClientCapabilities::Unavailable(
                            "Source session working directory is unavailable".to_owned(),
                        ));
                    }
                    match native_claude {
                        SourceNativeLaunchAvailability::Ready { .. } => {
                            Ok(DetailsClientCapabilities::Native {
                                source: SourceNativeBinding {
                                    service_id: expected,
                                    service_epoch: inventory.service_epoch,
                                },
                            })
                        }
                        SourceNativeLaunchAvailability::Unavailable { message } => {
                            Ok(DetailsClientCapabilities::Unavailable(message))
                        }
                    }
                }
                SessionPickerIdentity::HostedProvider(target) => {
                    let entry = inventory
                        .endpoints
                        .iter()
                        .find(|entry| entry.endpoint == target.endpoint)
                        .ok_or(SourceReadError::InvalidReply)?;
                    if !matches!(
                        entry.availability,
                        collaboration_client::protocol::EndpointAvailability::Available { .. }
                    ) {
                        return Ok(DetailsClientCapabilities::Unavailable(
                            "Source provider endpoint is unavailable".to_owned(),
                        ));
                    }
                    let mut bindings = entry.channels.iter().filter_map(|channel| match channel {
                        ChannelDescription::ExternalProvider {
                            transport,
                            binding_id,
                            binding_generation,
                            runtime,
                            capabilities,
                        } => Some(ProviderBindingIdentity {
                            endpoint: entry.endpoint.clone(),
                            binding_id: binding_id.clone(),
                            transport: *transport,
                            runtime: runtime.clone(),
                            generation: CodexGeneration {
                                service_epoch: inventory.service_epoch.clone(),
                                generation: *binding_generation,
                            },
                            capabilities: capabilities.clone(),
                        }),
                        _ => None,
                    });
                    let binding = bindings.next().ok_or(SourceReadError::InvalidReply)?;
                    if bindings.next().is_some() {
                        return Err(SourceReadError::InvalidReply);
                    }
                    let reply = agent_session_inventory::sessions::source_client_action::read_provider_details(
                        service_directory.as_deref(),
                        codex_home.as_deref(), home_directory.as_deref(), executable_path.as_deref(),
                        &query.source,
                        binding.clone(),
                        target.clone(),
                    )
                    .await?;
                    let SourceReply::ProviderInspection {
                        binding: actual,
                        inspection,
                    } = reply
                    else {
                        return Err(SourceReadError::InvalidReply);
                    };
                    if actual.as_ref() != &binding || inspection.target != *target {
                        return Err(SourceReadError::InvalidReply);
                    }
                    if !matches!(
                        inspection.state,
                        ProviderSessionState::Unloaded | ProviderSessionState::Closed
                    ) {
                        return Ok(DetailsClientCapabilities::Unavailable(
                            "Session is already loaded; Load and Resume cannot restore it again"
                                .to_owned(),
                        ));
                    }
                    let load = inspection.capabilities.load
                        && binding.capabilities.as_slice().iter().any(|capability| {
                            capability.name == ProviderCapabilityName::Load
                                && capability.status == ProviderCapabilityStatus::Supported
                        });
                    let actions = match (load, inspection.capabilities.resume) {
                        (true, true) => ManagedRestoreActions::LoadAndResume,
                        (true, false) => ManagedRestoreActions::Load,
                        (false, true) => ManagedRestoreActions::Resume,
                        (false, false) => {
                            return Ok(DetailsClientCapabilities::Unavailable(
                                "Source advertises neither Load nor Resume for this session"
                                    .to_owned(),
                            ));
                        }
                    };
                    Ok(DetailsClientCapabilities::Managed {
                        binding: Box::new(binding),
                        actions,
                    })
                }
                _ => Err(SourceReadError::InvalidReply),
            }
        })
    })
}

#[cfg(test)]
#[path = "details_client_inspection_tests.rs"]
mod inspection_tests;
