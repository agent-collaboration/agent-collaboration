use crate::session_picker::picker_rendering::iocraft_cell_count;
use std::time::Duration;

use super::picker_machine_controls::{PickerMachineFilter, PickerMachineStage};
use iocraft::prelude::*;

use crate::session_picker::picker_actions::SessionsPickerKey;
use crate::session_picker::picker_actions::SessionsPickerOutcome;
use crate::session_picker::picker_model::SessionsPickerModel;
use crate::session_picker::picker_rendering::MIN_PICKER_WIDTH;
use crate::session_picker::picker_request::SessionsPickerRecordLoader;
use crate::session_picker::picker_request::SessionsPickerRequest;
use agent_session_inventory::sessions::SessionConversationPreview;
#[cfg(test)]
use agent_session_inventory::sessions::SessionConversationSource;
use agent_session_inventory::sessions::SessionPickerRecord;

#[path = "picker_layout_budget.rs"]
#[cfg(test)]
mod picker_layout_budget;
#[cfg(test)]
use picker_layout_budget::session_visible_row_budget;
#[path = "picker_list_view.rs"]
mod picker_list_view;

#[path = "picker_detail_view.rs"]
mod picker_detail_view;
use picker_detail_view::render_details;

#[path = "picker_display_text.rs"]
mod picker_display_text;
use picker_display_text::compact_age;

pub(super) use picker_display_text::fit_line;

const MIN_RENDER_HEIGHT: usize = 24;

#[path = "conversation_preview_state.rs"]
mod conversation_preview_state;
use conversation_preview_state::{ConversationPreviewCache, ConversationPreviewLoadRequest};
#[cfg(test)]
use conversation_preview_state::{ConversationPreviewKey, SelectedConversationPreview};

#[derive(Default, Props)]
pub struct SessionsPickerComponentProps<'a> {
    request: SessionsPickerRequest,
    record_loader: Option<SessionsPickerRecordLoader>,
    source_preparer: Option<super::source_action_worker::SourceActionPreparer>,
    client_inventory_loader: Option<super::NewClientInventoryLoader>,
    client_executor:
        Option<agent_session_inventory::sessions::source_client_action::SessionClientExecutor>,
    details_client_inspector: Option<super::details_client_inventory::DetailsClientInspector>,
    session_clipboard_writer: Option<super::session_id_clipboard::SessionClipboardWriter>,
    width: usize,
    height: usize,
    selected_outcome_out: Option<&'a mut Option<SessionsPickerOutcome>>,
    #[cfg(all(feature = "quota-reset-test-harness", debug_assertions))]
    dispatch_receipt:
        Option<agent_collaboration_tui::input_dispatch_receipt::InputDispatchReceiptSink>,
    #[cfg(test)]
    surface_observer: Handler<(
        agent_collaboration_tui::surface_handle::SurfaceStackHandle,
        State<SessionsPickerModel>,
    )>,
    #[cfg(test)]
    input_observer: Handler<()>,
}

use super::source_reload_port::SessionRecordsReloadPort;
use super::source_reload_worker::run_session_record_reload_worker;

