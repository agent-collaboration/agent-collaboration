use codex_router_core::ids::AccountId;
use codex_router_core::provider::Provider;
use iocraft::prelude::*;

use super::quota_account_options::AccountOptionsState;
use super::quota_floor_editor::WeeklyFloorEditorState;
use super::quota_reset_presentation_model::ResetPaneTarget;
use codex_router_quota_service::quota_display_model::QuotaStatusAccountViewModel;
use codex_router_quota_service::quota_reset::reset_session_supervisor::{
    ResetWorkflowSnapshot, WorkflowPhase,
};

pub(crate) fn quota_filter_modal_owns_input(
    account_options: Option<&AccountOptionsState>,
    floor_editor: Option<&WeeklyFloorEditorState>,
    reset_snapshot: Option<&ResetWorkflowSnapshot>,
    reset_target: Option<&ResetPaneTarget>,
) -> bool {
    account_options.is_some()
        || floor_editor.is_some()
        || reset_target.is_some()
        || reset_snapshot.is_some_and(|snapshot| snapshot.phase() != WorkflowPhase::Browse)
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum AccountProviderFilter {
    #[default]
    All,
    Provider(Provider),
}

impl AccountProviderFilter {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Provider(Provider::Openai) => "OpenAI",
            Self::Provider(Provider::Claude) => "Claude",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum DisabledAccountVisibility {
    #[default]
    Shown,
    Hidden,
}

impl DisabledAccountVisibility {
    #[cfg(test)]
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Shown => "Shown",
            Self::Hidden => "Hidden",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct QuotaAccountFilters {
    pub(crate) group_by: crate::QuotaGroupFacet,
    pub(crate) provider: AccountProviderFilter,
    pub(crate) disabled: DisabledAccountVisibility,
}

impl QuotaAccountFilters {
    fn includes_row(self, row: &QuotaStatusAccountViewModel) -> bool {
        (self.provider == AccountProviderFilter::All
            || self.provider == AccountProviderFilter::Provider(row.provider))
            && (self.disabled == DisabledAccountVisibility::Shown || row.enabled)
    }

    pub(crate) fn visible_rows(
        self,
        rows: &[QuotaStatusAccountViewModel],
    ) -> Vec<QuotaStatusAccountViewModel> {
        rows.iter()
            .filter(|row| self.includes_row(row))
            .cloned()
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct QuotaFilterDraft {
    pub(crate) filters: QuotaAccountFilters,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct QuotaFilterState {
    pub(crate) committed: QuotaAccountFilters,
    pub(crate) draft: Option<QuotaFilterDraft>,
}

pub(crate) fn reconcile_quota_filter_focus(
    rows: &[QuotaStatusAccountViewModel],
    focused: &mut State<Option<AccountId>>,
) {
    let retained = focused
        .read()
        .as_ref()
        .is_some_and(|account_id| rows.iter().any(|row| &row.account_id == account_id));
    if !retained {
        let next = rows.first().map(|row| row.account_id.clone());
        if *focused.read() != next {
            focused.set(next);
        }
    }
}
