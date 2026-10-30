//! Details event effects: selection uses captured state; clipboard output stays here.
use super::details_dialog_state::{ClipboardRequestStatus, DetailsAction};
use super::session_id_clipboard::SessionClipboardWriter;
use super::{SessionsPickerOutcome, picker_model::SessionsPickerModel};
use agent_session_inventory::sessions::SessionActionSelection;
use iocraft::prelude::*;

pub(super) fn dispatch_details_action(
    mut model: State<SessionsPickerModel>,
    mut outcome: State<Option<SessionsPickerOutcome>>,
    action: DetailsAction,
    copier: &SessionClipboardWriter,
) {
    let selected = apply_details_action(&mut model.write(), action, copier);
    if let Some(selected) = selected {
        outcome.set(Some(selected));
    }
}

pub(super) fn apply_details_action(
    model: &mut SessionsPickerModel,
    action: DetailsAction,
    clipboard_writer: &super::session_id_clipboard::SessionClipboardWriter,
) -> Option<SessionsPickerOutcome> {
    if model.source_action.pending.is_some() || model.client_action.is_active() || model.show_help {
        return None;
    }
    let dialog = model.details_dialog.clone()?;
    if !dialog.actions().contains(&action) {
        return None;
    }
    if dialog.record().identity.is_provider()
        && matches!(
            action,
            DetailsAction::Load | DetailsAction::Resume | DetailsAction::ResumeBackground
        )
    {
        model.begin_details_client_action(&dialog, action);
        return None;
    }
    match action {
        DetailsAction::Load | DetailsAction::ResumeBackground => {}
        DetailsAction::Close => model.details_dialog = None,
        DetailsAction::Resume => {
            let record = dialog.record();
            let selection = SessionActionSelection::from_picker_record(record);
            if model.begin_source_selection(&selection, false, record.cwd.clone()) {
                return None;
            }
            model.details_dialog = None;
            return Some(SessionsPickerOutcome::ResumeSession(selection));
        }
        DetailsAction::Fork => {
            let record = dialog.record();
            if let super::details_client_inventory::DetailsClientReadState::Ready(
                super::details_client_inventory::DetailsClientCapabilities::Native { source },
            ) = &dialog.client_inventory
            {
                model.fork_confirmation =
                    super::picker_fork_confirmation::ForkConfirmation::capture_native(
                        record,
                        &model.request,
                        source,
                    );
                if model.fork_confirmation.is_some() {
                    model.details_dialog = None;
                }
                return None;
            }
            model.fork_confirmation = Some(
                super::picker_fork_confirmation::ForkConfirmation::capture(record, &model.request),
            );
            model.details_dialog = None;
        }
        DetailsAction::CopySessionId | DetailsAction::CopyPath => {
            let payload = match action {
                DetailsAction::CopyPath => dialog
                    .record()
                    .cwd
                    .as_deref()
                    .filter(|path| !path.trim().is_empty())?,
                _ => &dialog.record().session_id,
            };
            let copied = clipboard_writer(payload);
            if let Some(current) = &mut model.details_dialog {
                current.clipboard_status = if copied.is_ok() {
                    ClipboardRequestStatus::Sent
                } else {
                    ClipboardRequestStatus::Failed
                };
            }
        }
    }
    None
}

pub(super) fn handle_details_key(
    model: &mut SessionsPickerModel,
    code: KeyCode,
    modifiers: KeyModifiers,
    copier: &SessionClipboardWriter,
) -> Option<SessionsPickerOutcome> {
    let control_enter = code == KeyCode::Enter
        && modifiers.contains(KeyModifiers::CONTROL)
        && !modifiers.intersects(KeyModifiers::ALT | KeyModifiers::SUPER);
    let action = if control_enter && modifiers.contains(KeyModifiers::SHIFT) {
        Some(DetailsAction::Fork)
    } else if control_enter {
        Some(DetailsAction::Resume)
    } else {
        match code {
            KeyCode::Esc => Some(DetailsAction::Close),
            KeyCode::Left | KeyCode::Right if modifiers.is_empty() => {
                if let Some(dialog) = &mut model.details_dialog {
                    dialog.move_focus(code == KeyCode::Left);
                }
                None
            }
            KeyCode::Tab | KeyCode::BackTab => {
                if let Some(dialog) = &mut model.details_dialog {
                    dialog.move_focus(
                        code == KeyCode::BackTab || modifiers.contains(KeyModifiers::SHIFT),
                    );
                }
                None
            }
            KeyCode::Enter | KeyCode::Char(' ') if modifiers.is_empty() => model
                .details_dialog
                .as_ref()
                .map(|dialog| dialog.focused_action),
            _ => None,
        }
    };
    action.and_then(|action| apply_details_action(model, action, copier))
}
