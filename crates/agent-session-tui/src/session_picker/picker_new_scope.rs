//! New keeps the captured draft and dispatch owner; controls own field navigation.
use super::picker_choice_scope::{declare_combo, fields};
use super::{
    ActionDef, ActionId, AvailabilityFn, HelpControl, PickerDialogKind, PickerSurfaceContext,
    Placement, SurfaceResult, SurfaceStackHandle, action, target,
};
use agent_collaboration_tui::{
    actions::Availability,
    surfaces::focus_model::{GroupId, TargetSpec},
};

pub(super) fn declare_new(
    context: &PickerSurfaceContext,
    stack: SurfaceStackHandle,
    help: &HelpControl,
    actions: &mut Vec<ActionDef>,
    targets: &mut Vec<TargetSpec>,
) -> SurfaceResult<ActionId> {
    for field in fields(context, PickerDialogKind::New) {
        declare_combo(context, stack, help, field, actions, targets)?;
    }
    targets
        .push(TargetSpec::new(target("new-directory")?).in_group(GroupId::new("dialog-controls")?));
    let effect = context.clone();
    let help = help.clone();
    let mut start = action(
        "new-start",
        "Start",
        &["Ctrl+Enter"],
        Placement::Toolbar,
        move |()| {
            if !effect.owns_dialog_input(stack, PickerDialogKind::New) {
                return;
            }
            let mut model = effect.model;
            let selected = model.write().submit_new_form();
            if let Some(selected) = selected {
                let mut outcome = effect.outcome;
                outcome.set(Some(selected));
            }
            effect.synchronize_dialog(stack, &help);
        },
    )?;
    let state = context.model;
    start.availability = AvailabilityFn::new(move || {
        let model = state.read();
        let Some(draft) = model.new_session.as_ref() else {
            return Availability::Unavailable {
                reason: "New session is closed".into(),
            };
        };
        if model.client_action.is_active() || model.source_action.pending.is_some() {
            return Availability::Unavailable {
                reason: "Already starting".into(),
            };
        }
        match crate::session_picker::new_session_form::client_availability(draft, draft.client) {
            Availability::Unavailable { reason } => {
                return Availability::Unavailable { reason };
            }
            Availability::Available => {}
        }
        crate::session_picker::new_session_form::machine_choices(&model)
            .into_iter()
            .find(|choice| choice.key == draft.selected_source)
            .map_or(
                Availability::Unavailable {
                    reason: "Selected machine is unavailable".into(),
                },
                |choice| choice.availability,
            )
    });
    let primary = start.id.clone();
    actions.push(start);
    Ok(primary)
}
