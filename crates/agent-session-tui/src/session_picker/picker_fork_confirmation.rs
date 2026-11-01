//! Frozen fork source and destination choices; confirmation never re-reads focus.
use super::{PickerSourceContext, SessionsPickerRequest};
use agent_session_inventory::sessions::{
    RouterRegistryRead, SessionActionSelection, SessionPickerIdentity, SessionPickerRecord,
};
use codex_native_integration::{NativeWorkingDirectoryMetadata, native_working_directory_metadata};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ForkAvailability {
    Eligible,
    ProviderUnsupported,
    SourceUnqualified,
    HistoryRemainsOnSource,
}

impl ForkAvailability {
    pub(super) fn explanation(&self) -> Option<&'static str> {
        match self {
            Self::Eligible => None,
            Self::ProviderUnsupported => Some("Provider fork is unavailable in this launcher."),
            Self::SourceUnqualified => {
                Some("The source connection needs qualification before forking.")
            }
            Self::HistoryRemainsOnSource => {
                Some("History remains on the source machine; cross-machine fork is unavailable.")
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ForkDestinationKey {
    Source,
    Machine(PickerSourceContext),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ForkDestination {
    pub(super) key: ForkDestinationKey,
    pub(super) label: String,
    pub(super) availability: ForkAvailability,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct NativeForkSelection {
    pub(super) source: PickerSourceContext,
    pub(super) binding: agent_session_inventory::sessions::source_ssh_protocol::SourceNativeBinding,
    pub(super) target: collaboration_client::protocol::SessionRef,
    pub(super) directory:
        agent_session_inventory::sessions::router_connection_registry::AbsoluteRemoteCwd,
    pub(super) arguments: Vec<std::ffi::OsString>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ForkLaunchTarget {
    Codex(Box<SessionActionSelection>),
    Native(Box<NativeForkSelection>),
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ForkConfirmation {
    pub(super) launch_target: ForkLaunchTarget,
    pub(super) title: String,
    pub(super) full_title: String,
    pub(super) explicit_name: Option<String>,
    pub(super) source_label: String,
    pub(super) directory_label: String,
    pub(super) directory_origin: &'static str,
    pub(super) destinations: Vec<ForkDestination>,
    pub(super) focused_destination: usize,
    pub(super) notice: Option<&'static str>,
}

impl ForkConfirmation {
    pub(super) fn session_id_label(&self) -> Option<String> {
        match &self.launch_target {
            ForkLaunchTarget::Codex(selection) => Some(selection.session_id()),
            ForkLaunchTarget::Native(selection) => {
                Some(String::from(selection.target.session_id.clone()))
            }
            ForkLaunchTarget::Unsupported => None,
        }
    }
    pub(super) fn capture(record: &SessionPickerRecord, request: &SessionsPickerRequest) -> Self {
        let source_label = record.machine_label().to_owned();
        let configured = matches!(
            record.source_context,
            Some(PickerSourceContext::ConfiguredHosted(_))
        );
        let (directory_label, directory_origin) = if configured {
            (
                record
                    .cwd
                    .clone()
                    .unwrap_or_else(|| "Unavailable".to_owned()),
                "source session",
            )
        } else {
            match native_working_directory_metadata(&request.current_dir, &request.native_arguments)
            {
                NativeWorkingDirectoryMetadata::Invoking { directory } => {
                    (directory.display().to_string(), "invoking directory")
                }
                NativeWorkingDirectoryMetadata::Explicit { directory } => (
                    directory.to_string_lossy().into_owned(),
                    "explicit native argument",
                ),
                NativeWorkingDirectoryMetadata::UnresolvedExplicit => (
                    "Unresolved explicit override".to_owned(),
                    "native arguments; launcher determines the result",
                ),
            }
        };
        let availability = if record.identity.is_provider() {
            ForkAvailability::ProviderUnsupported
        } else if configured
            && !matches!((&record.source_context, &record.identity, record.provenance),
            (Some(PickerSourceContext::ConfiguredHosted(profile)), SessionPickerIdentity::HostedCodex(target), agent_session_inventory::sessions::SessionRowProvenance::ObservedHosted)
            if matches!(profile.transport, agent_session_inventory::sessions::router_connection_registry::RouterConnectionTransport::Ssh(_)) && target.endpoint.service_id == profile.service_id)
        {
            ForkAvailability::SourceUnqualified
        } else {
            ForkAvailability::Eligible
        };
        let mut destinations = vec![ForkDestination {
            key: ForkDestinationKey::Source,
            label: source_label.clone(),
            availability,
        }];
        if let RouterRegistryRead::Ready(registry) = &request.router_registry {
            destinations.extend(
                registry
                    .routers
                    .iter()
                    .filter(|profile| match &record.identity {
                        SessionPickerIdentity::HostedCodex(target)
                        | SessionPickerIdentity::HostedProvider(target)
                        | SessionPickerIdentity::NativeClaude(target) => {
                            profile.service_id != target.endpoint.service_id
                        }
                        SessionPickerIdentity::LocalCodex(_) => true,
                    })
                    .map(|profile| ForkDestination {
                        key: ForkDestinationKey::Machine(PickerSourceContext::ConfiguredHosted(
                            profile.clone(),
                        )),
                        label: profile.name.as_str().to_owned(),
                        availability: ForkAvailability::HistoryRemainsOnSource,
                    }),
            );
        }
        Self {
            launch_target: if record.identity.is_provider() {
                ForkLaunchTarget::Unsupported
            } else {
                ForkLaunchTarget::Codex(Box::new(SessionActionSelection::from_picker_record(
                    record,
                )))
            },
            title: record.title.clone(),
            full_title: record.full_title.clone(),
            explicit_name: record.explicit_name.clone(),
            source_label,
            directory_label,
            directory_origin,
            destinations,
            focused_destination: 0,
            notice: None,
        }
    }

    pub(super) fn capture_native(
        record: &SessionPickerRecord,
        request: &SessionsPickerRequest,
        binding: &agent_session_inventory::sessions::source_ssh_protocol::SourceNativeBinding,
    ) -> Option<Self> {
        let SessionPickerIdentity::NativeClaude(target) = &record.identity else {
            return None;
        };
        if target.endpoint.service_id != binding.service_id
            || String::from(target.endpoint.endpoint_id.clone()) != "claude-local"
        {
            return None;
        }
        let source = record
            .source_context
            .clone()
            .unwrap_or(PickerSourceContext::DefaultHosted);
        match &source {
            PickerSourceContext::DefaultHosted => {}
            PickerSourceContext::ConfiguredHosted(profile)
                if profile.service_id == binding.service_id
                    && matches!(
                        profile.transport,
                        agent_session_inventory::sessions::router_connection_registry::RouterConnectionTransport::Ssh(
                            _
                        )
                    ) => {}
            _ => return None,
        }
        let directory: agent_session_inventory::sessions::router_connection_registry::AbsoluteRemoteCwd =
            record.cwd.clone()?.try_into().ok()?;
        let mut confirmation = Self::capture(record, request);
        confirmation.directory_label = directory.as_str().to_owned();
        confirmation.directory_origin = "source session";
        confirmation.launch_target = ForkLaunchTarget::Native(Box::new(NativeForkSelection {
            source,
            binding: binding.clone(),
            target: target.clone(),
            directory,
            arguments: request.native_arguments.clone(),
        }));
        confirmation.destinations.first_mut()?.availability = ForkAvailability::Eligible;
        Some(confirmation)
    }

    pub(super) fn choice_options(
        &self,
    ) -> Vec<crate::session_picker::picker_choice::PickerChoice<ForkDestinationKey>> {
        self.destinations
            .iter()
            .map(|destination| {
                let mut option = crate::session_picker::picker_choice::PickerChoice::available(
                    destination.key.clone(),
                    destination.label.clone(),
                );
                if let Some(reason) = destination.availability.explanation() {
                    option.availability =
                        agent_collaboration_tui::actions::Availability::Unavailable {
                            reason: reason.to_owned(),
                        };
                }
                option
            })
            .collect()
    }
    pub(super) fn select_destination(&mut self, key: ForkDestinationKey) {
        if let Some(index) = self.destinations.iter().position(|destination| {
            destination.key == key && destination.availability == ForkAvailability::Eligible
        }) {
            self.focused_destination = index;
            self.notice = None;
        }
    }

    #[cfg(test)]
    pub(super) fn move_destination(&mut self, delta: isize) {
        self.focused_destination = self
            .focused_destination
            .saturating_add_signed(delta)
            .min(self.destinations.len().saturating_sub(1));
        self.notice = None;
    }

    pub(super) fn confirm(&mut self) -> Option<ForkLaunchTarget> {
        let destination = self.destinations.get(self.focused_destination)?;
        if let Some(reason) = destination.availability.explanation() {
            self.notice = Some(reason);
            None
        } else {
            match &self.launch_target {
                ForkLaunchTarget::Unsupported => None,
                target => Some(target.clone()),
            }
        }
    }
}
