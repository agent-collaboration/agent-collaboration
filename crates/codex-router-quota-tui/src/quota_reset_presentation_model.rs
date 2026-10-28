use codex_router_core::ids::AccountId;

use codex_router_quota_service::quota_reset::reset_session_supervisor::ResetWorkflowSnapshot;
use codex_router_quota_service::quota_reset::reset_session_supervisor::WorkflowPhase;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResetPaneTarget {
    pub(crate) account_id: AccountId,
    pub(crate) active_credential_generation: u64,
    pub(crate) account_label: String,
    pub(crate) account_tag: String,
    pub(crate) saved_reset_credits: String,
    pub(crate) saved_weekly_window: String,
}

pub(crate) fn reset_mode(snapshot: Option<&ResetWorkflowSnapshot>) -> bool {
    snapshot.is_some_and(|snapshot| snapshot.phase() != WorkflowPhase::Browse)
}

pub(crate) fn credit_page_start(
    current_start: usize,
    credit_count: usize,
    page_size: usize,
    next_page: bool,
) -> usize {
    if credit_count == 0 || page_size == 0 {
        return 0;
    }
    let maximum_start = credit_count.saturating_sub(1) / page_size * page_size;
    if next_page {
        current_start.saturating_add(page_size).min(maximum_start)
    } else {
        current_start.saturating_sub(page_size)
    }
}

pub(crate) fn reset_inventory_page_size(detail_height: usize) -> usize {
    const INSPECTED_DETAIL_NON_CREDIT_ROWS: usize = 10;

    detail_height
        .saturating_sub(INSPECTED_DETAIL_NON_CREDIT_ROWS)
        .clamp(1, 4)
}
