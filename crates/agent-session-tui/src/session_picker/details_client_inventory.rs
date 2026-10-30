//! Source-qualified client actions for one captured Details record.
use super::PickerSourceContext;
use agent_session_inventory::sessions::{
    SessionPickerRecord,
    source_ssh_protocol::{SourceNativeBinding, SourceReadError},
};
use collaboration_client::protocol::ProviderBindingIdentity;
use std::{future::Future, pin::Pin, sync::Arc};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagedRestoreActions {
    Load,
    Resume,
    LoadAndResume,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DetailsClientCapabilities {
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
pub struct DetailsClientQuery {
    pub source: PickerSourceContext,
    pub record: Box<SessionPickerRecord>,
}

pub type DetailsClientInspector = Arc<
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
    pub fn matches_query(&self, query: &DetailsClientQuery) -> bool {
        use agent_session_inventory::sessions::SessionPickerIdentity;
        let service = match (&query.record.identity, self) {
            (SessionPickerIdentity::HostedProvider(target), Self::Managed { binding, .. })
                if target.endpoint == binding.endpoint =>
            {
                Some(&binding.endpoint.service_id)
            }
            (SessionPickerIdentity::NativeClaude(target), Self::Native { source })
                if target.endpoint.service_id == source.service_id
                    && String::from(target.endpoint.endpoint_id.clone()) == "claude-local"
                    && query.record.native_restore_eligible() =>
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

#[cfg(test)]
#[path = "details_client_membership_tests.rs"]
mod membership_tests;
