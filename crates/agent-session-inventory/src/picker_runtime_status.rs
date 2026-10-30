use collaboration_client::protocol::{NativeActiveFlag, NativeThreadStatus, ProviderSessionState};

/// Fixed read categories, never server diagnostics or private payload data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeLiveUnavailableReason {
    TimedOut,
    TransportUnavailable,
    DiscoveryUnavailable,
    ProviderUnavailable,
}

impl NativeLiveUnavailableReason {
    pub const fn description(self) -> &'static str {
        match self {
            Self::TimedOut => "Native Claude inventory timed out",
            Self::TransportUnavailable => "Native Claude inventory transport unavailable",
            Self::DiscoveryUnavailable => "Native Claude inventory discovery unavailable",
            Self::ProviderUnavailable => "Native Claude inventory unavailable",
        }
    }
}

/// Availability of the selected Host's observation, separate from any thread's state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PickerRuntimeCoverage {
    #[default]
    Unobserved,
    Available,
    Unavailable,
    LocalOnly,
}

impl PickerRuntimeCoverage {
    pub const fn notice(self) -> Option<&'static str> {
        match self {
            Self::Unobserved | Self::Available => None,
            Self::Unavailable => Some("Live status unavailable"),
            Self::LocalOnly => Some("Local mode: no live status"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct PickerRecordsSnapshot {
    pub records: Vec<crate::sessions::SessionPickerRecord>,
    pub runtime_coverage: PickerRuntimeCoverage,
    pub native_live_coverage: Vec<NativeLiveCoverage>,
}

/// A native partition is observed independently from managed and Codex catalogues.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeLiveCoverage {
    pub endpoint: collaboration_client::protocol::EndpointRef,
    pub service_epoch: collaboration_client::protocol::UuidIdentity,
    pub view: NativeCatalogueView,
    pub state: NativeLiveReadState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum NativeCatalogueView {
    Stored,
    Loaded,
}

impl NativeCatalogueView {
    pub const fn request_view(self) -> collaboration_client::protocol::NativeSessionView {
        match self {
            Self::Stored => collaboration_client::protocol::NativeSessionView::Stored,
            Self::Loaded => collaboration_client::protocol::NativeSessionView::Loaded,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeLiveReadState {
    Reading,
    Complete,
    Incomplete(collaboration_client::protocol::NativeClaudeInventoryCoverage),
    Unavailable(NativeLiveUnavailableReason),
}

impl NativeLiveReadState {
    pub fn read_warning_lines(&self) -> Vec<String> {
        use collaboration_client::protocol::{
            NativeClaudeInventoryPart as Part, NativeInventorySource,
        };
        match self {
            Self::Incomplete(coverage) => [
                (NativeInventorySource::PeerRegistry, &coverage.peer),
                (NativeInventorySource::BackgroundJobs, &coverage.background),
            ]
            .into_iter()
            .filter_map(|(source, part)| match part {
                Part::NotRequested | Part::Complete => None,
                Part::Incomplete { unidentified_jobs } => Some(format!(
                    "Native Claude {source}: {unidentified_jobs} records lack a session identity"
                )),
                Part::Unavailable { reason } => Some(format!("Native Claude {source}: {reason}")),
            })
            .collect(),
            Self::Unavailable(reason) => vec![reason.description().to_owned()],
            Self::Reading | Self::Complete => Vec::new(),
        }
    }

    pub fn conflicts(&self) -> &[collaboration_client::protocol::NativeClaudeSessionConflict] {
        match self {
            Self::Incomplete(coverage) => &coverage.conflicts,
            _ => &[],
        }
    }
    pub const fn is_incomplete(&self) -> bool {
        matches!(self, Self::Incomplete(_) | Self::Unavailable(_))
    }
    pub fn from_observation(
        view: NativeCatalogueView,
        coverage: Option<collaboration_client::protocol::NativeClaudeInventoryCoverage>,
    ) -> Result<Self, &'static str> {
        use collaboration_client::protocol::NativeClaudeInventoryPart as Part;
        let coverage = coverage.ok_or("missing native inventory coverage")?;
        let mut targets = std::collections::BTreeSet::new();
        if coverage.conflicts.iter().any(|conflict| {
            view == NativeCatalogueView::Stored
                || !matches!(coverage.peer, Part::Complete)
                || collaboration_client::protocol::UuidIdentity::try_from(String::from(conflict.target.session_id.clone())).is_err()
                || !targets.insert(conflict.target.clone())
                || matches!(conflict.reason, collaboration_client::protocol::NativeClaudeConflictReason::DuplicateLiveClaims { claims } if claims < 2)
        }) {
            return Err("invalid native inventory conflicts");
        }
        if matches!(coverage.background, Part::NotRequested)
            || match view {
                NativeCatalogueView::Stored => !matches!(coverage.peer, Part::NotRequested),
                NativeCatalogueView::Loaded => matches!(coverage.peer, Part::NotRequested),
            }
            || [&coverage.peer, &coverage.background].iter().any(|part| {
                matches!(
                    part,
                    Part::Incomplete {
                        unidentified_jobs: 0
                    }
                )
            })
        {
            return Err("invalid native inventory coverage");
        }
        if matches!(coverage.background, Part::Complete)
            && matches!(coverage.peer, Part::Complete | Part::NotRequested)
            && coverage.conflicts.is_empty()
        {
            Ok(Self::Complete)
        } else {
            Ok(Self::Incomplete(coverage))
        }
    }
}

/// Picker-owned projection of the native runtime states that affect browsing.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PickerRuntimeStatus {
    #[default]
    Unknown,
    NotLoaded,
    Blocked,
    Active,
    Idle,
    SystemError,
    Conflict,
}

impl PickerRuntimeStatus {
    pub const fn from_provider(state: ProviderSessionState) -> Self {
        match state {
            ProviderSessionState::Unloaded | ProviderSessionState::Closed => Self::NotLoaded,
            ProviderSessionState::Idle => Self::Idle,
            ProviderSessionState::Running => Self::Active,
            ProviderSessionState::RequiresAction | ProviderSessionState::AuthenticationRequired => {
                Self::Blocked
            }
        }
    }

    pub fn from_native(status: &NativeThreadStatus) -> Self {
        match status {
            NativeThreadStatus::NotLoaded => Self::NotLoaded,
            NativeThreadStatus::Idle => Self::Idle,
            NativeThreadStatus::SystemError => Self::SystemError,
            NativeThreadStatus::Active { active_flags }
                if active_flags.iter().any(|flag| {
                    matches!(
                        flag,
                        NativeActiveFlag::WaitingOnApproval | NativeActiveFlag::WaitingOnUserInput
                    )
                }) =>
            {
                Self::Blocked
            }
            NativeThreadStatus::Active { .. } => Self::Active,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::NotLoaded => "Not loaded",
            Self::Blocked => "Blocked",
            Self::Active => "Active",
            Self::Idle => "Idle",
            Self::SystemError => "Error",
            Self::Conflict => "Conflict",
        }
    }

    pub const fn icon(self) -> &'static str {
        match self {
            Self::Unknown => "?",
            Self::NotLoaded => "◌",
            Self::Blocked => "◆",
            Self::Active => "●",
            Self::Idle => "○",
            Self::SystemError => "!",
            Self::Conflict => "!",
        }
    }
}

#[cfg(test)]
mod tests {
    use collaboration_client::protocol::{NativeActiveFlag, NativeThreadStatus};

    use super::PickerRuntimeStatus;

    #[test]
    fn native_active_waiting_flags_are_blocked() {
        for flag in [
            NativeActiveFlag::WaitingOnApproval,
            NativeActiveFlag::WaitingOnUserInput,
        ] {
            assert_eq!(
                PickerRuntimeStatus::from_native(&NativeThreadStatus::Active {
                    active_flags: vec![flag],
                }),
                PickerRuntimeStatus::Blocked
            );
        }
    }

    #[test]
    fn native_statuses_keep_unknown_separate_from_idle() {
        assert_eq!(PickerRuntimeStatus::default(), PickerRuntimeStatus::Unknown);
        assert_eq!(
            PickerRuntimeStatus::from_native(&NativeThreadStatus::Active {
                active_flags: Vec::new(),
            }),
            PickerRuntimeStatus::Active
        );
        assert_eq!(
            PickerRuntimeStatus::from_native(&NativeThreadStatus::Idle),
            PickerRuntimeStatus::Idle
        );
        assert_eq!(
            PickerRuntimeStatus::from_native(&NativeThreadStatus::NotLoaded),
            PickerRuntimeStatus::NotLoaded
        );
        assert_eq!(
            PickerRuntimeStatus::from_native(&NativeThreadStatus::SystemError),
            PickerRuntimeStatus::SystemError
        );
    }
}