#[component]
pub fn SessionsPickerComponent<'a>(
    props: &mut SessionsPickerComponentProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let session_clipboard_writer = props
        .session_clipboard_writer
        .clone()
        .unwrap_or_else(super::session_id_clipboard::unavailable_session_clipboard_writer);
    let mut system = hooks.use_context_mut::<SystemContext>();
    let width = if props.width == 0 { 76 } else { props.width };
    let height = if props.height == 0 {
        MIN_RENDER_HEIGHT
    } else {
        props.height
    };
    let mut model = hooks.use_state(|| {
        let mut model = SessionsPickerModel::new(props.request.clone(), width);
        if let Some(identity) = model
            .visible_record_at(0)
            .map(|record| record.identity.clone())
        {
            model.focus_visible_identity_in_window(&identity, None);
        }
        model
    });
    let mut conversation_cache = hooks.use_state(ConversationPreviewCache::default);
    let reload_generation = hooks.use_state(|| 0_u64);
    let reload_port = hooks.use_memo(
        || SessionRecordsReloadPort::new(model.read().data_query(), model.read().source_contexts()),
        (),
    );
    hooks.use_future({
        let receiver = reload_port.take_receiver();
        let record_loader = props.record_loader.clone();
        let mut model = model;
        let reload_port = reload_port.clone();
        let mut reload_generation = reload_generation;
        let mut conversation_cache = conversation_cache;
        async move {
            let (Some(receiver), Some(loader)) = (receiver, record_loader) else {
                return;
            };
            run_session_record_reload_worker(receiver, loader, move |request, update| {
                if reload_generation.get() != request.generation {
                    return;
                }
                let acceptance = model.write().accept_source_reload(&request, update);
                if matches!(
                    acceptance,
                    super::picker_source_view::SourceReloadAcceptance::RestoredIndependentView
                        | super::picker_source_view::SourceReloadAcceptance::RestoredRetiredView
                ) {
                    conversation_cache.write().invalidate();
                    let generation = reload_generation.get().saturating_add(1);
                    reload_generation.set(generation);
                    if acceptance
                        == super::picker_source_view::SourceReloadAcceptance::RestoredIndependentView
                    {
                        reload_port.send(model.write().prepare_source_reload(generation));
                    }
                }
            })
            .await;
        }
    });
    hooks.use_future({
        let reload_port = reload_port.clone();
        async move {
            reload_port.send(model.write().prepare_source_reload(reload_generation.get()));
            let mut refresh_interval = tokio::time::interval(Duration::from_secs(3));
            refresh_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            refresh_interval.tick().await;
            loop {
                refresh_interval.tick().await;
                if model.read().source_action.pending.is_some()
                    || model.read().client_action.is_active()
                    || model.read().machine_controls.filter != PickerMachineFilter::Default
                    || model.read().fork_confirmation.is_some()
                    || !matches!(
                        model.read().machine_controls.stage,
                        PickerMachineStage::Browsing
                    )
                {
                    continue;
                }
                reload_port.send(model.write().prepare_source_reload(reload_generation.get()));
            }
        }
    });
    let load_conversation = hooks.use_async_handler({
        let mut conversation_cache = conversation_cache;
        move |request: ConversationPreviewLoadRequest| async move {
            let source = request.key.source.clone();
            let preview = match tokio::task::spawn_blocking(move || {
                SessionConversationPreview::from_rollout_source(Some(&source))
            })
            .await
            {
                Ok(preview) => preview,
                Err(_) => SessionConversationPreview::unavailable("history unavailable"),
            };
            conversation_cache.write().complete_load(&request, preview);
        }
    });
    let mut selected_outcome = hooks.use_state(|| Option::<SessionsPickerOutcome>::None);
    let should_cancel = hooks.use_state(|| false);
    super::new_session_client_inventory::use_client_inventory(
        &mut hooks,
        model,
        props.client_inventory_loader.clone(),
    );
    super::details_client_inventory::use_details_client_inventory(
        &mut hooks,
        model,
        props.details_client_inspector.clone(),
    );
    super::client_action_state::use_client_actions(
        &mut hooks,
        super::client_action_state::ClientActionContext {
            model,
            executor: props.client_executor.clone(),
            outcome: selected_outcome,
            background_completed: std::sync::Arc::new({
                let reload_port = reload_port.clone();
                move || {
                    let mut generation = reload_generation;
                    let mut model = model;
                    let next = generation.get().saturating_add(1);
                    generation.set(next);
                    reload_port.send(model.write().prepare_source_reload(next));
                }
            }),
        },
    );
    super::source_action_worker::use_source_preparation(
        &mut hooks,
        model,
        selected_outcome,
        props.source_preparer.clone(),
    );
    use agent_collaboration_tui::{
        actions::ActionId,
        controls::{ComboBoxState, ListSelectionState, TextFieldState},
        input_owner::{InputOwner, Routed},
        overlay_host::OverlayHost,
        surface_handle::use_surface_stack,
    };
    let setup = (|| -> picker_surface::SurfaceResult<_> {
        let rows = picker_surface::selection_rows(&model.read())?;
        let list = ListSelectionState::new(rows, None)?;
        let combo = ComboBoxState::new(Vec::new(), None)?;
        let empty = picker_surface::empty_model()?;
        let quit = ActionId::new("quit")?;
        let quit_aliases = vec!["Ctrl+D".parse()?];
        Ok((list, combo, empty, quit, quit_aliases))
    })();
    let (initial_list, empty_combo, empty_surface, quit_action, global_quit_aliases) = match setup {
        Ok(setup) => setup,
        Err(error) => return element!(Text(content: error.to_string())).into_any(),
    };
    let surface_context = picker_surface::PickerSurfaceContext {
        model,
        outcome: selected_outcome,
        cancelled: should_cancel,
        error: hooks.use_state(|| None),
        reload_port,
        reload_generation,
        conversation_cache,
        copier: session_clipboard_writer.clone(),
        can_load_source: props.record_loader.is_some(),
        filter: hooks.use_state(TextFieldState::default),
        list: hooks.use_state(|| initial_list),
        list_control: hooks.use_state(|| None),
        help: hooks.use_state(|| None),
        combos: std::array::from_fn(|_| hooks.use_state(|| empty_combo.clone())),
        active_dialog: hooks.use_state(|| None),
    };
    let stack = use_surface_stack(&mut hooks, |stack| {
        match picker_surface::build_screen(surface_context.clone(), stack) {
            Ok(surface) => surface,
            Err(error) => {
                surface_context.report(error);
                empty_surface
            }
        }
    });
    let viewport = stack.viewport();
    #[cfg(test)]
    (props.surface_observer)((stack, model));
    let width = if viewport.0 == 0 {
        width
    } else {
        usize::from(viewport.0)
    };
    let height = if viewport.1 == 0 {
        height
    } else {
        usize::from(viewport.1)
    };
    if model.read().width != width {
        model.write().set_width(width);
    }
    if let Some(help) = surface_context.help.read().clone() {
        surface_context.synchronize_dialog(stack, &help);
    }
    if let Err(error) = picker_surface::synchronize_rows(&surface_context, stack) {
        surface_context.report(error);
    }

    if should_cancel.get() {
        selected_outcome.set(None);
    }
    if let Some(selected_outcome) = selected_outcome.read().clone() {
        if let Some(out) = props.selected_outcome_out.as_mut() {
            **out = Some(selected_outcome);
        }
        system.exit();
    } else if *should_cancel.read() {
        if let Some(out) = props.selected_outcome_out.as_mut() {
            **out = None;
        }
        system.exit();
    }

    if width < MIN_PICKER_WIDTH {
        if let Some(out) = props.selected_outcome_out.as_mut() {
            **out = super::client_action_state::interrupt_client_action(&model.read())
                .or(Some(SessionsPickerOutcome::TerminalTooNarrow));
        }
        system.exit();
        return element! {
            View(width: iocraft_cell_count(width), flex_direction: FlexDirection::Column) {
                Text(content: "terminal too narrow\n")
            }
        }
        .into_any();
    }

    let _selected_conversation = {
        let model_value = model.read();
        model_value
            .details_dialog
            .as_ref()
            .map(|dialog| dialog.record())
            .or_else(|| model_value.focused_record())
            .map(|record| {
                let selected_preview = {
                    let cache = conversation_cache.read();
                    cache.select_record(record)
                };
                if let Some(load_request) = selected_preview.load_request.clone() {
                    conversation_cache.write().start_loading(&load_request);
                    load_conversation(load_request);
                }
                selected_preview.preview
            })
    };

    let routed_context = surface_context.clone();
    #[cfg(test)]
    let input_observer = props.input_observer.clone();
    let root = element!(InputOwner(stack, width: u16::try_from(width).unwrap_or(u16::MAX),
        height: u16::try_from(height).unwrap_or(u16::MAX), quit_action, global_quit_aliases,
        on_routed: move |routed| {
            if let Routed::OverlayClosed(id) = routed {
                let active = routed_context.active_dialog.read().clone();
                if let Some(active) = active.filter(|dialog| dialog.id == id) {
                    routed_context.close_dialog(stack, active.kind);
                }
            }
            if let Some(help) = routed_context.help.read().clone() {
                routed_context.synchronize_dialog(stack, &help);
            }
            if let Err(error) = picker_surface::synchronize_rows(&routed_context, stack) {
                routed_context.report(error);
            }
            #[cfg(test)]
            input_observer(());
        }) {
        OverlayHost {picker_surface::PickerSurfaceView(context:surface_context)}
    });
    #[cfg(all(feature = "quota-reset-test-harness", debug_assertions))]
    let root = {
        let mut root = root;
        root.props.dispatch_receipt = props.dispatch_receipt.clone();
        root
    };
    root.into_any()
}

#[path = "picker_entrypoint.rs"]
mod picker_entrypoint;
pub use picker_entrypoint::run_sessions_picker;
#[cfg(feature = "quota-reset-test-harness")]
pub use picker_entrypoint::run_sessions_picker_test_harness;

#[cfg(test)]
#[path = "picker_component_tests.rs"]
mod tests;

#[path = "picker_machine_selection.rs"]
mod picker_machine_selection;

#[cfg(test)]
#[path = "form_alignment_tests.rs"]
mod form_alignment_tests;
#[path = "picker_surface.rs"]
mod picker_surface;
