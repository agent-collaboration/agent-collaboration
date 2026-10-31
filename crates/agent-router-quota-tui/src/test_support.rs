//! Test-only construction of the real quota control tree, with explicitly injected effects.
use crate::{QuotaOutcome, QuotaStatusViewOptions, quota_status_component::QuotaStatusComponent};
use codex_router_quota_service::quota_display_model::QuotaStatusViewModel;
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum QuotaFixtureControl {
    #[default]
    Browse,
    Details,
    Filters,
    Options,
    Credits,
    CreditChoices,
    ResetPhase,
    Floor,
}

pub struct QuotaFixtureOptions {
    pub view_model: QuotaStatusViewModel,
    pub ports: QuotaStatusViewOptions,
    pub width: u16,
    pub height: u16,
    pub control: QuotaFixtureControl,
}

pub struct QuotaControlFixture {
    pub element: AnyElement<'static>,
    pub outcome: Arc<Mutex<Option<QuotaOutcome>>>,
    #[cfg(test)]
    pub(crate) popup: crate::quota_test_support::PublishedQuotaPopup,
}

pub fn quota_control_fixture(inputs: QuotaFixtureOptions) -> QuotaControlFixture {
    let outcome = Arc::new(Mutex::new(None));
    let options = inputs.ports;
    let (reset_intent_sender, reset_snapshot_receiver) = options
        .reset_session_ports
        .map(|ports| (Some(ports.intent_sender), Some(ports.snapshot_receiver)))
        .unwrap_or((None, None));
    let element = element!(QuotaStatusComponent(
        view_model:inputs.view_model,width:usize::from(inputs.width),height:usize::from(inputs.height),fixture_control:Some(inputs.control),outcome_sink:Some(outcome.clone()),
        reload_view_model:options.reload_view_model,reset_intent_sender,reset_snapshot_receiver,weekly_floor_saver:options.weekly_floor_saver,credit_policy_saver:options.credit_policy_saver,credit_usage_refresher:options.credit_usage_refresher,
        quota_refresher:options.quota_refresher,account_commands:options.account_commands,initial_focused_account_id:options.focused_account_id,notice:options.notice,
    ));
    #[cfg(test)]
    let popup = crate::quota_test_support::PublishedQuotaPopup::default();
    #[cfg(test)]
    let element = {
        let mut element = element;
        element.props.surface_stack_observer = Some(popup.observer());
        element
    };
    QuotaControlFixture {
        element: element.into_any(),
        outcome,
        #[cfg(test)]
        popup,
    }
}

pub(crate) fn open_fixture_control(
    stack: agent_collaboration_tui::surface_handle::SurfaceStackHandle,
    context: &crate::quota_control_context::QuotaControlContext,
    control: QuotaFixtureControl,
) -> Result<(), String> {
    use crate::quota_control_context::QuotaDialogKind;
    match control {
        QuotaFixtureControl::Browse => Ok(()),
        QuotaFixtureControl::Details => crate::quota_dialog_controls::open_quota_dialog(
            stack,
            context.clone(),
            QuotaDialogKind::Account(context.capture_selected()?),
        ),
        QuotaFixtureControl::Filters => crate::quota_dialog_controls::open_quota_dialog(
            stack,
            context.clone(),
            QuotaDialogKind::Filters,
        ),
        QuotaFixtureControl::ResetPhase => {
            let target = context.capture_selected()?;
            let row = context
                .target_row(&target)
                .ok_or("Explicit reset fixture account is unavailable")?;
            let captured =
                crate::quota_account_options::events::reset_pane_target_for_account(&row)
                    .ok_or("Explicit reset fixture credentials are unavailable")?;
            let mut options = context.account_options;
            options.set(Some(
                crate::quota_account_options::AccountOptionsState::new(&row, 1),
            ));
            let mut reset_target = context.reset_target;
            reset_target.set(Some(captured));
            crate::quota_dialog_controls::open_quota_dialog(
                stack,
                context.clone(),
                QuotaDialogKind::Options,
            )
        }
        QuotaFixtureControl::Options
        | QuotaFixtureControl::Credits
        | QuotaFixtureControl::CreditChoices => {
            crate::quota_dialog_controls::open_account_options(stack, context.clone(), None)?;
            if matches!(
                control,
                QuotaFixtureControl::Credits | QuotaFixtureControl::CreditChoices
            ) {
                let mut options = context.account_options;
                let mut next = options.read().clone();
                if let Some(options) = next.as_mut() {
                    options.tab = crate::quota_account_options::AccountOptionsTab::Credits;
                    if control == QuotaFixtureControl::CreditChoices {
                        crate::quota_account_options::update_credit_policy_editor(
                            options,
                            crate::quota_account_options::AccountOptionsKeyAction::BeginEditing,
                            None,
                        );
                    }
                }
                options.set(next);
                crate::quota_dialog_controls::refresh_dialog_model(stack, context)?;
                if control == QuotaFixtureControl::CreditChoices {
                    let policy = context
                        .account_options
                        .read()
                        .as_ref()
                        .and_then(|options| options.editor.as_ref())
                        .map(|editor| editor.draft_policy)
                        .ok_or("Credit fixture editor unavailable")?;
                    let mut choices = context.credit_choice;
                    choices.set(crate::quota_combo_controls::credit_options(policy)?);
                    crate::quota_combo_controls::open_choice(
                        stack,
                        context.clone(),
                        crate::quota_combo_controls::QuotaChoiceField::Credit,
                    )?;
                }
            }
            Ok(())
        }
        QuotaFixtureControl::Floor => {
            crate::quota_dialog_controls::open_weekly_floor(stack, context.clone(), None)
        }
    }
}
