//! Quota display data projected by the domain.
use crate::quota::CreditUsageStatus;
use codex_router_core::ids::AccountId;
use codex_router_core::provider::Provider;
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct QuotaStatusViewModel {
    pub width: usize,
    pub route_line: String,
    pub header: crate::quota_display_facts::QuotaHeaderFacts,
    pub why_line: String,
    pub pool_freshness_summary: String,
    pub selection_projection_degraded: bool,
    pub serving_clients: Option<u32>,
    pub rows: Vec<QuotaStatusAccountViewModel>,
    pub selected: Option<QuotaSelectedAccountViewModel>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuotaStatusAccountViewModel {
    pub provider: Provider,
    pub login_state: QuotaLoginState,
    pub account_id: AccountId,
    pub account_tag: String,
    pub active_credential_generation: Option<u64>,
    pub enabled: bool,
    pub selected: bool,
    pub account: String,
    pub status: crate::quota_display_facts::QuotaRowStatus,
    pub active_clients: String,
    pub reset_credits: String,
    pub credit_usage_summary: String,
    pub credit_usage: CreditUsageStatus,
    pub reason: String,
    pub weekly_window: String,
    pub display_weekly: QuotaDisplayWindow,
    pub display_short: QuotaDisplayWindow,
    pub short_window: String,
    pub burn_meter: String,
    pub sample_metadata: SampleMetadata,
    pub reset_pace: ResetPaceViewModel,
    pub weekly_pace: String,
    pub weekly_quota_floor_percent: u16,
    pub details: QuotaSelectedAccountViewModel,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum QuotaLoginState {
    #[default]
    NotRequired,
    Required,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SampleConfidence {
    #[default]
    Unknown,
    Fresh,
    Stale,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SampleMetadata {
    pub confidence: SampleConfidence,
    pub age_label: String,
    pub age_seconds: Option<u64>,
    pub semantic_label: &'static str,
}

impl Default for SampleMetadata {
    fn default() -> Self {
        Self {
            confidence: SampleConfidence::Unknown,
            age_label: "unknown".to_owned(),
            age_seconds: None,
            semantic_label: "sample unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ResetPaceState {
    UnderBurning,
    #[default]
    Healthy,
    OverBurning,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ResetPaceMeterSegments {
    pub filled: usize,
    pub empty: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResetPaceViewModel {
    pub state: ResetPaceState,
    pub multiple_label: String,
    pub impact_label: Option<String>,
    pub semantic_label: &'static str,
    pub meter_left_segments: ResetPaceMeterSegments,
    pub meter_right_segments: ResetPaceMeterSegments,
    pub center_marker: char,
    pub unavailable_reason: Option<String>,
}

impl Default for ResetPaceViewModel {
    fn default() -> Self {
        Self {
            state: ResetPaceState::Unavailable,
            multiple_label: "burn unavailable".to_owned(),
            impact_label: None,
            semantic_label: "burn unavailable",
            meter_left_segments: ResetPaceMeterSegments {
                filled: 0,
                empty: 7,
            },
            meter_right_segments: ResetPaceMeterSegments {
                filled: 0,
                empty: 7,
            },
            center_marker: '│',
            unavailable_reason: Some("reset pace unavailable".to_owned()),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct QuotaSelectedAccountViewModel {
    pub account: String,
    pub status: String,
    pub reason: String,
    pub short_window: String,
    pub weekly_window: String,
    pub burn_meter: String,
    pub burn_pace: String,
    pub sample_metadata: SampleMetadata,
    pub reset_pace: ResetPaceViewModel,
    pub short_reset_pace: ResetPaceViewModel,
    pub total_rate: String,
    pub connection_rate: String,
    pub active_clients: String,
    pub guards: String,
    pub reset: String,
    pub note: String,
}

/// Included quota facts, kept separate from the preserved static display strings.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum QuotaDisplayWindow {
    #[default]
    Unknown,
    Known {
        remaining_percent: u32,
        reset_in_seconds: Option<u64>,
        stale: bool,
    },
}
