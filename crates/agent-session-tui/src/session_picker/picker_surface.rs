//! Picker declarations and effects; terminal routing belongs to the shared crate.
use super::{ConversationPreviewCache, SessionsPickerModel, SessionsPickerOutcome};
use crate::session_picker::{
    picker_machine_controls::{PickerMachineFilter, PickerMachineStage},
    session_id_clipboard::SessionClipboardWriter,
    source_reload_port::SessionRecordsReloadPort,
};
use agent_collaboration_tui::{
    actions::{ActionDef, ActionId, ActionRegistry, AvailabilityFn, Placement, ScopeId},
    controls::{
        combo::ComboBoxState,
        fields::TextFieldState,
        list::{ListSelectionControl, ListSelectionState},
    },
    dialog::help::HelpControl,
    surfaces::{
        focus_model::{FocusModel, TargetId},
        surface_handle::SurfaceStackHandle,
        surface_stack::{OverlayId, SurfaceId, SurfaceModel},
    },
};
use iocraft::prelude::*;

#[path = "picker_choice_scope.rs"]
mod picker_choice_scope;
#[path = "picker_dialog_scope.rs"]
mod picker_dialog_scope;
#[path = "picker_help_content.rs"]
mod picker_help_content;
#[path = "picker_new_scope.rs"]
mod picker_new_scope;
#[path = "picker_screen_scope.rs"]
mod picker_screen_scope;
#[path = "picker_surface_view.rs"]
mod picker_surface_view;
#[path = "picker_toolbar.rs"]
mod picker_toolbar;

pub(super) type SurfaceResult<TResult> = Result<TResult, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PickerDialogKind {
    New,
    Details,
    Filters,
    Machine,
    Fork,
    Source,
    Client,
}

#[derive(Clone)]
pub(super) struct ActivePickerDialog {
    pub id: OverlayId,
    pub kind: PickerDialogKind,
    pub scroll: agent_collaboration_tui::controls::list::ScrollHandle,
    pub targets: Vec<TargetId>,
}

#[derive(Clone)]
pub(super) struct PickerSurfaceContext {
    pub model: State<SessionsPickerModel>,
    pub outcome: State<Option<SessionsPickerOutcome>>,
    pub cancelled: State<bool>,
    pub error: State<Option<String>>,
    pub reload_port: SessionRecordsReloadPort,
    pub reload_generation: State<u64>,
    pub conversation_cache: State<ConversationPreviewCache>,
    pub copier: SessionClipboardWriter,
    pub can_load_source: bool,
    pub filter: State<TextFieldState>,
    pub list: State<ListSelectionState>,
    pub list_control: State<Option<ListSelectionControl>>,
    pub help: State<Option<HelpControl>>,
    pub combos: [State<ComboBoxState>; 11],
    pub active_dialog: State<Option<ActivePickerDialog>>,
}

impl PickerSurfaceContext {
    pub fn is_closing(&self) -> bool {
        self.outcome.read().is_some() || self.cancelled.get()
    }

    pub fn owns_dialog_input(&self, stack: SurfaceStackHandle, kind: PickerDialogKind) -> bool {
        if self.is_closing() {
            return false;
        }
        self.active_dialog.read().as_ref().is_some_and(|dialog| {
            dialog.kind == kind && stack.snapshot().top_id() == SurfaceId::Overlay(dialog.id)
        })
    }
    pub fn report(&self, error: impl std::fmt::Display) {
        let mut state = self.error;
        let message = error.to_string();
        if state.read().as_ref() != Some(&message) {
            state.set(Some(message));
        }
    }

    pub fn refresh(&self) {
        if self.is_closing() {
            return;
        }
        let mut cache = self.conversation_cache;
        cache.write().invalidate();
        let mut generation = self.reload_generation;
        let next = generation.get().saturating_add(1);
        generation.set(next);
        let mut model = self.model;
        let request = model.write().prepare_source_reload(next);
        self.reload_port.send(request);
    }

    pub fn change_query(&self, change: impl FnOnce(&mut SessionsPickerModel)) {
        if self.is_closing() {
            return;
        }
        let mut model = self.model;
        let previous = model.read().data_query();
        {
            let mut value = model.write();
            change(&mut value);
            value.rebuild_browse_choices();
        }
        if model.read().data_query() != previous {
            self.refresh();
        }
    }

    pub fn quit(&self) {
        if self.is_closing() {
            return;
        }
        let interrupted =
            crate::session_picker::client_action_state::interrupt_client_action(&self.model.read());
        if let Some(interrupted) = interrupted {
            let mut outcome = self.outcome;
            outcome.set(Some(interrupted));
        } else {
            let mut model = self.model;
            model.write().cancel_source_action();
            let mut cancelled = self.cancelled;
            cancelled.set(true);
        }
    }

