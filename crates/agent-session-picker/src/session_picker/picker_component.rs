use crate::session_picker::picker_rendering::iocraft_cell_count;
use std::time::Duration;

use super::picker_machine_controls::{PickerMachineFilter, PickerMachineStage};
use iocraft::prelude::*;

use crate::session_picker::interactive_row::InteractiveSessionChoiceRow;
use crate::session_picker::picker_actions::SessionsPickerKey;
use crate::session_picker::picker_actions::SessionsPickerOutcome;
use crate::session_picker::picker_model::SessionsPickerModel;
use crate::session_picker::picker_rendering::MIN_PICKER_WIDTH;
use crate::session_picker::picker_request::SessionsPickerRecordLoader;
use crate::session_picker::picker_request::SessionsPickerRequest;
use agent_session_inventory::picker_data_query::SessionsPickerRoot;
use agent_session_inventory::sessions::SessionConversationPreview;
#[cfg(test)]
use agent_session_inventory::sessions::SessionConversationSource;
use agent_session_inventory::sessions::SessionPickerRecord;
use agent_session_inventory::sessions::SessionsSort;

#[path = "picker_frame_view.rs"]
mod picker_frame_view;
use picker_frame_view::render_picker_view;

#[path = "details_dialog_view.rs"]
mod details_dialog_view;

#[path = "picker_layout_budget.rs"]
mod picker_layout_budget;
use picker_layout_budget::{picker_body_budget, session_visible_row_budget};

#[path = "picker_list_view.rs"]
mod picker_list_view;
use picker_list_view::{no_matching_sessions_label, render_session_list, start_new_args_label};

#[path = "picker_detail_view.rs"]
mod picker_detail_view;
use picker_detail_view::{render_details, render_start_new_details};

#[path = "picker_display_text.rs"]
mod picker_display_text;
use picker_display_text::{compact_age, root_label, runtime_view_label, sort_label, truncate_end};

pub(super) use picker_display_text::fit_line;

const MIN_RENDER_HEIGHT: usize = 24;
const SIDECAR_PICKER_WIDTH: usize = 140;
const START_NEW_DETAILS_HEIGHT: usize = 6;

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
    let (terminal_width, terminal_height) = hooks.use_terminal_size();
    let live_terminal_width = props.width == 0;
    let live_terminal_height = props.height == 0 && live_terminal_width;
    let observed_width = hooks.use_state(|| {
        if live_terminal_width {
            let width = usize::from(terminal_width);
            if width == 0 { MIN_PICKER_WIDTH } else { width }
        } else {
            props.width
        }
    });
    let observed_height = hooks.use_state(|| {
        if live_terminal_height {
            let height = usize::from(terminal_height);
            if height == 0 {
                MIN_RENDER_HEIGHT
            } else {
                height
            }
        } else if props.height == 0 {
            MIN_RENDER_HEIGHT
        } else {
            props.height.max(1)
        }
    });
    let width = observed_width.get();
    let height = observed_height.get();
    let mut model = hooks.use_state(|| SessionsPickerModel::new(props.request.clone(), width));
    if model.read().width != width {
        model.write().set_width(width);
    }
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
        model,
        props.client_executor.clone(),
    );
    super::source_action_worker::use_source_preparation(
        &mut hooks,
        model,
        selected_outcome,
        props.source_preparer.clone(),
    );
    picker_input::use_picker_input(
        &mut hooks,
        picker_input::PickerInputContext {
            model,
            session_clipboard_writer: session_clipboard_writer.clone(),
            observed_width,
            observed_height,
            reload_generation,
            can_load_source: props.record_loader.is_some(),
            selected_outcome,
            should_cancel,
            conversation_cache,
            reload_port: reload_port.clone(),
            live_terminal_width,
            live_terminal_height,
            width,
        },
    );

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
        };
    }

    if model.read().client_action.is_active() {
        return super::client_action_view::render_client_action(
            &model.read(),
            model,
            selected_outcome,
            height,
        );
    }

    let machine_context = picker_machine_input::MachineSelectionContext {
        model,
        can_load_source: props.record_loader.is_some(),
        reload_port: reload_port.clone(),
        reload_generation,
        conversation_cache,
    };
    if let Some(background) = super::picker_help_view::render_choice_surface(
        &model.read(),
        model,
        height,
        Handler::from({
            let context = machine_context.clone();
            move |key| picker_machine_input::select_machine_choice(context.clone(), key)
        }),
        Handler::from(move |_| {
            picker_machine_input::cancel_machine_choice(machine_context.clone())
        }),
        selected_outcome,
    ) {
        return background;
    }

    let selected_conversation = {
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

    let minimum_render_height = if live_terminal_height {
        1
    } else {
        MIN_RENDER_HEIGHT.min(height)
    };
    render_picker_view(
        &model.read(),
        model,
        selected_outcome,
        selected_conversation.as_ref(),
        picker_frame_view::PickerViewportBudget {
            height,
            minimum_render_height,
        },
        session_clipboard_writer,
        Handler::from(move |_| {
            let mut model = model;
            let mut cache = conversation_cache;
            let mut generation = reload_generation;
            let previous = model.read().data_query();
            model.write().apply_provider_filters();
            if model.read().data_query() != previous {
                cache.write().invalidate();
                let next = generation.get().saturating_add(1);
                generation.set(next);
                reload_port.send(model.write().prepare_source_reload(next));
            }
        }),
    )
}

#[path = "picker_entrypoint.rs"]
mod picker_entrypoint;
pub use picker_entrypoint::run_sessions_picker;
#[cfg(feature = "quota-reset-test-harness")]
pub use picker_entrypoint::run_sessions_picker_test_harness;

#[cfg(test)]
#[path = "picker_component_tests.rs"]
mod tests;

#[path = "picker_machine_input.rs"]
mod picker_machine_input;

#[cfg(test)]
#[path = "form_alignment_tests.rs"]
mod form_alignment_tests;
#[path = "picker_input.rs"]
mod picker_input;
