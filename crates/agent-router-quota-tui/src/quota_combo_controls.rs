//! Quota values use stable option keys; the shared ComboBox owns query and navigation.
use super::{
    quota_control_context::{
        QuotaControlContext, QuotaControlResult, action_id, quota_action, show_control_result,
    },
    quota_provider_filters::{AccountProviderFilter, DisabledAccountVisibility},
};
use agent_collaboration_tui::{
    actions::Placement,
    controls::{
        combo::{ComboBoxBinding, ComboBoxControl, ComboBoxState},
        list::{OptionKey, SelectOption},
    },
    surfaces::{
        focus_model::TargetId,
        surface_handle::{SurfaceContext, SurfaceStackHandle},
    },
};
use codex_router_core::{credit_usage::CreditUsagePolicy, provider::Provider};
use iocraft::prelude::*;

#[derive(Clone, Copy)]
pub(crate) enum QuotaChoiceField {
    Provider,
    Disabled,
    Group,
    Credit,
}

pub(crate) fn provider_options(value: AccountProviderFilter) -> QuotaControlResult<ComboBoxState> {
    choice_state(
        &[("all", "All"), ("openai", "OpenAI"), ("claude", "Claude")],
        match value {
            AccountProviderFilter::All => "all",
            AccountProviderFilter::Provider(Provider::Openai) => "openai",
            AccountProviderFilter::Provider(Provider::Claude) => "claude",
        },
    )
}
pub(crate) fn disabled_options(
    value: DisabledAccountVisibility,
) -> QuotaControlResult<ComboBoxState> {
    choice_state(
        &[("shown", "Shown"), ("hidden", "Hidden")],
        match value {
            DisabledAccountVisibility::Shown => "shown",
            DisabledAccountVisibility::Hidden => "hidden",
        },
    )
}
pub(crate) fn grouping_options(value: crate::QuotaGroupFacet) -> QuotaControlResult<ComboBoxState> {
    choice_state(
        &[
            ("none", "None"),
            ("provider", "Provider"),
            ("status", "Status"),
        ],
        value.stored_value(),
    )
}
pub(crate) fn credit_options(value: CreditUsagePolicy) -> QuotaControlResult<ComboBoxState> {
    choice_state(
        &[("disallow", "Disallow"), ("allow", "Allow")],
        match value {
            CreditUsagePolicy::Disallow => "disallow",
            CreditUsagePolicy::Allow => "allow",
        },
    )
}
fn choice_state(options: &[(&str, &str)], value: &str) -> QuotaControlResult<ComboBoxState> {
    let options = options
        .iter()
        .map(|(key, label)| {
            Ok(SelectOption::available(
                OptionKey::new(*key).map_err(|error| error.to_string())?,
                *label,
            ))
        })
        .collect::<QuotaControlResult<Vec<_>>>()?;
    ComboBoxState::new(
        options,
        Some(OptionKey::new(value).map_err(|error| error.to_string())?),
    )
    .map_err(|error| error.to_string())
}

pub(crate) fn choice_target(field: QuotaChoiceField) -> QuotaControlResult<TargetId> {
    Ok(TargetId::for_action(&action_id(match field {
        QuotaChoiceField::Provider => "provider-field",
        QuotaChoiceField::Disabled => "disabled-field",
        QuotaChoiceField::Group => "group-field",
        QuotaChoiceField::Credit => "credit-field",
    })?))
}

pub(crate) fn choice_action(
    stack: SurfaceStackHandle,
    context: QuotaControlContext,
    field: QuotaChoiceField,
) -> QuotaControlResult<agent_collaboration_tui::actions::ActionDef> {
    let (name, title) = match field {
        QuotaChoiceField::Provider => ("provider-field", "Account provider"),
        QuotaChoiceField::Disabled => ("disabled-field", "Disabled accounts"),
        QuotaChoiceField::Group => ("group-field", "Group by"),
        QuotaChoiceField::Credit => ("credit-field", "Use credits"),
    };
    quota_action(name, title, &[], Placement::Hidden, move |()| {
        let result = open_choice(stack, context.clone(), field);
        show_control_result(&context, result);
    })
}

pub(crate) fn open_choice(
    stack: SurfaceStackHandle,
    context: QuotaControlContext,
    field: QuotaChoiceField,
) -> QuotaControlResult<()> {
    let value = match field {
        QuotaChoiceField::Provider => context.provider_choice,
        QuotaChoiceField::Disabled => context.disabled_choice,
        QuotaChoiceField::Group => context.group_choice,
        QuotaChoiceField::Credit => context.credit_choice,
    };
    let parent_scroll = context
        .dialog
        .read()
        .as_ref()
        .map(|session| session.scroll.clone())
        .ok_or_else(|| "Quota dialog is unavailable".to_owned())?;
    let selected_context = context;
    let control = ComboBoxControl::new(ComboBoxBinding {
        parent: SurfaceContext {
            stack,
            surface: stack.snapshot().top_id(),
        },
        field: choice_target(field)?,
        parent_scroll: Some(parent_scroll),
        value,
        on_pick: Handler::from(move |key: OptionKey| {
            let mut filters = selected_context.filters;
            match field {
                QuotaChoiceField::Provider
                | QuotaChoiceField::Disabled
                | QuotaChoiceField::Group => {
                    let mut next = *filters.read();
                    if let Some(draft) = next.draft.as_mut() {
                        match (field, key.as_str()) {
                            (QuotaChoiceField::Provider, "all") => {
                                draft.filters.provider = AccountProviderFilter::All
                            }
                            (QuotaChoiceField::Provider, "openai") => {
                                draft.filters.provider =
                                    AccountProviderFilter::Provider(Provider::Openai)
                            }
                            (QuotaChoiceField::Provider, "claude") => {
                                draft.filters.provider =
                                    AccountProviderFilter::Provider(Provider::Claude)
                            }
                            (QuotaChoiceField::Disabled, "shown") => {
                                draft.filters.disabled = DisabledAccountVisibility::Shown
                            }
                            (QuotaChoiceField::Disabled, "hidden") => {
                                draft.filters.disabled = DisabledAccountVisibility::Hidden
                            }
                            (QuotaChoiceField::Group, "none") => {
                                draft.filters.group_by = crate::QuotaGroupFacet::None
                            }
                            (QuotaChoiceField::Group, "provider") => {
                                draft.filters.group_by = crate::QuotaGroupFacet::Provider
                            }
                            (QuotaChoiceField::Group, "status") => {
                                draft.filters.group_by = crate::QuotaGroupFacet::Status
                            }
                            _ => {
                                selected_context.report("Quota choice is unavailable");
                                return;
                            }
                        }
                    }
                    filters.set(next);
                }
                QuotaChoiceField::Credit => {
                    let action = match key.as_str() {
                        "allow" => {
                            super::quota_account_options::AccountOptionsKeyAction::SelectAllow
                        }
                        "disallow" => {
                            super::quota_account_options::AccountOptionsKeyAction::SelectDisallow
                        }
                        _ => {
                            selected_context.report("Credit choice is unavailable");
                            return;
                        }
                    };
                    selected_context.apply_options(action);
                }
            }
        }),
    });
    control
        .open()
        .map(|_| ())
        .map_err(|error| error.to_string())
}
