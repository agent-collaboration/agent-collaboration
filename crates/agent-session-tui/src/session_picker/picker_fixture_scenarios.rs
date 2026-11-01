//! Named synthetic inputs; all effects enter through the picker's existing ports.
use super::provider_filter_storyboard_fixture::{FIXTURE_SERVICE, SHARED_SESSION};
use super::{PickerMachineSourceMode, PickerSourceContext, SessionsPickerRequest};
use agent_session_inventory::sessions::SessionPickerRecord;
use collaboration_client::protocol::{
    ClaudeCodeBackgroundOrigin, NativeClaudeBackgroundState, ProviderSessionSummary, SessionRef,
};
use std::io;

#[cfg(debug_assertions)]
#[path = "picker_fixture_fork_preparation.rs"]
mod fork_preparation;
#[cfg(debug_assertions)]
pub(super) use fork_preparation::source_preparer;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FixtureSourceChange {
    Add,
    Remove,
    Replace,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PickerFixtureScenario {
    StoppedNativeClaude,
    LocalCodexNew,
    UncommittedMachine,
    SourceOptions(FixtureSourceChange),
    ForkSshProfile,
    ClaudeNewSuccess,
    LargeList,
}

impl PickerFixtureScenario {
    pub(super) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "stopped-native-claude" => Self::StoppedNativeClaude,
            "local-codex-new" => Self::LocalCodexNew,
            "new-machine-uncommitted" => Self::UncommittedMachine,
            "source-options-add" => Self::SourceOptions(FixtureSourceChange::Add),
            "source-options-remove" => Self::SourceOptions(FixtureSourceChange::Remove),
            "source-options-replace" => Self::SourceOptions(FixtureSourceChange::Replace),
            "fork-ssh-profile" => Self::ForkSshProfile,
            "claude-new-success" => Self::ClaudeNewSuccess,
            "large-list" => Self::LargeList,
            _ => return None,
        })
    }

    pub(super) fn apply(
        self,
        mut request: SessionsPickerRequest,
    ) -> io::Result<SessionsPickerRequest> {
        match self {
            Self::StoppedNativeClaude => {
                let target: SessionRef = serde_json::from_value(serde_json::json!({
                    "endpoint":{"serviceId":FIXTURE_SERVICE,"endpointId":"claude-local"},
                    "sessionId":SHARED_SESSION,
                }))
                .map_err(io::Error::other)?;
                let summary = ProviderSessionSummary::ClaudeCodeBackground {
                    origin: ClaudeCodeBackgroundOrigin::ClaudeCodeBackground,
                    target,
                    job_handle: "fixture-stopped-job"
                        .to_owned()
                        .try_into()
                        .map_err(io::Error::other)?,
                    working_directory: "/fixture/stopped-claude"
                        .to_owned()
                        .try_into()
                        .map_err(io::Error::other)?,
                    name: Some("Stopped native Claude".to_owned()),
                    started_at: 1,
                    state: NativeClaudeBackgroundState::Stopped,
                    status: None,
                    peer: None,
                };
                let mut record =
                    SessionPickerRecord::from_provider_summary(&summary, "Stopped native Claude");
                record.source_context = Some(PickerSourceContext::DefaultHosted);
                request.records.retain(|row| {
                    !matches!(
                        row.identity,
                        agent_session_inventory::sessions::SessionPickerIdentity::NativeClaude(_)
                    )
                });
                request.records.push(record);
            }
            Self::LocalCodexNew => request.machine_mode = PickerMachineSourceMode::LocalCodex,
            Self::UncommittedMachine => {
                return super::provider_filter_storyboard_fixture::apply_storyboard_variant(
                    request,
                    Some("configured-source-new"),
                );
            }
            Self::SourceOptions(change) => request.records = change.initial_records(),
            Self::ForkSshProfile => {
                request = super::provider_filter_storyboard_fixture::apply_storyboard_variant(
                    request,
                    Some("configured-source-new"),
                )?;
                let agent_session_inventory::sessions::RouterRegistryRead::Ready(registry) =
                    &request.router_registry
                else {
                    return Err(io::Error::other(
                        "SSH fork fixture requires its configured source",
                    ));
                };
                let profile = registry
                    .routers
                    .first()
                    .cloned()
                    .ok_or_else(|| io::Error::other("SSH fork source is absent"))?;
                let target:SessionRef=serde_json::from_value(serde_json::json!({"endpoint":{"serviceId":FIXTURE_SERVICE,"endpointId":"codex-fixture-ssh"},"sessionId":SHARED_SESSION})).map_err(io::Error::other)?;
                let mut record = super::test_support::picker_record(
                    SHARED_SESSION,
                    "SSH fork source",
                    "/fixture/ssh-fork",
                    "codex-router",
                    "cli",
                );
                record.identity =
                    agent_session_inventory::sessions::SessionPickerIdentity::HostedCodex(target);
                record.provenance =
                    agent_session_inventory::sessions::SessionRowProvenance::ObservedHosted;
                record.machine_display_label = Some(profile.name.as_str().to_owned());
                record.source_context = Some(PickerSourceContext::ConfiguredHosted(profile));
                request.records = vec![record];
            }
            Self::ClaudeNewSuccess => {}
            Self::LargeList => {
                const RECENCY_BASE_MS: i64 = 1_790_000_000_000;
                request.records = (0..60)
                    .map(|index| {
                        let mut record = super::test_support::picker_record(
                            &format!("fixture-large-{index:03}"),
                            &format!("Large session {:03}", index + 1),
                            "/repo/project-a",
                            "codex-router",
                            "cli",
                        );
                        record.recency_at_ms = Some(RECENCY_BASE_MS - i64::from(index) * 60_000);
                        record.created_at_ms = Some(RECENCY_BASE_MS - 86_400_000);
                        record
                    })
                    .collect();
            }
        }
        Ok(request)
    }
}

