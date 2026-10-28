//! Shared dialog presentation of one captured client action and its observation.
use super::{
    SessionsPickerOutcome,
    client_action_state::{ClientResultAction, PickerClientAction, interrupt_client_action},
    picker_model::SessionsPickerModel,
};
use crate::presentation::session_picker::picker_rendering::iocraft_cell_count;
use crate::sessions::{
    source_client_action::{CapturedClientAction, SessionClientEffect},
    source_ssh_protocol::{
        SourceNativeObservation, SourceProviderActionResult, SourceProviderOperationRead,
        SourceReply,
    },
};
use codex_router_tui::{DialogAction, DialogFrame, DialogHost, DialogKeyboardOwner, DialogMargins};
use iocraft::prelude::*;

pub(super) fn finish_or_detach(
    mut model: State<SessionsPickerModel>,
    mut outcome: State<Option<SessionsPickerOutcome>>,
) {
    if let Some(interrupted) = interrupt_client_action(&model.read()) {
        outcome.set(Some(interrupted));
    } else {
        model.write().client_action = PickerClientAction::Idle;
    }
}

pub(super) fn check_operation(mut model: State<SessionsPickerModel>) {
    let request = match &model.read().client_action {
        PickerClientAction::Finished { request, .. } => request.clone(),
        _ => return,
    };
    let (service_epoch, operation_id) = match &request.effect {
        SessionClientEffect::Managed {
            binding,
            operation_id,
            ..
        } => (
            binding.generation.service_epoch.clone(),
            operation_id.clone(),
        ),
        SessionClientEffect::ReadManaged {
            service_epoch,
            operation_id,
        } => (service_epoch.clone(), operation_id.clone()),
        SessionClientEffect::Native { .. } => return,
    };
    let mut model = model.write();
    let Some(attempt) = model.next_client_generation.checked_add(1) else {
        return;
    };
    model.next_client_generation = attempt;
    model.client_action = PickerClientAction::Queued(CapturedClientAction {
        attempt,
        source: request.source,
        service_id: request.service_id,
        effect: SessionClientEffect::ReadManaged {
            service_epoch,
            operation_id,
        },
    });
}

pub(super) fn handle_client_key(
    mut model: State<SessionsPickerModel>,
    outcome: State<Option<SessionsPickerOutcome>>,
    code: KeyCode,
    modifiers: KeyModifiers,
) {
    if modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER) {
        return;
    }
    match code {
        KeyCode::Tab | KeyCode::BackTab | KeyCode::Left | KeyCode::Right
            if !modifiers.contains(KeyModifiers::SHIFT)
                || matches!(code, KeyCode::Tab | KeyCode::BackTab) =>
        {
            if let PickerClientAction::Finished { request, focus, .. } =
                &mut model.write().client_action
                && !matches!(request.effect, SessionClientEffect::Native { .. })
            {
                *focus = match focus {
                    ClientResultAction::Close => ClientResultAction::CheckOperation,
                    ClientResultAction::CheckOperation => ClientResultAction::Close,
                };
            }
        }
        KeyCode::Esc => finish_or_detach(model, outcome),
        KeyCode::Enter => {
            let action = match &model.read().client_action {
                PickerClientAction::Finished { focus, .. } => Some(*focus),
                _ => None,
            };
            match action {
                Some(ClientResultAction::Close) => finish_or_detach(model, outcome),
                Some(ClientResultAction::CheckOperation) => check_operation(model),
                None => {}
            }
        }
        _ => {}
    }
}

fn observation_lines(
    request: &CapturedClientAction,
    result: &Result<SourceReply, crate::sessions::source_ssh_protocol::SourceReadError>,
) -> Vec<String> {
    use collaboration_client::ConversationOperationResult;
    use collaboration_client::protocol::ConversationCreateOutcome;
    let mut lines = Vec::new();
    match result {
        Err(crate::sessions::source_ssh_protocol::SourceReadError::Rejected(crate::sessions::source_ssh_protocol::SourceRejection::InvalidRequest))=>lines.push("Source rejected this request. Update agent-sessions on both machines before trying this action.".to_owned()),
        Err(error)=>lines.push(error.to_string()),
        Ok(SourceReply::NativeClaude {result})=>{
            match &result.observation {
                SourceNativeObservation::Observed {status,stdout,stderr}=>{
                    lines.push(format!("Native exit: {status:?}"));
                    if !stdout.is_empty() {lines.push(String::from_utf8_lossy(stdout).into_owned());}
                    if !stderr.is_empty() {lines.push(String::from_utf8_lossy(stderr).into_owned());}
                }
                SourceNativeObservation::Unknown {status,stdout,stderr,..}=>{
                    lines.push(request.unknown_description());
                    if let Some(status)=status {lines.push(format!("Observed exit: {status:?}"));}
                    if !stdout.is_empty() {lines.push(String::from_utf8_lossy(stdout).into_owned());}
                    if !stderr.is_empty() {lines.push(String::from_utf8_lossy(stderr).into_owned());}
                }
            }
        }
        Ok(SourceReply::ProviderAction {result})=>match result.as_ref() {
            SourceProviderActionResult::ManagedCreate {result}=>match result.as_ref() {
                ConversationCreateOutcome::Created {target,..}=>lines.push(format!("Created session {}",String::from(target.session_id.clone()))),
                ConversationCreateOutcome::CreatedWithoutSettings {target,..}=>lines.push(format!("Created session {}; some settings were not applied",String::from(target.session_id.clone()))),
                ConversationCreateOutcome::Pending {..}=>lines.push("Creation is pending. Check the retained operation; do not submit again.".to_owned()),
            },
            SourceProviderActionResult::ManagedLoad {result}=>lines.push(match result.as_ref() {
                ConversationOperationResult::Completed {target,..}=>format!("Loaded session {}",String::from(target.session_id.clone())),
                ConversationOperationResult::Pending {..}=>"Load is pending. Check the retained operation.".to_owned(),
                ConversationOperationResult::Running {..}=>"Load outcome is uncertain. Check the retained operation.".to_owned(),
            }),
            SourceProviderActionResult::ManagedResume {result}=>lines.push(format!("Resume stage: {:?}; effect: {:?}",result.operation.stage,result.operation.effect)),
            SourceProviderActionResult::Failed {failure,..}=>lines.push(format!("Action failed: {}",String::from(failure.message.clone()))),
            SourceProviderActionResult::Unknown {..}=>lines.push(request.unknown_description()),
        },
        Ok(SourceReply::ProviderOperation {result,..})=>match result.as_ref() {
            SourceProviderOperationRead::Found {operation}=>lines.push(format!("Operation stage: {:?}; effect: {:?}",operation.stage,operation.effect)),
            SourceProviderOperationRead::Failed {failure}=>lines.push(format!("Operation lookup: {}",String::from(failure.message.clone()))),
        },
        Ok(_)=>lines.push(request.unknown_description()),
    }
    lines
}

