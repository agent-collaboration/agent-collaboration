//! Machine keyboard and pointer commits share the existing source-reload boundary.
use crate::session_picker::screen::picker_component::ConversationPreviewCache;
use crate::session_picker::screen::picker_component::SessionRecordsReloadPort;
use crate::session_picker::screen::picker_component::SessionsPickerModel;
use crate::session_picker::screen::picker_component::State;
use crate::session_picker::source::picker_machine_controls::MachineChoiceKey;
use crate::session_picker::source::picker_machine_controls::PickerMachineStage;

#[derive(Clone)]
pub(crate) struct MachineSelectionContext {
    pub model: State<SessionsPickerModel>,
    pub can_load_source: bool,
    pub reload_port: SessionRecordsReloadPort,
    pub reload_generation: State<u64>,
    pub conversation_cache: State<ConversationPreviewCache>,
}

pub(crate) fn cancel_machine_choice(mut context: MachineSelectionContext) {
    let mut model = context.model.write();
    let was_loading = matches!(
        model.machine_controls.stage,
        PickerMachineStage::Loading { .. }
    );
    model.machine_controls.cancel_choice();
    if was_loading {
        context.conversation_cache.write().invalidate();
        let generation = context.reload_generation.get().saturating_add(1);
        context.reload_generation.set(generation);
        context
            .reload_port
            .send(model.prepare_source_reload(generation));
    }
}

pub(crate) fn select_machine_choice(mut context: MachineSelectionContext, key: MachineChoiceKey) {
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
