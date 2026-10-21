//! One event owner routes active choices/dialogs before browse commands.
use super::*;
pub(super) struct PickerInputContext {
    pub model: State<SessionsPickerModel>,
    pub session_clipboard_writer:
        crate::presentation::session_picker::session_id_clipboard::SessionClipboardWriter,
    pub observed_width: State<usize>,
    pub observed_height: State<usize>,
    pub reload_generation: State<u64>,
    pub can_load_source: bool,
    pub selected_outcome: State<Option<SessionsPickerOutcome>>,
    pub should_cancel: State<bool>,
    pub conversation_cache: State<ConversationPreviewCache>,
    pub reload_port: SessionRecordsReloadPort,
    pub live_terminal_width: bool,
    pub live_terminal_height: bool,
    pub width: usize,
}

pub(super) fn use_picker_input(hooks: &mut Hooks<'_, '_>, context: PickerInputContext) {
    let PickerInputContext {
        mut model,
        session_clipboard_writer,
        mut observed_width,
        mut observed_height,
        mut reload_generation,
        can_load_source,
        mut selected_outcome,
        mut should_cancel,
        mut conversation_cache,
        reload_port,
        live_terminal_width,
        live_terminal_height,
        width,
    } = context;
    hooks.use_terminal_events(move |event| {
        if let TerminalEvent::Resize(width, height) = event {
            if live_terminal_width {
                observed_width.set(usize::from(width));
            }
            if live_terminal_height {
                observed_height.set(usize::from(height).max(1));
            }
            return;
        }
        if let TerminalEvent::BracketedPaste(text) = event {
            if width < MIN_PICKER_WIDTH || should_cancel.get() || selected_outcome.read().is_some()
            {
                return;
            }
            let mut model_value = model.write();
            if let Some(confirmation) = model_value
                .fork_confirmation
                .as_mut()
                .filter(|confirmation| confirmation.search.is_open())
            {
                let options = confirmation.choice_options();
                confirmation.search.append_query(&text, &options);
                return;
            }
            if matches!(
                model_value.machine_controls.stage,
                PickerMachineStage::Choosing { .. }
            ) && !model_value.show_help
            {
                let options = model_value.machine_controls.choice_options(
                    &model_value.request.router_registry,
                    model_value.request.machine_mode,
                );
                model_value
                    .machine_controls
                    .search
                    .append_query(&text, &options);
                return;
            }
            if model_value
                .new_session
                .as_ref()
                .is_some_and(|draft| draft.choice.is_open())
                && !model_value.show_help
            {
                let choices = super::super::new_session_form::active_new_choices(&model_value);
                if let Some(draft) = model_value.new_session.as_mut() {
                    draft.choice.append_query(&text, &choices);
                }
                return;
            }
            if model_value.provider_filter_draft.is_some()
                && model_value.provider_choice.is_open()
                && !model_value.show_help
            {
                let field = model_value.provider_filter_draft.as_ref().map_or(
                    super::super::provider_filter_model::ProviderFilterFocus::Provider,
                    |draft| draft.focus,
                );
                let choices =
                    super::super::provider_filter_model::filter_choices(&model_value, field);
                model_value.provider_choice.append_query(&text, &choices);
                return;
            }
            if model_value.source_action.pending.is_some()
                || model_value.client_action.is_active()
                || model_value.show_help
                || model_value.fork_confirmation.is_some()
                || model_value.provider_filter_draft.is_some()
                || model_value.details_dialog.is_some()
                || model_value.new_session.is_some()
                || !matches!(
                    model_value.machine_controls.stage,
                    PickerMachineStage::Browsing
                )
            {
                return;
            }
            let previous_query = model_value.data_query();
            model_value.append_search_text(&text);
            let changed = model_value.data_query() != previous_query;
            drop(model_value);
            if changed {
                conversation_cache.write().invalidate();
                let generation = reload_generation.get().saturating_add(1);
                reload_generation.set(generation);
                reload_port.send(model.write().prepare_source_reload(generation));
            }
            return;
        }
        let TerminalEvent::Key(KeyEvent {
            code,
            kind,
            modifiers,
            ..
        }) = event
        else {
            return;
        };
        if kind == KeyEventKind::Release {
            return;
        }
        if width < MIN_PICKER_WIDTH {
            return;
        }

        let mut model_value = model.write();
        if modifiers.contains(KeyModifiers::CONTROL) && matches!(code, KeyCode::Char('c' | 'd'))
            || matches!(code, KeyCode::Char('\u{3}' | '\u{4}'))
        {
            if let Some(interrupted) =
                super::super::client_action_state::interrupt_client_action(&model_value)
            {
                selected_outcome.set(Some(interrupted));
                return;
            }
            model_value.cancel_source_action();
            should_cancel.set(true);
            selected_outcome.set(None);
            return;
        }
        if should_cancel.get() || selected_outcome.read().is_some() {
            return;
        }
        if model_value.source_action.pending.is_some() {
            if code == KeyCode::Esc {
                model_value.cancel_source_action();
            }
            return;
        }
        let help_key = matches!(code, KeyCode::F(1) | KeyCode::Char('\u{1f}'))
            || matches!(code, KeyCode::Char('/' | '_' | '7'))
                && modifiers.contains(KeyModifiers::CONTROL);
        if help_key {
            model_value.handle_key(SessionsPickerKey::ToggleHelp);
            return;
        }
        if model_value.show_help {
            if code == KeyCode::Esc {
                model_value.show_help = false;
            }
            return;
        }
        if model_value.client_action.is_active() {
            drop(model_value);
            super::super::client_action_view::handle_client_key(
                model,
                selected_outcome,
                code,
                modifiers,
            );
            return;
        }
        if model_value.new_session.is_some() {
            if let Some(outcome) =
                super::super::new_session_form::handle_new_key(&mut model_value, code, modifiers)
            {
                selected_outcome.set(Some(outcome));
            }
            return;
        }
        if let Some(confirmation) = model_value.fork_confirmation.as_mut() {
            if confirmation.search.is_open() {
                let choices = confirmation.choice_options();
                if let codex_router_tui::ChoiceTransition::Selected(key) =
                    confirmation.search.handle_key(code, &choices)
                {
                    confirmation.select_destination(key);
                }
                return;
            }
            match code {
                KeyCode::Tab => {
                    if let Some(destination) = confirmation
                        .destinations
                        .get(confirmation.focused_destination)
                    {
                        confirmation.search.open(destination.key.clone());
                    }
                }
                KeyCode::Up => confirmation.move_destination(-1),
                KeyCode::Down => confirmation.move_destination(1),
                KeyCode::Esc => model_value.fork_confirmation = None,
                KeyCode::Enter
                    if !modifiers.intersects(
                        KeyModifiers::ALT | KeyModifiers::CONTROL | KeyModifiers::SUPER,
                    ) =>
                {
                    drop(model_value);
                    super::super::picker_fork_view::confirm_fork(model, selected_outcome);
                }
                _ => {}
            }
            return;
        }
        if matches!(
            model_value.machine_controls.stage,
            PickerMachineStage::Loading { .. }
        ) {
            if matches!(code, KeyCode::Esc) {
                drop(model_value);
                picker_machine_input::cancel_machine_choice(
                    picker_machine_input::MachineSelectionContext {
                        model,
                        can_load_source,
                        reload_port: reload_port.clone(),
                        reload_generation,
                        conversation_cache,
                    },
                );
            }
            return;
        }
        if matches!(
            model_value.machine_controls.stage,
            PickerMachineStage::Choosing { .. }
        ) {
            drop(model_value);
            picker_machine_input::handle_machine_choice_key(
                picker_machine_input::MachineSelectionContext {
                    model,
                    can_load_source,
                    reload_port: reload_port.clone(),
                    reload_generation,
                    conversation_cache,
                },
                code,
            );
            return;
        }
        let filter_query_before = model_value.data_query();
        if super::super::provider_filter_input::handle_provider_filter_key(
            &mut model_value,
            code,
            modifiers,
        ) {
            if model_value.data_query() != filter_query_before {
                conversation_cache.write().invalidate();
                let generation = reload_generation.get().saturating_add(1);
                reload_generation.set(generation);
                reload_port.send(model_value.prepare_source_reload(generation));
            }
            return;
        }
        if model_value.details_dialog.is_some() {
            if let Some(outcome) = super::super::details_dialog_actions::handle_details_key(
                &mut model_value,
                code,
                modifiers,
                &session_clipboard_writer,
            ) {
                selected_outcome.set(Some(outcome));
            }
            return;
        }
        if code == KeyCode::F(4) {
            model_value.open_details_dialog();
            return;
        }
        if matches!(code, KeyCode::F(2))
            || code == KeyCode::Char('g') && modifiers.contains(KeyModifiers::CONTROL)
        {
            let registry = model_value.request.router_registry.clone();
            model_value.machine_controls.open_browse(&registry);
            return;
        }
        if matches!(code, KeyCode::Tab | KeyCode::BackTab) {
            model_value.machine_controls.control_focused =
                !model_value.machine_controls.control_focused;
            return;
        }
        if model_value.machine_controls.control_focused {
            match code {
                KeyCode::Enter => {
                    if !modifiers
                        .intersects(KeyModifiers::ALT | KeyModifiers::CONTROL | KeyModifiers::SUPER)
                    {
                        let registry = model_value.request.router_registry.clone();
                        model_value.machine_controls.open_browse(&registry);
                    }
                    return;
                }
                KeyCode::Up | KeyCode::Down | KeyCode::Esc => {
                    model_value.machine_controls.control_focused = false;
                    return;
                }
                _ => {}
            }
        }
        let previous_query = model_value.data_query();
        match code {
            KeyCode::Down => model_value.handle_key(SessionsPickerKey::MoveDown),
            KeyCode::Up => model_value.handle_key(SessionsPickerKey::MoveUp),
            KeyCode::PageDown => model_value.handle_key(SessionsPickerKey::PageDown),
            KeyCode::PageUp => model_value.handle_key(SessionsPickerKey::PageUp),
            KeyCode::Home => model_value.handle_key(SessionsPickerKey::MoveFirst),
            KeyCode::End => model_value.handle_key(SessionsPickerKey::MoveLast),
            KeyCode::Char('n') if modifiers.contains(KeyModifiers::CONTROL) => {
                selected_outcome.set(model_value.start_new_action());
            }
            KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
                model_value.handle_key(SessionsPickerKey::CycleRoot);
            }
            KeyCode::Char('t') if modifiers.contains(KeyModifiers::CONTROL) => {
                model_value.handle_key(SessionsPickerKey::CycleRuntimeView);
            }
            KeyCode::Char('o') if modifiers.contains(KeyModifiers::CONTROL) => {
                model_value.handle_key(SessionsPickerKey::CycleSort);
            }
            KeyCode::Char('r') if modifiers.contains(KeyModifiers::CONTROL) => {
                conversation_cache.write().invalidate();
                let generation = reload_generation.get().saturating_add(1);
                reload_generation.set(generation);
                reload_port.send(model_value.prepare_source_reload(generation));
            }
            KeyCode::Char('c' | 'd') if modifiers.contains(KeyModifiers::CONTROL) => {
                should_cancel.set(true);
            }
            KeyCode::Char('\u{3}' | '\u{4}') => {
                should_cancel.set(true);
            }
            KeyCode::Backspace => model_value.handle_key(SessionsPickerKey::SearchBackspace),
            KeyCode::Char(character)
                if !modifiers.intersects(
                    KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                ) =>
            {
                model_value.handle_key(SessionsPickerKey::SearchChar(character));
            }
            KeyCode::Enter
                if modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT)
                    && !modifiers.intersects(KeyModifiers::ALT | KeyModifiers::SUPER) =>
            {
                model_value.open_fork_confirmation();
            }
            KeyCode::Enter
                if modifiers.contains(KeyModifiers::CONTROL)
                    && !modifiers.intersects(KeyModifiers::ALT | KeyModifiers::SUPER) =>
            {
                if model_value.begin_source_resume() {
                    return;
                }
                let outcome = if model_value.focused_identity().is_none() {
                    model_value.start_new_action()
                } else {
                    model_value.activation_outcome_for_focus()
                };
                selected_outcome.set(outcome);
            }
            KeyCode::Enter if modifiers.is_empty() => model_value.open_details_dialog(),
            KeyCode::Esc => {
                if model_value.search.is_empty()
                    && model_value.machine_controls.filter != PickerMachineFilter::Default
                {
                    let mode = model_value.request.machine_mode;
                    model_value
                        .machine_controls
                        .return_to_default(mode, can_load_source);
                    conversation_cache.write().invalidate();
                    let generation = reload_generation.get().saturating_add(1);
                    reload_generation.set(generation);
                    reload_port.send(model_value.prepare_source_reload(generation));
                } else if model_value.search.is_empty() {
                    should_cancel.set(true);
                } else {
                    model_value.handle_key(SessionsPickerKey::ClearSearch);
                }
            }
            _ => {}
        }
        let next_query = model_value.data_query();
        drop(model_value);

        if next_query != previous_query {
            conversation_cache.write().invalidate();
            let generation = reload_generation.get().saturating_add(1);
            reload_generation.set(generation);
            reload_port.send(model.write().prepare_source_reload(generation));
        }
    });
}