pub(super) fn render_client_action(
    model: &SessionsPickerModel,
    state: State<SessionsPickerModel>,
    outcome: State<Option<SessionsPickerOutcome>>,
    height: usize,
) -> Element<'static, View> {
    let (request, result) = match &model.client_action {
        PickerClientAction::Queued(request) | PickerClientAction::Running(request) => {
            (request, None)
        }
        PickerClientAction::Finished {
            request, result, ..
        } => (request, Some(result)),
        PickerClientAction::Idle => return element!(View),
    };
    let mut lines = vec![format!("Source: {}", request.source_label())];
    match &request.effect {
        SessionClientEffect::Native { action, .. } => {
            use crate::sessions::source_ssh_protocol::SourceNativeClaudeAction;
            lines.push("Client: Claude Code · background".to_owned());
            match action {
                SourceNativeClaudeAction::New { directory, .. } => {
                    lines.push(format!("New · {}", directory.as_str()))
                }
                SourceNativeClaudeAction::Resume {
                    target, directory, ..
                }
                | SourceNativeClaudeAction::Fork {
                    target, directory, ..
                } => lines.push(format!(
                    "Session {} · {}",
                    String::from(target.session_id.clone()),
                    directory.as_str()
                )),
            }
        }
        SessionClientEffect::Managed {
            binding,
            operation_id,
            ..
        } => {
            lines.push(format!("Client: {:?}", binding.runtime.provider));
            lines.push(format!("Operation: {}", operation_id.as_str()));
        }
        SessionClientEffect::ReadManaged { operation_id, .. } => {
            lines.push(format!("Operation: {}", operation_id.as_str()))
        }
    }
    if let Some(result) = result {
        lines.extend(observation_lines(request, result));
    } else {
        lines.push(
            "Waiting for the selected source result. The action will not be resubmitted."
                .to_owned(),
        );
    }
    let focus = match &model.client_action {
        PickerClientAction::Finished { focus, .. } => *focus,
        _ => ClientResultAction::Close,
    };
    let mut actions=vec![element!(DialogAction(label:if result.is_some() {"Close"}else {"Stop waiting"},has_focus:focus==ClientResultAction::Close,keyboard_owner:DialogKeyboardOwner::Caller,handler:move |_|finish_or_detach(state,outcome))).into_any()];
    if result.is_some() && !matches!(request.effect, SessionClientEffect::Native { .. }) {
        actions.push(element!(DialogAction(label:"Check operation",has_focus:focus==ClientResultAction::CheckOperation,keyboard_owner:DialogKeyboardOwner::Caller,handler:move |_|check_operation(state))).into_any());
    }
    let background=element!(DialogFrame(width:u16::try_from(model.width).unwrap_or(u16::MAX),height:u16::try_from(height).unwrap_or(u16::MAX),title:"Client action".to_owned(),keyboard_owner:DialogKeyboardOwner::Caller,scroll_keys:codex_router_tui::DialogScrollKeys::Navigation,margins:DialogMargins {top:1,right:2,bottom:1,left:2},help_action:Some(super::picker_help_view::help_action(state)),actions) {
        #(lines.into_iter().map(|line|element!(Text(content:line))))
    }).into_any();
    let dialog = model
        .show_help
        .then(|| super::picker_help_view::render_help(model, state, height));
    element!(View(width:iocraft_cell_count(model.width),height:iocraft_cell_count(height)) {
        DialogHost(width:u16::try_from(model.width).unwrap_or(u16::MAX),height:u16::try_from(height).unwrap_or(u16::MAX),background:vec![background],dialog)
    })
}
