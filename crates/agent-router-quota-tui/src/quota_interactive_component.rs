//! One InputOwner mounts quota's declared screen and overlay scopes.
use super::{
    quota_control_context::{
        QuotaControlContext, QuotaDialogKind, action_id, close_quota_dialog, show_control_result,
    },
    quota_status_props::QuotaStatusComponentProps,
    quota_view_configuration::{
        LIVE_QUOTA_STATUS_SPINNER_INTERVAL, MIN_QUOTA_WIDTH, MIN_RENDER_HEIGHT,
    },
};
use agent_collaboration_tui::{
    controls::HelpControl,
    input_owner::InputOwner,
    overlay_host::OverlayHost,
    surface_handle::{SurfaceStackHandle, use_surface_stack},
    surface_stack::SurfaceId,
};
use iocraft::prelude::*;
use std::sync::Arc;

pub(crate) fn render_interactive_quota_status(
    props: &QuotaStatusComponentProps,
    mut hooks: Hooks,
) -> AnyElement<'static> {
    let defaults = (
        super::quota_combo_controls::provider_options(Default::default()),
        super::quota_combo_controls::disabled_options(Default::default()),
        super::quota_combo_controls::credit_options(
            codex_router_core::credit_usage::CreditUsagePolicy::Disallow,
        ),
    );
    let (Ok(provider), Ok(disabled), Ok(credit)) = defaults else {
        return element!(Text(content:"Quota control options are invalid",color:Color::Red))
            .into_any();
    };
    let initial_focus = props.initial_focused_account_id.clone().or_else(|| {
        props
            .view_model
            .rows
            .iter()
            .find(|row| row.selected)
            .or_else(|| props.view_model.rows.first())
            .map(|row| row.account_id.clone())
    });
    let initial_selection = match super::quota_list_controls::initial_account_selection(
        &props.view_model,
        initial_focus.as_ref(),
    ) {
        Ok(value) => value,
        Err(error) => return element!(Text(content:error,color:Color::Red)).into_any(),
    };
    let view_model = hooks.use_state(|| props.view_model.clone());
    let mut refresh_receiver = hooks.use_ref(|| None);
    let refresh_sender = hooks.use_memo(
        || {
            let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
            refresh_receiver.set(Some(receiver));
            sender
        },
        (),
    );
    let context = QuotaControlContext {
        #[cfg(test)]
        viewport_trace: props.viewport_trace.clone(),
        view_model,
        filters: hooks.use_state(Default::default),
        provider_choice: hooks.use_state(|| provider),
        disabled_choice: hooks.use_state(|| disabled),
        credit_choice: hooks.use_state(|| credit),
        focused_account_id: hooks.use_state(|| initial_focus),
        account_selection: hooks.use_state(|| initial_selection),
        dialog: hooks.use_state(|| None),
        remove_confirmation: hooks.use_state(|| false),
        account_options: hooks.use_state(|| None),
        account_options_generation: hooks.use_state(|| 0),
        options_switch_available_width: hooks.use_state(|| 0),
        account_options_commands: hooks.use_memo(
            super::quota_account_options::AccountOptionsCommandPort::new,
            (),
        ),
        weekly_floor_editor: hooks.use_state(|| None),
        weekly_floor_commands: hooks.use_memo(
            super::quota_floor_editor::WeeklyFloorEditorCommandPort::new,
            (),
        ),
        weekly_floor_available: props.weekly_floor_saver.is_some(),
        reset_snapshot: hooks.use_state(|| {
            props
                .reset_snapshot_receiver
                .as_ref()
                .map(|receiver| receiver.borrow().clone())
        }),
        reset_target: hooks.use_state(|| None),
        reset_intents: props.reset_intent_sender.clone(),
        inventory_page_start: hooks.use_state(|| 0),
        spinner_tick: hooks.use_state(|| 0),
        width: hooks.use_state(|| {
            if props.width == 0 {
                #[cfg(test)]
                let initial = props
                    .terminal_width_reader
                    .and_then(|reader| reader())
                    .unwrap_or(MIN_QUOTA_WIDTH);
                #[cfg(not(test))]
                let initial = props.view_model.width;
                initial.max(MIN_QUOTA_WIDTH)
            } else {
                props.width.max(MIN_QUOTA_WIDTH)
            }
        }),
        height: hooks.use_state(|| {
            if props.height == 0 {
                MIN_RENDER_HEIGHT
            } else {
                props.height.max(1)
            }
        }),
        outcome: hooks.use_state(|| None),
        should_exit: hooks.use_state(|| false),
        notice: hooks.use_state(|| props.notice.clone()),
        account_commands: props.account_commands.clone(),
        account_pending: hooks.use_state(|| None),
        refresh: hooks.use_state(Default::default),
        refresh_sender,
        refresh_available: props.quota_refresher.is_some(),
    };
    let mut screen_controls = hooks.use_ref(|| None);
    let model_context = context.clone();
    let stack = use_surface_stack(&mut hooks, move |stack| {
        let result = action_id("help").and_then(|id| {
            let list = super::quota_list_controls::account_list_control(stack, &model_context)?;
            let help = HelpControl::new(stack, id);
            let model = super::quota_screen_controls::quota_screen_model(
                stack,
                model_context.clone(),
                help.clone(),
                &list,
            )?;
            screen_controls.set(Some((list, help)));
            Ok(model)
        });
        match result {
            Ok(model) => model,
            Err(error) => {
                model_context.report(error);
                stack.snapshot().top().clone()
            }
        }
    });
    let Some((list, _help)) = screen_controls.read().clone() else {
        return element!(Text(content:"Quota controls could not start",color:Color::Red))
            .into_any();
    };
    #[cfg(any(test, feature = "test-support"))]
    {
        let mut fixture_opened = hooks.use_ref(|| false);
        if !fixture_opened.get() && stack.viewport() != (0, 0) {
            if let Some(control) = props.fixture_control {
                show_control_result(
                    &context,
                    super::test_support::open_fixture_control(stack, &context, control),
                );
            }
            fixture_opened.set(true);
        }
    }
    use_quota_scope_updates(&mut hooks, stack, &context, &list);
    let reload_lock = hooks.use_memo(|| Arc::new(tokio::sync::Mutex::new(())), ());
    super::quota_background_updates::use_quota_background_updates(
        &mut hooks,
        super::quota_background_updates::QuotaBackgroundUpdates {
            weekly_floor_saver: props.weekly_floor_saver.clone(),
            reload_view_model: props.reload_view_model.clone(),
            credit_policy_saver: props.credit_policy_saver.clone(),
            credit_usage_refresher: props.credit_usage_refresher.clone(),
            reset_intent_sender: props.reset_intent_sender.clone(),
            reset_snapshot_receiver: props.reset_snapshot_receiver.clone(),
            reload_interval: props.reload_interval,
            reload_lock: reload_lock.clone(),
            weekly_floor_command_port: context.weekly_floor_commands.clone(),
            account_options_command_port: context.account_options_commands.clone(),
            view_model,
            weekly_floor_editor: context.weekly_floor_editor,
            account_options: context.account_options,
            reset_snapshot: context.reset_snapshot,
            reset_target: context.reset_target,
            inventory_page_start: context.inventory_page_start,
        },
    );
    super::quota_control_effects::use_quota_control_effects(
        &mut hooks,
        super::quota_control_effects::QuotaControlEffects {
            context: context.clone(),
            stack,
            refresher: props.quota_refresher.clone(),
            refresh_requests: refresh_receiver.write().take(),
            loader: props.reload_view_model.clone(),
            reload_lock,
        },
    );
    super::responsive_quota_layout::use_quota_status_spinner(
        &mut hooks,
        context.spinner_tick,
        if props.spinner_interval.is_zero() {
            LIVE_QUOTA_STATUS_SPINNER_INTERVAL
        } else {
            props.spinner_interval
        },
    );
    let mut system = hooks.use_context_mut::<SystemContext>();
    let outcome = context.outcome.read().clone().or_else(|| {
        context
            .should_exit
            .get()
            .then_some(super::QuotaOutcome::Quit)
    });
    if let Some(outcome) = outcome {
        if let Some(sink) = &props.outcome_sink {
            *sink
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(outcome);
        }
        #[cfg(test)]
        if let Some(observer) = &props.quit_observer {
            observer(());
        }
        system.exit();
    }
    let width = u16::try_from(context.width.get()).unwrap_or(u16::MAX);
    let height = u16::try_from(context.height.get()).unwrap_or(u16::MAX);
    let quit = match action_id("quit") {
        Ok(id) => id,
        Err(error) => return element!(Text(content:error,color:Color::Red)).into_any(),
    };
    let routed_context = context.clone();
    #[cfg(test)]
    let routed_outcome_observer = props.routed_outcome_observer.clone();
    element!(InputOwner(stack,width,height,quit_action:quit,on_routed:move |_| {
        let (width,height)=stack.viewport();let mut observed_width=routed_context.width;let mut observed_height=routed_context.height;
        if observed_width.get()!=usize::from(width) {observed_width.set(usize::from(width));}
        if observed_height.get()!=usize::from(height) {observed_height.set(usize::from(height));}
        if let Some(session)=routed_context.dialog.read().clone() && stack.snapshot().surface(SurfaceId::Overlay(session.id)).is_none() {let mut dialog=routed_context.dialog;dialog.set(None);}
        #[cfg(test)] if let Some(observer)=&routed_outcome_observer {observer(routed_context.outcome.read().clone());}
    }) {OverlayHost {super::quota_control_browse_view::QuotaControlBrowse(context,list)}}).into_any()
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DialogModelRevision {
    kind: QuotaDialogKind,
    confirmation: bool,
    phase: codex_router_quota_service::quota_reset::reset_session_supervisor::WorkflowPhase,
    reset_selection: Option<
        codex_router_quota_service::quota_reset::reset_session_supervisor::ConfirmationSelection,
    >,
    options: Option<(
        super::quota_account_options::AccountOptionsTab,
        Option<super::quota_account_options::CreditPolicyEditorPhase>,
        bool,
    )>,
    floor: Option<super::quota_floor_editor::WeeklyFloorEditorPhase>,
    account: Option<(
        bool,
        codex_router_quota_service::quota_display_model::QuotaLoginState,
    )>,
}

