//! Claude choices, captured resume options and user-visible background receipts.
use crate::session_picker::client::client_action_view::*;
use crate::session_picker::details::details_client_inventory::DetailsClientCapabilities;
use crate::session_picker::details::details_client_inventory::DetailsClientReadState;
use crate::session_picker::details::details_dialog_state::SessionDetailsDialog;
use crate::session_picker::new_session::new_session_form::NewSessionClient;
use crate::session_picker::new_session::new_session_form::run_choices;
use crate::session_picker::screen::fixtures::fixture_data::picker_request;
use agent_session_inventory::sessions::SessionPickerRecord;
use agent_session_inventory::sessions::source_ssh_protocol::SourceNativeBinding;
use agent_session_inventory::sessions::source_ssh_protocol::SourceNativeClaudeAction;
use agent_session_inventory::sessions::source_ssh_protocol::SourceNativeClaudeIdentity;
use agent_session_inventory::sessions::source_ssh_protocol::SourceNativeClaudeResult;
use agent_session_inventory::sessions::source_ssh_protocol::SourceNativeExitStatus;

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
    // Owner R13: one Claude client, with a separate Run choice.
    model.new_session.as_mut().unwrap().client = NewSessionClient::ClaudeNativeForeground;
    let choices = run_choices(&model);
    assert!(choices.iter().any(|choice| choice.label == "Foreground"));
    assert!(choices.iter().any(|choice| choice.label == "Background"));
}

#[test]
fn claude_background_new_returns_terminal_handoff_before_native_start() {
    use crate::session_picker::new_session::new_session_client_inventory::CursorCreationAvailability;
    use crate::session_picker::new_session::new_session_client_inventory::NewClientInventoryState;
    use crate::session_picker::new_session::new_session_client_inventory::NewClientSourceCapabilities;
    use crate::session_picker::new_session::new_session_form::NewSessionClient;
    use agent_session_inventory::sessions::source_ssh_protocol::SourceNativeLaunchAvailability;

    let mut request = picker_request();
    request.native_arguments = ["--model", "haiku", "--name", "literal --bg value"]
        .map(std::ffi::OsString::from)
        .to_vec();
    let arguments = request.native_arguments.clone();
    let mut model = SessionsPickerModel::new(request, 120);
    model.open_new_form();
    let source = binding();
    let mut draft = model.new_session.take().expect("new draft");
    draft.client = NewSessionClient::ClaudeNativeBackground;
    draft.client_inventory =
        NewClientInventoryState::Ready(Box::new(NewClientSourceCapabilities {
            service_id: source.service_id,
            service_epoch: source.service_epoch,
            native_claude: SourceNativeLaunchAvailability::Ready {
                capabilities: vec![],
            },
            cursor: CursorCreationAvailability::Unavailable("not used".to_owned()),
        }));

    model.begin_new_client_action(&draft);

    let PickerClientAction::Terminal(captured, mode) = &model.client_action else {
        panic!(
            "background New must leave the picker before starting native Claude; got {:?}",
            model.client_action
        );
    };
    assert_eq!(*mode, agent_session_inventory::sessions::claude_terminal_launch::ClaudeTerminalMode::BackgroundThenAttach);
    assert!(matches!(
        &captured.effect,
        SessionClientEffect::Native { action: SourceNativeClaudeAction::New { arguments: actual, .. }, .. }
            if actual == &arguments
    ));
}

#[test]
fn claude_stopped_details_offers_open_without_background_resume() {
    let summary = serde_json::from_value(serde_json::json!({
        "origin":"claudeCodeBackground",
        "target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},"sessionId":"00000000-0000-4000-8000-000000000042"},
        "jobHandle":"deadbeef","workingDirectory":"/source/project","startedAt":1,"state":{"kind":"stopped"}
    })).unwrap();
    let record = SessionPickerRecord::from_provider_summary(&summary, "Native Claude");
    let mut dialog = SessionDetailsDialog::capture(&record);
    dialog.client_inventory =
        DetailsClientReadState::Ready(DetailsClientCapabilities::Native { source: binding() });
    let actions = dialog.actions();
    assert!(
        actions
            .iter()
            .any(|action| dialog.action_label(*action) == "Open")
    );
    assert!(
        actions
            .iter()
            .all(|action| dialog.action_label(*action) != "Resume in background"),
        "stopped Claude sessions must use Open to resume and attach"
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
