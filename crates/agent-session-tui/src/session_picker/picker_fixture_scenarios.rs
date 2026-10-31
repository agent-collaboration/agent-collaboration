//! Named synthetic inputs; all effects enter through the picker's existing ports.
use super::provider_filter_storyboard_fixture::{FIXTURE_SERVICE, SHARED_SESSION};
use super::{PickerMachineSourceMode, PickerSourceContext, SessionsPickerRequest};
use agent_session_inventory::sessions::SessionPickerRecord;
use collaboration_client::protocol::{
    ClaudeCodeBackgroundOrigin, NativeClaudeBackgroundState, ProviderSessionSummary, SessionRef,
};
use std::io;

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
#[path = "picker_fixture_source_updates.rs"]
pub(super) mod source_updates;