    pub fn escape_screen(&self) {
        let mut model = self.model;
        if !model.read().search.is_empty() {
            self.change_query(|model| model.handle_key(super::SessionsPickerKey::ClearSearch));
            let mut filter = self.filter;
            filter.set(TextFieldState::default());
            return;
        }
        if model.read().machine_controls.filter != PickerMachineFilter::Default {
            let mode = model.read().request.machine_mode;
            model
                .write()
                .machine_controls
                .return_to_default(mode, self.can_load_source);
            self.refresh();
        }
        self.change_query(|model| {
            model.root = model.request.root;
            model.provider = model.request.provider.clone();
            model.source = model.request.source;
            model.sort = model.request.sort;
            model.runtime_view = Default::default();
            model.provider_filter = Default::default();
            model.set_width(model.width);
        });
    }

    pub fn desired_dialog(&self) -> Option<PickerDialogKind> {
        let model = self.model.read();
        if model.client_action.is_active() {
            Some(PickerDialogKind::Client)
        } else if model.source_action.pending.is_some() {
            Some(PickerDialogKind::Source)
        } else if model.new_session.is_some() {
            Some(PickerDialogKind::New)
        } else if model.fork_confirmation.is_some() {
            Some(PickerDialogKind::Fork)
        } else if !matches!(model.machine_controls.stage, PickerMachineStage::Browsing) {
            Some(PickerDialogKind::Machine)
        } else if model.provider_filter_draft.is_some() {
            Some(PickerDialogKind::Filters)
        } else if model.details_dialog.is_some() {
            Some(PickerDialogKind::Details)
        } else {
            None
        }
    }

    pub fn synchronize_dialog(&self, stack: SurfaceStackHandle, help: &HelpControl) {
        if let Err(error) = picker_dialog_scope::synchronize(self, stack, help) {
            self.report(error);
        }
    }

    pub fn close_dialog(&self, stack: SurfaceStackHandle, kind: PickerDialogKind) {
        let mut model = self.model;
        match kind {
            PickerDialogKind::New => model.write().new_session = None,
            PickerDialogKind::Details => model.write().details_dialog = None,
            PickerDialogKind::Filters => model.write().provider_filter_draft = None,
            PickerDialogKind::Fork => model.write().fork_confirmation = None,
            PickerDialogKind::Source => model.write().cancel_source_action(),
            PickerDialogKind::Machine => {
                super::picker_machine_selection::cancel_machine_choice(
                    super::picker_machine_selection::MachineSelectionContext {
                        model,
                        can_load_source: self.can_load_source,
                        reload_port: self.reload_port.clone(),
                        reload_generation: self.reload_generation,
                        conversation_cache: self.conversation_cache,
                    },
                );
            }
            PickerDialogKind::Client => {
                crate::session_picker::client_action_view::finish_or_detach(model, self.outcome)
            }
        }
        if self.active_dialog.read().as_ref().is_some_and(|dialog| {
            stack
                .snapshot()
                .surface(SurfaceId::Overlay(dialog.id))
                .is_some()
        }) {
            stack.close_top();
        }
        let mut active = self.active_dialog;
        active.set(None);
    }
}

pub(super) fn action(
    name: &str,
    title: impl Into<agent_collaboration_tui::actions::ActionTitle>,
    keys: &[&str],
    placement: Placement,
    run: impl Fn(()) + Send + Sync + 'static,
) -> SurfaceResult<ActionDef> {
    Ok(ActionDef {
        id: ActionId::new(name)?,
        title: title.into(),
        toolbar_label: None,
        keys: keys
            .iter()
            .map(|key| key.parse())
            .collect::<Result<_, _>>()?,
        availability: AvailabilityFn::default(),
        choice: None,
        placement,
        run: Handler::from(run),
    })
}

pub(super) fn target(name: &str) -> SurfaceResult<TargetId> {
    Ok(TargetId::new(name)?)
}

pub(super) fn empty_model() -> SurfaceResult<SurfaceModel> {
    Ok(SurfaceModel {
        focus: FocusModel::new(Vec::new())?,
        actions: ActionRegistry::new(ScopeId::new("picker-initializing")?, Vec::new())?,
        primary_action: None,
    })
}

pub(super) use picker_screen_scope::{build_screen, selection_rows, synchronize_rows};
pub(super) use picker_surface_view::PickerSurfaceView;