fn use_quota_scope_updates(
    hooks: &mut Hooks<'_, '_>,
    stack: SurfaceStackHandle,
    context: &QuotaControlContext,
    list: &agent_collaboration_tui::controls::ListSelectionControl,
) {
    show_control_result(
        context,
        super::quota_list_controls::synchronize_account_selection(list, context),
    );
    let mut previous = hooks.use_state(|| None);
    let session = context.dialog.read().clone();
    if let Some(session) = session {
        let closed = match session.kind {QuotaDialogKind::Floor=>context.weekly_floor_editor.read().is_none(),QuotaDialogKind::Options=>context.account_options.read().is_none() && context.reset_target.read().is_none() && context.phase()==codex_router_quota_service::quota_reset::reset_session_supervisor::WorkflowPhase::Browse,_=>false};
        if closed {
            close_quota_dialog(stack, context);
            previous.set(None);
            return;
        }
        let options = context.account_options.read().as_ref().map(|options| {
            (
                options.tab,
                options.editor.as_ref().map(|editor| editor.phase),
                options.pending_tab.is_some(),
            )
        });
        let account = if let QuotaDialogKind::Account(target) = &session.kind {
            context
                .target_row(target)
                .map(|row| (row.enabled, row.login_state))
        } else {
            None
        };
        let revision = DialogModelRevision {
            kind: session.kind,
            confirmation: context.remove_confirmation.get(),
            phase: context.phase(),
            reset_selection: context
                .reset_snapshot
                .read()
                .as_ref()
                .map(|snapshot| snapshot.confirmation_selection()),
            options,
            floor: context
                .weekly_floor_editor
                .read()
                .as_ref()
                .map(|editor| editor.phase),
            account,
        };
        if previous.read().as_ref() != Some(&revision) {
            show_control_result(
                context,
                super::quota_dialog_controls::refresh_dialog_model(stack, context),
            );
            previous.set(Some(revision));
        }
    } else if previous.read().is_some() {
        previous.set(None);
    }
}
