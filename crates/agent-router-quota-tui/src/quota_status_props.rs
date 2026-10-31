//! Printed output and the interactive control tree share quota facts, not input paths.
use super::{
    CreditUsagePolicySaver, CreditUsageRefresher, quota_floor_editor::WeeklyQuotaFloorSaver,
    quota_status_view_model::QuotaStatusViewModelLoader,
    quota_view_configuration::QuotaInteractionMode,
};
use codex_router_quota_service::{
    quota_display_model::QuotaStatusViewModel,
    quota_reset::reset_session_supervisor::{ResetIntentSender, ResetWorkflowSnapshot},
};
use iocraft::prelude::*;
use std::time::Duration;
use tokio::sync::watch;

#[derive(Default, Props)]
pub(crate) struct QuotaStatusComponentProps {
    #[cfg(test)]
    pub(crate) dispatch_receipt:
        Option<agent_collaboration_tui::input_dispatch_receipt::InputDispatchReceiptSink>,
    #[cfg(test)]
    pub(crate) quit_observer: Option<Handler<()>>,
    #[cfg(test)]
    pub(crate) routed_outcome_observer: Option<Handler<Option<crate::QuotaOutcome>>>,
    pub(crate) quota_refresher: Option<crate::QuotaRefresher>,
    pub(crate) account_commands: Option<crate::QuotaAccountCommandQueue>,
    pub(crate) outcome_sink: Option<std::sync::Arc<std::sync::Mutex<Option<crate::QuotaOutcome>>>>,
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fixture_control: Option<crate::test_support::QuotaFixtureControl>,
    pub(crate) initial_focused_account_id: Option<codex_router_core::ids::AccountId>,
    pub(crate) notice: Option<String>,
    pub(crate) interaction_mode: QuotaInteractionMode,
    pub(crate) view_model: QuotaStatusViewModel,
    pub(crate) width: usize,
    pub(crate) height: usize,
    #[cfg(test)]
    pub(crate) viewport_trace: Option<super::quota_viewport_trace::QuotaViewportTrace>,
    #[cfg(test)]
    /// Synthetic resize tests nominate a reader; compiled PTY fixtures keep the default.
    pub(crate) terminal_width_reader: Option<fn() -> Option<usize>>,
    pub(crate) reload_view_model: Option<QuotaStatusViewModelLoader>,
    pub(crate) reload_interval: Duration,
    pub(crate) spinner_interval: Duration,
    pub(crate) reset_intent_sender: Option<ResetIntentSender>,
    pub(crate) reset_snapshot_receiver: Option<watch::Receiver<ResetWorkflowSnapshot>>,
    pub(crate) weekly_floor_saver: Option<WeeklyQuotaFloorSaver>,
    pub(crate) credit_policy_saver: Option<CreditUsagePolicySaver>,
    pub(crate) credit_usage_refresher: Option<CreditUsageRefresher>,
}
