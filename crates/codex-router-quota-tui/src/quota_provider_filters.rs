use codex_router_core::ids::AccountId;
use codex_router_core::provider::Provider;
use iocraft::prelude::*;

use super::quota_account_options::AccountOptionsState;
use super::quota_floor_editor::WeeklyFloorEditorState;
use super::quota_reset_presentation_model::ResetPaneTarget;
use codex_router_quota_service::quota_display_model::{
    QuotaStatusAccountViewModel, QuotaStatusViewModel,
};
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
pub(crate) enum QuotaInteractionMode {
    #[default]
    Interactive,
    Static,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum AccountProviderFilter {
    #[default]
    All,
    Provider(Provider),
}

impl AccountProviderFilter {
    pub(crate) const CHOICES: [Self; 3] = [
        Self::All,
        Self::Provider(Provider::Openai),
        Self::Provider(Provider::Claude),
    ];

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
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Shown => "Shown",
            Self::Hidden => "Hidden",
        }
    }

    pub(crate) const fn toggled(self) -> Self {
        match self {
            Self::Shown => Self::Hidden,
            Self::Hidden => Self::Shown,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct QuotaAccountFilters {
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum QuotaFilterFocus {
    #[default]
    Provider,
    Disabled,
    Apply,
    Cancel,
}

impl QuotaFilterFocus {
    fn moved(self, backwards: bool) -> Self {
        match (self, backwards) {
            (Self::Provider, false) | (Self::Apply, true) => Self::Disabled,
            (Self::Disabled, false) | (Self::Cancel, true) => Self::Apply,
            (Self::Apply, false) | (Self::Provider, true) => Self::Cancel,
            (Self::Cancel, false) | (Self::Disabled, true) => Self::Provider,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct QuotaFilterDraft {
    pub(crate) filters: QuotaAccountFilters,
    pub(crate) focus: QuotaFilterFocus,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct QuotaFilterState {
    pub(crate) committed: QuotaAccountFilters,
    pub(crate) draft: Option<QuotaFilterDraft>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QuotaFilterValue {
    Provider(AccountProviderFilter),
    Disabled(DisabledAccountVisibility),
}

pub(crate) fn quota_filter_choices(
    field: QuotaFilterFocus,
) -> Vec<codex_router_tui::ChoiceOption<QuotaFilterValue>> {
    match field {
        QuotaFilterFocus::Provider => AccountProviderFilter::CHOICES
            .into_iter()
            .map(|provider| {
                codex_router_tui::ChoiceOption::available(
                    QuotaFilterValue::Provider(provider),
                    provider.label(),
                )
            })
            .collect(),
        QuotaFilterFocus::Disabled => [
            DisabledAccountVisibility::Shown,
            DisabledAccountVisibility::Hidden,
        ]
        .into_iter()
        .map(|visibility| {
            codex_router_tui::ChoiceOption::available(
                QuotaFilterValue::Disabled(visibility),
                visibility.label(),
            )
        })
        .collect(),
        QuotaFilterFocus::Apply | QuotaFilterFocus::Cancel => Vec::new(),
    }
}

#[derive(Clone, Copy)]
pub(crate) enum QuotaFilterAction {
    Open,
    Provider(AccountProviderFilter),
    ToggleDisabled,
    Disabled(DisabledAccountVisibility),
    Apply,
    Cancel,
}

#[derive(Clone, Copy)]
pub(crate) struct QuotaFilterController {
    pub(crate) state: State<QuotaFilterState>,
    pub(crate) choice: State<codex_router_tui::SearchSelectState<QuotaFilterValue>>,
    pub(crate) view_model: State<QuotaStatusViewModel>,
    pub(crate) focused_account_id: State<Option<AccountId>>,
}

impl QuotaFilterController {
    pub(crate) fn dispatch(mut self, action: QuotaFilterAction) {
        let mut next = *self.state.read();
        match action {
            QuotaFilterAction::Open => {
                self.choice.set(codex_router_tui::SearchSelectState::Closed);
                next.draft = Some(QuotaFilterDraft {
                    filters: next.committed,
                    focus: QuotaFilterFocus::Provider,
                })
            }
            QuotaFilterAction::Cancel => next.draft = None,
            QuotaFilterAction::Apply => {
                if let Some(draft) = next.draft.take() {
                    next.committed = draft.filters;
                    let rows = next.committed.visible_rows(&self.view_model.read().rows);
                    reconcile_quota_filter_focus(&rows, &mut self.focused_account_id);
                }
            }
            QuotaFilterAction::Provider(provider) => {
                if let Some(draft) = next.draft.as_mut() {
                    draft.filters.provider = provider;
                    draft.focus = QuotaFilterFocus::Provider;
                }
            }
            QuotaFilterAction::Disabled(visibility) => {
                if let Some(draft) = next.draft.as_mut() {
                    draft.filters.disabled = visibility;
                    draft.focus = QuotaFilterFocus::Disabled;
                }
            }
            QuotaFilterAction::ToggleDisabled => {
                if let Some(draft) = next.draft.as_mut() {
                    draft.filters.disabled = draft.filters.disabled.toggled();
                    draft.focus = QuotaFilterFocus::Disabled;
                }
            }
        }
        self.state.set(next);
    }

    pub(crate) fn open_choice(mut self, field: QuotaFilterFocus) {
        let mut next = *self.state.read();
        if let Some(draft) = next.draft.as_mut() {
            draft.focus = field;
            let value = match field {
                QuotaFilterFocus::Provider => QuotaFilterValue::Provider(draft.filters.provider),
                QuotaFilterFocus::Disabled => QuotaFilterValue::Disabled(draft.filters.disabled),
                QuotaFilterFocus::Apply | QuotaFilterFocus::Cancel => return,
            };
            self.choice.write().open(value);
            self.state.set(next);
        }
    }
    pub(crate) fn select_choice(mut self, value: QuotaFilterValue) {
        match value {
            QuotaFilterValue::Provider(provider) => {
                self.dispatch(QuotaFilterAction::Provider(provider))
            }
            QuotaFilterValue::Disabled(visibility) => {
                self.dispatch(QuotaFilterAction::Disabled(visibility))
            }
        }
        self.choice.set(codex_router_tui::SearchSelectState::Closed);
    }
    pub(crate) fn handle_key(mut self, code: KeyCode, modifiers: KeyModifiers) -> bool {
        let Some(mut draft) = self.state.read().draft else {
            return false;
        };
        if self.choice.read().is_open() {
            let choices = quota_filter_choices(draft.focus);
            let transition = self.choice.write().handle_key(code, &choices);
            if let codex_router_tui::ChoiceTransition::Selected(value) = transition {
                self.select_choice(value);
            }
            return true;
        }
        match code {
            KeyCode::Enter
                if matches!(
                    draft.focus,
                    QuotaFilterFocus::Provider | QuotaFilterFocus::Disabled
                ) =>
            {
                self.open_choice(draft.focus)
            }
            KeyCode::Esc => self.dispatch(QuotaFilterAction::Cancel),
            KeyCode::Enter => self.dispatch(if draft.focus == QuotaFilterFocus::Cancel {
                QuotaFilterAction::Cancel
            } else {
                QuotaFilterAction::Apply
            }),
            KeyCode::Tab | KeyCode::BackTab => {
                draft.focus = draft
                    .focus
                    .moved(code == KeyCode::BackTab || modifiers.contains(KeyModifiers::SHIFT));
                let mut next = *self.state.read();
                next.draft = Some(draft);
                self.state.set(next);
            }
            KeyCode::Up | KeyCode::Down => {
                draft.focus = draft.focus.moved(code == KeyCode::Up);
                let mut next = *self.state.read();
                next.draft = Some(draft);
                self.state.set(next);
            }
            KeyCode::Char(' ') if draft.focus == QuotaFilterFocus::Disabled => {
                self.dispatch(QuotaFilterAction::ToggleDisabled)
            }
            _ => {}
        }
        true
    }
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

pub(crate) fn opens_quota_filters(code: KeyCode, modifiers: KeyModifiers) -> bool {
    code == KeyCode::F(3)
        || (code == KeyCode::Char('f') && modifiers.contains(KeyModifiers::CONTROL))
}
