//! Machine keyboard and pointer commits share the existing source-reload boundary.
use super::*;
use crate::presentation::session_picker::picker_machine_controls::{
    MachineChoiceKey, PickerMachineStage,
};
use codex_router_tui::ChoiceTransition;

#[derive(Clone)]
pub(super) struct MachineSelectionContext {
    pub model: State<SessionsPickerModel>,
    pub can_load_source: bool,
    pub reload_port: SessionRecordsReloadPort,
    pub reload_generation: State<u64>,
    pub conversation_cache: State<ConversationPreviewCache>,
}

pub(super) fn select_machine_choice(mut context: MachineSelectionContext, key: MachineChoiceKey) {
    let mut model = context.model.write();
    let registry = model.request.router_registry.clone();
    let mode = model.request.machine_mode;
    let options = model.machine_controls.choice_options(&registry, mode);
    let Some(index) = options.iter().position(|option| option.key == key) else {
        return;
    };
    let PickerMachineStage::Choosing { focus, .. } = &mut model.machine_controls.stage else {
        return;
    };
    *focus = index;
    let previous_filter = model.machine_controls.filter.clone();
    model
        .machine_controls
        .select_choice(&registry, mode, context.can_load_source);
    if matches!(
        model.machine_controls.stage,
        PickerMachineStage::Choosing { .. }
    ) {
        model.machine_controls.search.open(key);
    }
    context.conversation_cache.write().invalidate();
    if model.machine_controls.filter != previous_filter
        || matches!(
            model.machine_controls.stage,
            PickerMachineStage::Loading { .. }
        )
    {
        let generation = context.reload_generation.get().saturating_add(1);
        context.reload_generation.set(generation);
        context
            .reload_port
            .send(model.prepare_source_reload(generation));
    }
}

pub(super) fn handle_machine_choice_key(context: MachineSelectionContext, code: KeyCode) {
    let transition = {
        let mut state = context.model;
        let mut model = state.write();
        let options = model
            .machine_controls
            .choice_options(&model.request.router_registry, model.request.machine_mode);
        model.machine_controls.search.handle_key(code, &options)
    };
    match transition {
        ChoiceTransition::Selected(key) => select_machine_choice(context, key),
        ChoiceTransition::Cancelled => {
            let mut state = context.model;
            state.write().machine_controls.cancel_choice();
        }
        ChoiceTransition::Handled | ChoiceTransition::Unhandled => {}
    }
}
