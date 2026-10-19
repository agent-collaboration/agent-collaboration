//! Source-qualified client actions for one captured Details record.
use super::PickerSourceContext;
use crate::sessions::{
    SessionPickerRecord,
    source_ssh_protocol::{SourceNativeBinding, SourceReadError},
};
use collaboration_client::protocol::ProviderBindingIdentity;
use std::{future::Future, pin::Pin, sync::Arc};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ManagedRestoreActions {
    Load,
    Resume,
    LoadAndResume,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DetailsClientCapabilities {
    Native {
        source: SourceNativeBinding,
    },
    Managed {
        binding: Box<ProviderBindingIdentity>,
        actions: ManagedRestoreActions,
    },
    Unavailable(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DetailsClientQuery {
    pub(crate) source: PickerSourceContext,
    pub(crate) record: Box<SessionPickerRecord>,
}

pub(crate) type DetailsClientInspector = Arc<
    dyn Fn(
            DetailsClientQuery,
        ) -> Pin<
            Box<dyn Future<Output = Result<DetailsClientCapabilities, SourceReadError>> + Send>,
        > + Send
        + Sync,
>;

#[cfg(test)]
#[path = "details_client_action_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "details_client_inspection_tests.rs"]
mod inspection_tests;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) enum DetailsClientReadState {
    #[default]
    Unobserved,
    Checking,
    Ready(DetailsClientCapabilities),
    Unavailable(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DetailsReadRequest {
    generation: u64,
    query: DetailsClientQuery,
}

pub(super) fn use_details_client_inventory(
    hooks: &mut iocraft::prelude::Hooks<'_, '_>,
    mut model: iocraft::prelude::State<super::picker_model::SessionsPickerModel>,
    inspector: Option<DetailsClientInspector>,
) {
    use super::source_action_worker::{LatestSourceReadPort, run_latest_source_read};
    use iocraft::prelude::*;
    let has_inspector = inspector.is_some();
    let port = hooks.use_memo(LatestSourceReadPort::<DetailsReadRequest>::new, ());
    hooks.use_future({
        let receiver = port.take_receiver();
        async move {
            let (Some(receiver), Some(inspector)) = (receiver, inspector) else {
                return;
            };
            run_latest_source_read(
                receiver,
                move |request: DetailsReadRequest| inspector(request.query),
                move |request, result| {
                    let mut model = model.write();
                    if let Some(dialog) = model.details_dialog.as_mut()
                        && dialog.generation == request.generation
                        && dialog.record() == request.query.record.as_ref()
                    {
                        dialog.client_inventory = match result {
                            Ok(capabilities) if capabilities.matches_query(&request.query) => {
                                DetailsClientReadState::Ready(capabilities)
                            }
                            Ok(_) => DetailsClientReadState::Unavailable(
                                "Client inspection did not match the captured source session"
                                    .to_owned(),
                            ),
                            Err(error) => DetailsClientReadState::Unavailable(error.to_string()),
                        };
                    }
                },
            )
            .await;
        }
    });
    let pending = if has_inspector {
        let model = model.read();
        model.details_dialog.as_ref().and_then(|dialog| {
            (dialog.record().identity.is_provider()
                && matches!(
                    dialog.client_inventory,
                    DetailsClientReadState::Unobserved | DetailsClientReadState::Checking
                ))
            .then(|| DetailsReadRequest {
                generation: dialog.generation,
                query: DetailsClientQuery {
                    source: dialog
                        .record()
                        .source_context
                        .clone()
                        .unwrap_or(PickerSourceContext::DefaultHosted),
                    record: Box::new(dialog.record().clone()),
                },
            })
        })
    } else {
        None
    };
    let mark_checking = pending.is_some()
        && model.read().details_dialog.as_ref().is_some_and(|dialog| {
            matches!(dialog.client_inventory, DetailsClientReadState::Unobserved)
        });
    if mark_checking && let Some(dialog) = model.write().details_dialog.as_mut() {
        dialog.client_inventory = DetailsClientReadState::Checking;
    }
    port.synchronize(pending);
}

impl DetailsClientCapabilities {
    pub(super) fn matches_query(&self, query: &DetailsClientQuery) -> bool {
        use crate::sessions::SessionPickerIdentity;
        let service = match (&query.record.identity, self) {
            (SessionPickerIdentity::HostedProvider(target), Self::Managed { binding, .. })
                if target.endpoint == binding.endpoint =>
            {
                Some(&binding.endpoint.service_id)
            }
            (SessionPickerIdentity::NativeClaude(target), Self::Native { source })
                if target.endpoint.service_id == source.service_id
                    && String::from(target.endpoint.endpoint_id.clone()) == "claude-local" =>
            {
                Some(&source.service_id)
            }
            (_, Self::Unavailable(_)) => return true,
            _ => None,
        };
        match (&query.source, service) {
            (PickerSourceContext::DefaultHosted, Some(_)) => true,
            (PickerSourceContext::ConfiguredHosted(profile), Some(service)) => {
                profile.service_id == *service
            }
            _ => false,
        }
    }
}

pub(super) fn production_inspector(context: crate::CliContext) -> DetailsClientInspector {
    Arc::new(move |query| {
        let context = context.clone();
        Box::pin(async move {
            use crate::sessions::{
                SessionPickerIdentity,
                source_ssh_protocol::{SourceNativeLaunchAvailability, SourceReply},
            };
            use collaboration_client::protocol::{
                ChannelDescription, CodexGeneration, ProviderCapabilityName,
                ProviderCapabilityStatus, ProviderSessionState,
            };
            let (expected, inventory, native_claude) =
                super::new_session_client_inventory::read_client_inventory(
                    &context,
                    query.source.clone(),
                )
                .await?;
            let membership = super::SourceInventoryBinding::from_inventory(&expected, &inventory)
                .map_err(|_| SourceReadError::InvalidReply)?;
            match &query.record.identity {
                SessionPickerIdentity::NativeClaude(target) => {
                    if membership.native_claude_endpoint.as_ref() != Some(&target.endpoint) {
                        return Err(SourceReadError::InvalidReply);
                    }
                    if query.record.cwd.as_ref().is_none_or(|directory| {
                        crate::sessions::router_connection_registry::AbsoluteRemoteCwd::try_from(
                            directory.clone(),
                        )
                        .is_err()
                    }) {
                        return Ok(DetailsClientCapabilities::Unavailable(
                            "Source session working directory is unavailable".to_owned(),
                        ));
                    }
                    match native_claude {
                        SourceNativeLaunchAvailability::Ready => {
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
                    let reply = crate::sessions::source_client_action::read_provider_details(
                        &context,
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
