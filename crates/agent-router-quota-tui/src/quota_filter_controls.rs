//! The form commits domain filter values; dropdowns keep their own draft query.
use super::{
    quota_combo_controls::{QuotaChoiceField, choice_action, choice_target},
    quota_control_context::{
        QuotaControlContext, QuotaControlResult, QuotaDialogControls, close_quota_dialog,
        quota_action,
    },
};
use agent_collaboration_tui::{
    actions::Placement,
    controls::button_target,
    focus_model::{GroupId, TargetId},
    surface_handle::SurfaceStackHandle,
};

pub(crate) fn filter_dialog_controls(
    stack: SurfaceStackHandle,
    context: QuotaControlContext,
) -> QuotaControlResult<QuotaDialogControls> {
    let group = GroupId::new("quota-filter-fields").map_err(|error| error.to_string())?;
    let provider = choice_action(stack, context.clone(), QuotaChoiceField::Provider)?;
    let disabled = choice_action(stack, context.clone(), QuotaChoiceField::Disabled)?;
    let apply_context = context.clone();
    let apply = quota_action(
        "apply-filters",
        "Apply",
        &["Ctrl+Enter"],
        Placement::Toolbar,
        move |()| {
            let mut filters = apply_context.filters;
            let mut next = *filters.read();
            if let Some(draft) = next.draft.take() {
                next.committed = draft.filters;
            }
            filters.set(next);
            let rows = next
                .committed
                .visible_rows(&apply_context.view_model.read().rows);
            let mut focused = apply_context.focused_account_id;
            super::quota_provider_filters::reconcile_quota_filter_focus(&rows, &mut focused);
            close_quota_dialog(stack, &apply_context);
        },
    )?;
    let cancel_context = context;
    let cancel = quota_action(
        "cancel-filters",
        "Cancel",
        &["Esc"],
        Placement::Toolbar,
        move |()| super::quota_dialog_controls::close_dialog(stack, &cancel_context),
    )?;
    let primary = Some(apply.id.clone());
    let targets = vec![
        button_target(choice_target(QuotaChoiceField::Provider)?, &provider)
            .in_group(group.clone()),
        button_target(choice_target(QuotaChoiceField::Disabled)?, &disabled).in_group(group),
        button_target(TargetId::for_action(&apply.id), &apply),
        button_target(TargetId::for_action(&cancel.id), &cancel),
    ];
    Ok((vec![provider, disabled, apply, cancel], targets, primary))
}
