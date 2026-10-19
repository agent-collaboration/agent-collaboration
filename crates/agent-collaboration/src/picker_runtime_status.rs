use collaboration_client::protocol::{NativeActiveFlag, NativeThreadStatus, ProviderSessionState};

/// Fixed read categories, never server diagnostics or private payload data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeLiveUnavailableReason {
    TimedOut,
    TransportUnavailable,
    DiscoveryUnavailable,
    ProviderUnavailable,
}

impl NativeLiveUnavailableReason {
    pub(crate) const fn description(self) -> &'static str {
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
pub(crate) enum PickerRuntimeCoverage {
    #[default]
    Unobserved,
    Available,
    Unavailable,
    LocalOnly,
}

impl PickerRuntimeCoverage {
    pub(crate) const fn notice(self) -> Option<&'static str> {
        match self {
            Self::Unobserved | Self::Available => None,
            Self::Unavailable => Some("Live status unavailable"),
            Self::LocalOnly => Some("Local mode: no live status"),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PickerRecordsSnapshot {
    pub(crate) records: Vec<crate::sessions::SessionPickerRecord>,
    pub(crate) runtime_coverage: PickerRuntimeCoverage,
    pub(crate) native_live_coverage: Vec<NativeLiveCoverage>,
}

/// A native partition is observed independently from managed and Codex catalogues.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeLiveCoverage {
    pub(crate) endpoint: collaboration_client::protocol::EndpointRef,
    pub(crate) service_epoch: collaboration_client::protocol::UuidIdentity,
    pub(crate) state: NativeLiveReadState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeLiveReadState {
    Reading,
    Complete,
    Unavailable(NativeLiveUnavailableReason),
}

/// Picker-owned projection of the native runtime states that affect browsing.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum PickerRuntimeStatus {
    #[default]
    Unknown,
    NotLoaded,
    Blocked,
    Active,
    Idle,
    SystemError,
}

impl PickerRuntimeStatus {
    pub(crate) const fn from_provider(state: &ProviderSessionState) -> Self {
        match state {
            ProviderSessionState::Unloaded | ProviderSessionState::Closed => Self::NotLoaded,
            ProviderSessionState::Idle => Self::Idle,
            ProviderSessionState::Running => Self::Active,
            ProviderSessionState::RequiresAction | ProviderSessionState::AuthenticationRequired => {
                Self::Blocked
            }
        }
    }

    pub(crate) fn from_native(status: &NativeThreadStatus) -> Self {
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

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::NotLoaded => "Not loaded",
            Self::Blocked => "Blocked",
            Self::Active => "Active",
            Self::Idle => "Idle",
            Self::SystemError => "Error",
        }
    }

    pub(crate) const fn icon(self) -> &'static str {
        match self {
            Self::Unknown => "?",
            Self::NotLoaded => "◌",
            Self::Blocked => "◆",
            Self::Active => "●",
            Self::Idle => "○",
            Self::SystemError => "!",
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