impl FixtureSourceChange {
    fn records(providers: &[&str]) -> Vec<SessionPickerRecord> {
        providers
            .iter()
            .map(|provider| {
                super::test_support::picker_record(
                    &format!("fixture-provider-{provider}"),
                    &format!("Source option {provider}"),
                    "/repo/project-a",
                    provider,
                    "cli",
                )
            })
            .collect()
    }

    pub(super) fn initial_records(self) -> Vec<SessionPickerRecord> {
        Self::records(match self {
            Self::Add => &["beta-provider", "omega-provider"],
            Self::Remove | Self::Replace => &["alpha-provider", "beta-provider", "omega-provider"],
        })
    }

    pub(super) fn updated_records(self) -> Vec<SessionPickerRecord> {
        Self::records(match self {
            Self::Add => &["alpha-provider", "beta-provider", "omega-provider"],
            Self::Remove => &["alpha-provider", "omega-provider"],
            Self::Replace => &["beta-provider", "zeta-provider"],
        })
    }
}

#[cfg(debug_assertions)]
pub(super) fn client_ports(
    scenario: Option<PickerFixtureScenario>,
    request: &SessionsPickerRequest,
) -> Option<super::SessionClientPorts> {
    use super::details_client_inventory::DetailsClientCapabilities;
    use agent_session_inventory::sessions::{
        SessionPickerIdentity,
        source_ssh_protocol::{SourceNativeBinding, SourceReadError, SourceRejection},
    };
    use std::sync::Arc;
    if scenario == Some(PickerFixtureScenario::ClaudeNewSuccess) {
        return claude_new_ports();
    }
    if scenario != Some(PickerFixtureScenario::StoppedNativeClaude) {
        return None;
    }
    let captured = request
        .records
        .iter()
        .find(|row| matches!(row.identity, SessionPickerIdentity::NativeClaude(_)))?
        .clone();
    Some(super::SessionClientPorts {
        inventory_loader: Arc::new(|_| {
            Box::pin(async { Err(SourceReadError::Rejected(SourceRejection::InvalidRequest)) })
        }),
        executor: Arc::new(|_| {
            Box::pin(async { Err(SourceReadError::Rejected(SourceRejection::InvalidRequest)) })
        }),
        details_inspector: Arc::new(move |query| {
            let matches = query.record.as_ref() == &captured
                && query.source == PickerSourceContext::DefaultHosted;
            Box::pin(async move {
                if !matches {
                    return Err(SourceReadError::Rejected(SourceRejection::InvalidRequest));
                }
                Ok(DetailsClientCapabilities::Native {
                    source: SourceNativeBinding {
                        service_id: FIXTURE_SERVICE
                            .to_owned()
                            .try_into()
                            .map_err(|_| SourceReadError::InvalidReply)?,
                        service_epoch: "00000000-0000-4000-8000-000000000002"
                            .to_owned()
                            .try_into()
                            .map_err(|_| SourceReadError::InvalidReply)?,
                    },
                })
            })
        }),
    })
}

#[cfg(debug_assertions)]
fn claude_new_ports() -> Option<super::SessionClientPorts> {
    use super::new_session_client_inventory::{
        CursorCreationAvailability, NewClientSourceCapabilities,
    };
    use agent_session_inventory::sessions::source_ssh_protocol::{
        SourceNativeLaunchAvailability, SourceReadError, SourceRejection,
    };
    use std::sync::Arc;
    let capabilities = NewClientSourceCapabilities {
        service_id: FIXTURE_SERVICE.to_owned().try_into().ok()?,
        service_epoch: "00000000-0000-4000-8000-000000000002"
            .to_owned()
            .try_into()
            .ok()?,
        native_claude: SourceNativeLaunchAvailability::Ready {
            capabilities: Vec::new(),
        },
        cursor: CursorCreationAvailability::Unavailable(
            "Fixture has no Cursor endpoint".to_owned(),
        ),
    };
    Some(super::SessionClientPorts {
        inventory_loader: Arc::new(move |source| {
            let capabilities = capabilities.clone();
            Box::pin(async move {
                if source != PickerSourceContext::DefaultHosted {
                    return Err(SourceReadError::Rejected(SourceRejection::InvalidRequest));
                }
                Ok(capabilities)
            })
        }),
        // Foreground New returns a captured terminal handoff. An unexpected
        // in-picker execution is rejected; this fixture never starts a client.
        executor: Arc::new(|_| {
            Box::pin(async { Err(SourceReadError::Rejected(SourceRejection::InvalidRequest)) })
        }),
        details_inspector: Arc::new(|_| {
            Box::pin(async { Err(SourceReadError::Rejected(SourceRejection::InvalidRequest)) })
        }),
    })
}

#[cfg(debug_assertions)]
#[path = "picker_fixture_source_updates.rs"]
pub(super) mod source_updates;
