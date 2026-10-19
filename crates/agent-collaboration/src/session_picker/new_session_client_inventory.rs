//! Read-only source qualification for the New draft. Names and stored rows do
//! not establish that a source can create a particular kind of client.
use super::{PickerSourceContext, SourceInventoryBinding};
use crate::{
    CliContext,
    sessions::source_ssh_protocol::{
        SourceNativeLaunchAvailability, SourceOperation, SourceReadError, SourceRejection,
        SourceReply,
    },
};
use collaboration_client::protocol::{
    ChannelDescription, CodexGeneration, EndpointAvailability, EndpointInventory,
    ProviderBindingIdentity, ProviderCapabilityName, ProviderCapabilityStatus, ProviderKind,
    UuidIdentity,
};
use iocraft::prelude::*;
use std::{future::Future, pin::Pin, sync::Arc};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum CursorCreationAvailability {
    Ready(ProviderBindingIdentity),
    Unavailable(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NewClientSourceCapabilities {
    pub(super) service_id: UuidIdentity,
    pub(super) service_epoch: UuidIdentity,
    pub(super) native_claude: SourceNativeLaunchAvailability,
    pub(super) cursor: CursorCreationAvailability,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) enum NewClientInventoryState {
    #[default]
    Unobserved,
    Checking,
    Ready(Box<NewClientSourceCapabilities>),
    Unavailable(String),
}

pub(crate) type NewClientInventoryLoader = Arc<
    dyn Fn(
            PickerSourceContext,
        ) -> Pin<
            Box<dyn Future<Output = Result<NewClientSourceCapabilities, SourceReadError>> + Send>,
        > + Send
        + Sync,
>;

#[derive(Clone)]
pub(crate) struct SessionClientPorts {
    pub(crate) inventory_loader: NewClientInventoryLoader,
    pub(crate) executor: crate::sessions::source_client_action::SessionClientExecutor,
    pub(crate) details_inspector: super::details_client_inventory::DetailsClientInspector,
}

pub(crate) fn production_ports(context: CliContext) -> SessionClientPorts {
    SessionClientPorts {
        inventory_loader: production_loader(context.clone()),
        details_inspector: super::details_client_inventory::production_inspector(context.clone()),
        executor: crate::sessions::source_client_action::production_executor(context),
    }
}

pub(crate) fn production_loader(context: CliContext) -> NewClientInventoryLoader {
    Arc::new(move |source| {
        let context = context.clone();
        Box::pin(async move {
            let (expected, inventory, native_claude) =
                read_client_inventory(&context, source).await?;
            qualify_capabilities(&expected, inventory, native_claude)
        })
    })
}

pub(super) async fn read_client_inventory(
    context: &CliContext,
    source: PickerSourceContext,
) -> Result<
    (
        UuidIdentity,
        EndpointInventory,
        SourceNativeLaunchAvailability,
    ),
    SourceReadError,
> {
    let (expected, reply) = match source {
        PickerSourceContext::ConfiguredHosted(profile) => {
            let expected = profile.service_id.clone();
            let reply = crate::sessions::source_ssh_transport::read_source(
                &profile,
                SourceOperation::ClientInventory,
            )
            .await?;
            (expected, reply)
        }
        PickerSourceContext::DefaultHosted => {
            crate::sessions::source_ssh_executor::inspect_local_clients(&context).await?
        }
        PickerSourceContext::LocalCodex => {
            return Err(SourceReadError::Rejected(
                SourceRejection::UnsupportedConnection,
            ));
        }
    };
    let SourceReply::ClientInventory {
        inventory,
        native_claude,
    } = reply
    else {
        return Err(SourceReadError::InvalidReply);
    };
    Ok((expected, inventory, native_claude))
}

fn qualify_capabilities(
    expected: &UuidIdentity,
    inventory: EndpointInventory,
    native_claude: SourceNativeLaunchAvailability,
) -> Result<NewClientSourceCapabilities, SourceReadError> {
    SourceInventoryBinding::from_inventory(expected, &inventory)
        .map_err(|_| SourceReadError::InvalidReply)?;
    let mut candidates = Vec::new();
    for endpoint in &inventory.endpoints {
        if !matches!(
            endpoint.availability,
            EndpointAvailability::Available { .. }
        ) {
            continue;
        }
        for channel in &endpoint.channels {
            if let ChannelDescription::ExternalProvider {
                transport,
                binding_id,
                binding_generation,
                runtime,
                capabilities,
            } = channel
                && runtime.provider == ProviderKind::Cursor
                && capabilities.as_slice().iter().any(|capability| {
                    capability.name == ProviderCapabilityName::Create
                        && capability.status == ProviderCapabilityStatus::Supported
                })
            {
                candidates.push(ProviderBindingIdentity {
                    endpoint: endpoint.endpoint.clone(),
                    binding_id: binding_id.clone(),
                    runtime: runtime.clone(),
                    transport: *transport,
                    generation: CodexGeneration {
                        service_epoch: inventory.service_epoch.clone(),
                        generation: *binding_generation,
                    },
                    capabilities: capabilities.clone(),
                });
            }
        }
    }
    let cursor = match candidates.len() {
        1 => CursorCreationAvailability::Ready(candidates.remove(0)),
        0 => CursorCreationAvailability::Unavailable(
            "Source has no available Cursor endpoint advertising Create".to_owned(),
        ),
        _ => CursorCreationAvailability::Unavailable(
            "Source has multiple Cursor creation bindings; choose an unambiguous source".to_owned(),
        ),
    };
    Ok(NewClientSourceCapabilities {
        service_id: expected.clone(),
        service_epoch: inventory.service_epoch,
        native_claude,
        cursor,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ClientInventoryReadRequest {
    generation: u64,
    source: PickerSourceContext,
}

pub(super) fn use_client_inventory(
    hooks: &mut Hooks<'_, '_>,
    mut model: State<super::picker_model::SessionsPickerModel>,
    loader: Option<NewClientInventoryLoader>,
) {
    use super::source_action_worker::{LatestSourceReadPort, run_latest_source_read};
    let has_loader = loader.is_some();
    let port = hooks.use_memo(LatestSourceReadPort::<ClientInventoryReadRequest>::new, ());
    hooks.use_future({
        let receiver = port.take_receiver();
        async move {
            let (Some(receiver), Some(loader)) = (receiver, loader) else {
                return;
            };
            run_latest_source_read(
                receiver,
                move |request: ClientInventoryReadRequest| loader(request.source),
                move |request, result| {
                    if let Some(draft) = model.write().new_session.as_mut()
                        && draft.generation == request.generation
                        && draft.selected_source == request.source
                    {
                        draft.client_inventory = match result {
                            Ok(capabilities) => {
                                NewClientInventoryState::Ready(Box::new(capabilities))
                            }
                            Err(error) => NewClientInventoryState::Unavailable(error.to_string()),
                        };
                    }
                },
            )
            .await;
        }
    });
    let pending = has_loader
        .then(|| {
            let model = model.read();
            model.new_session.as_ref().and_then(|draft| {
                (matches!(
                    draft.client_inventory,
                    NewClientInventoryState::Unobserved | NewClientInventoryState::Checking
                ) && draft.selected_source != PickerSourceContext::LocalCodex)
                    .then(|| ClientInventoryReadRequest {
                        generation: draft.generation,
                        source: draft.selected_source.clone(),
                    })
            })
        })
        .flatten();
    let mark_checking = pending.is_some()
        && model
            .read()
            .new_session
            .as_ref()
            .is_some_and(|draft| draft.client_inventory == NewClientInventoryState::Unobserved);
    if mark_checking && let Some(draft) = model.write().new_session.as_mut() {
        draft.client_inventory = NewClientInventoryState::Checking;
    }
    port.synchronize(pending);
}
