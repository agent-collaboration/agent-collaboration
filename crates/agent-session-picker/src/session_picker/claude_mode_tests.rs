//! Claude choices, captured resume options and user-visible background receipts.
use super::*;
use crate::session_picker::{
    details_client_inventory::{DetailsClientCapabilities, DetailsClientReadState},
    details_dialog_state::SessionDetailsDialog,
    new_session_form::{NewFormFocus, active_new_choices},
    test_support::picker_request,
};
use agent_session_inventory::sessions::{
    SessionPickerRecord,
    source_ssh_protocol::{
        SourceNativeBinding, SourceNativeClaudeAction, SourceNativeClaudeIdentity,
        SourceNativeClaudeResult, SourceNativeExitStatus,
    },
};

fn binding() -> SourceNativeBinding {
    SourceNativeBinding {
        service_id: "00000000-0000-4000-8000-000000000001"
            .to_owned()
            .try_into()
            .unwrap(),
        service_epoch: "00000000-0000-4000-8000-000000000002"
            .to_owned()
            .try_into()
            .unwrap(),
    }
}

#[test]
fn claude_new_offers_explicit_foreground_and_background_choices() {
    let mut model = SessionsPickerModel::new(picker_request(), 120);
    model.open_new_form();
    model.new_session.as_mut().unwrap().focus = NewFormFocus::Client;
    let choices = active_new_choices(&model);
    assert!(choices.iter().any(|choice| choice.label == "Claude Code"));
    assert!(
        choices
            .iter()
            .any(|choice| choice.label == "Claude Code · background")
    );
}

#[test]
fn claude_stopped_details_offers_background_resume_without_global_options() {
    let summary = serde_json::from_value(serde_json::json!({
        "origin":"claudeCodeBackground",
        "target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},"sessionId":"00000000-0000-4000-8000-000000000042"},
        "jobHandle":"deadbeef","workingDirectory":"/source/project","startedAt":1,"state":{"kind":"stopped"}
    })).unwrap();
    let record = SessionPickerRecord::from_provider_summary(&summary, "Native Claude");
    let mut dialog = SessionDetailsDialog::capture(&record);
    dialog.client_inventory =
        DetailsClientReadState::Ready(DetailsClientCapabilities::Native { source: binding() });
    let action = dialog
        .actions()
        .into_iter()
        .find(|action| action.label() == "Resume in background")
        .expect("a stopped native session needs an explicit background action");
    let mut request = picker_request();
    request.native_arguments = ["--model", "claude-haiku-5-5", "--effort", "low"]
        .map(std::ffi::OsString::from)
        .to_vec();
    let mut model = SessionsPickerModel::new(request, 120);
    assert!(model.begin_details_client_action(&dialog, action));
    let PickerClientAction::Queued(captured) = model.client_action else {
        panic!("background resume stays in the picker");
    };
    let SessionClientEffect::Native {
        action: SourceNativeClaudeAction::Resume { arguments, .. },
        ..
    } = captured.effect
    else {
        panic!("background resume keeps its captured identity");
    };
    assert!(
        arguments.is_empty(),
        "global arguments must not make Claude start a copy"
    );
}

#[test]
fn claude_background_success_shows_one_plain_recorded_receipt() {
    let source = binding();
    let directory: agent_session_inventory::sessions::router_connection_registry::AbsoluteRemoteCwd = "/source/project".to_owned().try_into().unwrap();
    let request = CapturedClientAction {
        attempt: 1,
        source: crate::session_picker::PickerSourceContext::DefaultHosted,
        service_id: source.service_id.clone(),
        effect: SessionClientEffect::Native {
            service_epoch: source.service_epoch.clone(),
            action: SourceNativeClaudeAction::New {
                directory: directory.clone(),
                arguments: vec![],
            },
        },
    };
    let reply = Ok(SourceReply::NativeClaude { result: Box::new(SourceNativeClaudeResult {
        source, action: SourceNativeClaudeIdentity::New { directory },
        observation: SourceNativeObservation::Observed {
            status: SourceNativeExitStatus::Exited { code: 0 },
            stdout: b"backgrounded \xc2\xb7 a1b2c3d4 \xc2\xb7 <redacted> (idle \xe2\x80\x94 send a prompt to start)\n  claude attach a1b2c3d4    open in this terminal\n".to_vec(),
            stderr: vec![],
        },
    }) });
    assert_eq!(
        observation_lines(&request, &reply),
        ["Started in background: a1b2c3d4. Open it from the list to attach."]
    );
}
