//! Shared dialog presentation of one captured client action and its observation.
use crate::session_picker::SessionsPickerOutcome;
use crate::session_picker::client::client_action_state::PickerClientAction;
use crate::session_picker::client::client_action_state::interrupt_client_action;
use crate::session_picker::screen::picker_model::SessionsPickerModel;
use agent_session_inventory::sessions::source_client_action::CapturedClientAction;
use agent_session_inventory::sessions::source_client_action::SessionClientEffect;
use agent_session_inventory::sessions::source_ssh_protocol::SourceNativeObservation;
use agent_session_inventory::sessions::source_ssh_protocol::SourceProviderActionResult;
use agent_session_inventory::sessions::source_ssh_protocol::SourceProviderOperationRead;
use agent_session_inventory::sessions::source_ssh_protocol::SourceReply;
use iocraft::prelude::*;

pub(crate) fn finish_or_detach(
    mut model: State<SessionsPickerModel>,
    mut outcome: State<Option<SessionsPickerOutcome>>,
) {
    if let Some(interrupted) = interrupt_client_action(&model.read()) {
        outcome.set(Some(interrupted));
    } else {
        model.write().client_action = PickerClientAction::Idle;
    }
}

pub(crate) fn check_operation(mut model: State<SessionsPickerModel>) {
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

fn observation_lines(
    request: &CapturedClientAction,
    result: &Result<
        SourceReply,
        agent_session_inventory::sessions::source_ssh_protocol::SourceReadError,
    >,
) -> Vec<String> {
    use collaboration_client::ConversationOperationResult;
    use collaboration_client::protocol::ConversationCreateOutcome;
    let mut lines = Vec::new();
    match result {
        Err(agent_session_inventory::sessions::source_ssh_protocol::SourceReadError::UnguardedNativeResume)=>lines.push(format!("{} runs an older agent-collaboration that could start a copy of this session. Update it (brew upgrade codex-router), then open again.", request.source_label())),
        Err(agent_session_inventory::sessions::source_ssh_protocol::SourceReadError::Rejected(agent_session_inventory::sessions::source_ssh_protocol::SourceRejection::InvalidRequest))=>lines.push("Source rejected this request. Update agent-sessions on both machines before trying this action.".to_owned()),
        Err(error) if matches!(request.effect,SessionClientEffect::Native {..})=>lines.push(format!("Could not complete this Claude action: {error}. Refresh the list, or use its existing terminal if it is already running.")),
        Err(error)=>lines.push(error.to_string()),
        Ok(SourceReply::NativeClaude {result})=>{
            match &result.observation {
                SourceNativeObservation::Observed {status:agent_session_inventory::sessions::source_ssh_protocol::SourceNativeExitStatus::Exited {code:0},stdout,..}=>{
                    match agent_session_inventory::sessions::claude_terminal_launch::parse_background_receipt(stdout) {
                        Ok(handle)=>lines.push(format!("Started in background: {}. Open it from the list to attach.",String::from(handle))),
                        Err(error)=>lines.push(error.to_string()),
                    }
                }
                SourceNativeObservation::Observed {status,stderr,..}=>{
                    use agent_session_inventory::sessions::source_ssh_protocol::SourceNativeExitStatus;
                    lines.push(match status {
                        SourceNativeExitStatus::Exited {code}=>format!("Claude exited with code {code}. Check claude agents before trying again."),
                        SourceNativeExitStatus::Terminated {..}=>"Claude stopped by a signal. Check claude agents before trying again.".to_owned(),
                    });
                    let excerpt=String::from_utf8_lossy(stderr).split_whitespace().collect::<Vec<_>>().join(" ");
                    if !excerpt.is_empty() {lines.push(format!("Claude: {}",excerpt.chars().take(240).collect::<String>()));}
                }
                SourceNativeObservation::Unknown {..}=>{
                    lines.push(request.unknown_description());
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

pub(crate) fn render_client_body(model: &SessionsPickerModel) -> AnyElement<'static> {
    let request = match &model.client_action {
        PickerClientAction::Terminal(request, _)
        | PickerClientAction::Queued(request)
        | PickerClientAction::Running(request)
        | PickerClientAction::Finished { request, .. } => request,
        PickerClientAction::Idle => return element!(View).into_any(),
    };
    element!(agent_collaboration_tui::UiText(content:format!("Source: {}",request.source_label()),role:agent_collaboration_tui::UiTextRole::Body,wrap:TextWrap::Wrap)).into_any()
}

pub(crate) fn render_client_status(model: &SessionsPickerModel) -> Option<AnyElement<'static>> {
    let content = match &model.client_action {
        PickerClientAction::Idle => return None,
        PickerClientAction::Terminal(..)
        | PickerClientAction::Queued(_)
        | PickerClientAction::Running(_) => {
            "Waiting for the selected source result. The action will not be resubmitted.".to_owned()
        }
        // Supplemental diagnostics belong to the same current result paragraph.
        PickerClientAction::Finished { request, result } => {
            observation_lines(request, result).join(" ")
        }
    };
    Some(element!(agent_collaboration_tui::UiText(content,role:agent_collaboration_tui::UiTextRole::Body,wrap:TextWrap::Wrap)).into_any())
}

#[cfg(test)]
pub(crate) mod claude_mode_test;

#[cfg(test)]
pub(crate) mod configured_claude_resume_test;
