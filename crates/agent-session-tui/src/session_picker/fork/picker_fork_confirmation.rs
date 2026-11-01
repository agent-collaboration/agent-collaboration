//! Frozen fork source and destination choices; confirmation never re-reads focus.
use crate::session_picker::PickerSourceContext;
use crate::session_picker::SessionsPickerRequest;
use agent_session_inventory::sessions::RouterRegistryRead;
use agent_session_inventory::sessions::SessionActionSelection;
use agent_session_inventory::sessions::SessionPickerIdentity;
use agent_session_inventory::sessions::SessionPickerRecord;
use codex_native_integration::NativeWorkingDirectoryMetadata;
use codex_native_integration::native_working_directory_metadata;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ForkAvailability {
    Eligible,
    ProviderUnsupported,
    SourceUnqualified,
    HistoryRemainsOnSource,
}

impl ForkAvailability {
    pub(crate) fn explanation(&self) -> Option<&'static str> {
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
pub(crate) enum ForkDestinationKey {
    Source,
    Machine(PickerSourceContext),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ForkDestination {
    pub(crate) key: ForkDestinationKey,
    pub(crate) label: String,
    pub(crate) availability: ForkAvailability,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeForkSelection {
    pub(crate) source: PickerSourceContext,
    pub(crate) binding: agent_session_inventory::sessions::source_ssh_protocol::SourceNativeBinding,
    pub(crate) target: collaboration_client::protocol::SessionRef,
    pub(crate) directory:
        agent_session_inventory::sessions::router_connection_registry::AbsoluteRemoteCwd,
    pub(crate) arguments: Vec<std::ffi::OsString>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ForkLaunchTarget {
    Codex(Box<SessionActionSelection>),
    Native(Box<NativeForkSelection>),
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ForkConfirmation {
    pub(crate) launch_target: ForkLaunchTarget,
    pub(crate) title: String,
    pub(crate) full_title: String,
    pub(crate) explicit_name: Option<String>,
    pub(crate) source_label: String,
    pub(crate) directory_label: String,
    pub(crate) directory_origin: &'static str,
    pub(crate) destinations: Vec<ForkDestination>,
    pub(crate) focused_destination: usize,
    pub(crate) notice: Option<&'static str>,
}

impl ForkConfirmation {
    pub(crate) fn session_id_label(&self) -> Option<String> {
        match &self.launch_target {
            ForkLaunchTarget::Codex(selection) => Some(selection.session_id()),
            ForkLaunchTarget::Native(selection) => {
                Some(String::from(selection.target.session_id.clone()))
            }
            ForkLaunchTarget::Unsupported => None,
        }
    }
    pub(crate) fn capture(record: &SessionPickerRecord, request: &SessionsPickerRequest) -> Self {
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

    pub(crate) fn capture_native(
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

    pub(crate) fn choice_options(
        &self,
    ) -> Vec<crate::session_picker::screen::picker_choice::PickerChoice<ForkDestinationKey>> {
        self.destinations
            .iter()
            .map(|destination| {
                let mut option =
                    crate::session_picker::screen::picker_choice::PickerChoice::available(
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
    pub(crate) fn select_destination(&mut self, key: ForkDestinationKey) {
        if let Some(index) = self.destinations.iter().position(|destination| {
            destination.key == key && destination.availability == ForkAvailability::Eligible
        }) {
            self.focused_destination = index;
            self.notice = None;
        }
    }

    #[cfg(test)]
    pub(crate) fn move_destination(&mut self, delta: isize) {
        self.focused_destination = self
            .focused_destination
            .saturating_add_signed(delta)
            .min(self.destinations.len().saturating_sub(1));
        self.notice = None;
    }

    pub(crate) fn confirm(&mut self) -> Option<ForkLaunchTarget> {
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
