//! Mounted UI capture/admission/detachment. The executor is the external effect
//! port; actual source/API/vendor interactions have their own process gates.
use super::*;
use crate::session_picker::{
    new_session_client_inventory::{CursorCreationAvailability, NewClientSourceCapabilities},
    picker_component::SessionsPickerComponent,
    test_support::picker_request,
};
use agent_session_inventory::sessions::source_ssh_protocol::SourceNativeLaunchAvailability;
use collaboration_client::protocol::ProviderBindingIdentity;
use futures_util::StreamExt;
use std::sync::{Arc, Mutex};

fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}
fn control(character: char) -> TerminalEvent {
    let mut event = KeyEvent::new(KeyEventKind::Press, KeyCode::Char(character));
    event.modifiers = KeyModifiers::CONTROL;
    TerminalEvent::Key(event)
}

#[tokio::test]
async fn client_start_captures_one_effect_and_detach_retains_native_or_managed_identity() {
    for client_name in ["Cursor", "Claude"] {
        let binding:ProviderBindingIdentity=serde_json::from_value(serde_json::json!({
            "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"actual-cursor"},
            "bindingId":"cursor-binding","runtime":{"provider":"cursor","runtimeName":"fixture"},"transport":"stdioAcp",
            "generation":{"serviceEpoch":"00000000-0000-4000-8000-000000000002","generation":7},
            "capabilities":[{"name":"create","status":"supported","evidence":"advertised"}]
        })).unwrap();
        let capabilities = NewClientSourceCapabilities {
            service_id: binding.endpoint.service_id.clone(),
            service_epoch: binding.generation.service_epoch.clone(),
            native_claude: SourceNativeLaunchAvailability::Ready,
            cursor: CursorCreationAvailability::Ready(binding.clone()),
        };
        let loader: crate::session_picker::NewClientInventoryLoader = Arc::new(move |_| {
            let capabilities = capabilities.clone();
            Box::pin(async move { Ok(capabilities) })
        });
        let captured = Arc::new(Mutex::new(Vec::<CapturedClientAction>::new()));
        let executor: SessionClientExecutor = Arc::new({
            let captured = captured.clone();
            move |request| {
                captured.lock().unwrap().push(request);
                Box::pin(std::future::pending())
            }
        });
        let mut request = picker_request();
        if client_name == "Claude" {
            request.native_arguments = ["--name", "literal --bg value"]
                .map(std::ffi::OsString::from)
                .to_vec();
        }
        let arguments = request.native_arguments.clone();
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let mut outcome: Option<SessionsPickerOutcome> = None;
        let mut phase = 0;
        let mut last = String::new();
        {
            let mut picker = element!(SessionsPickerComponent(request,client_inventory_loader:Some(loader),client_executor:Some(executor),width:120usize,height:36usize,selected_outcome_out:&mut outcome));
            let frames = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
            tokio::pin!(frames);
            let journey = async {
                while let Some(canvas) = frames.next().await {
                    let frame = canvas.to_string();
                    match phase {
                        0 => {
                            sender.send(control('n')).unwrap();
                            phase = 1;
                        }
                        1 if frame.contains("New session") => {
                            for code in [
                                KeyCode::BackTab,
                                KeyCode::BackTab,
                                KeyCode::BackTab,
                                KeyCode::BackTab,
                                KeyCode::Enter,
                            ] {
                                sender.send(key(code)).unwrap();
                            }
                            phase = 2;
                        }
                        2 if frame.contains("Search:") => {
                            sender
                                .send(TerminalEvent::BracketedPaste(client_name.to_owned()))
                                .unwrap();
                            phase = 3;
                        }
                        3 if frame.contains(&format!("Search: {client_name}")) => {
                            sender.send(key(KeyCode::Enter)).unwrap();
                            phase = 4;
                        }
                        4 if !frame.contains("Search:")
                            && frame.lines().any(|line| {
                                line.contains("Client")
                                    && line.contains(client_name)
                                    && line.contains('▾')
                            }) =>
                        {
                            assert!(captured.lock().unwrap().is_empty(), "choice cannot submit");
                            for code in [
                                KeyCode::Tab,
                                KeyCode::Tab,
                                KeyCode::Tab,
                                KeyCode::Tab,
                                KeyCode::Enter,
                            ] {
                                sender.send(key(code)).unwrap();
                            }
                            phase = 5;
                        }
                        5 if frame.contains("Client action")
                            && !captured.lock().unwrap().is_empty() =>
                        {
                            assert!(
                                frame
                                    .lines()
                                    .any(|line| line.contains("Source: This machine")),
                                "captured source is visible: {frame}"
                            );
                            for event in [
                                key(KeyCode::Enter),
                                key(KeyCode::Enter),
                                control('n'),
                                control('d'),
                            ] {
                                sender.send(event).unwrap();
                            }
                            phase = 6;
                        }
                        _ => {}
                    }
                    last = frame;
                }
            };
            assert!(
                tokio::time::timeout(std::time::Duration::from_secs(2), journey)
                    .await
                    .is_ok(),
                "{client_name} phase={phase}: {last}"
            );
        }
        assert_eq!(phase, 6);
        let captured = captured.lock().unwrap();
        assert_eq!(
            captured.len(),
            1,
            "pending repeated Start/background keys cannot replay"
        );
        assert_eq!(
            captured[0].source,
            agent_session_inventory::source_inventory_request::PickerSourceContext::DefaultHosted
        );
        assert_eq!(
            String::from(captured[0].service_id.clone()),
            "00000000-0000-4000-8000-000000000001"
        );
        let Some(SessionsPickerOutcome::InterruptedClientAction(detached)) = outcome else {
            panic!("detachment must retain uncertain action")
        };
        assert_eq!(detached.as_ref(), &captured[0]);
        match &detached.effect {
            SessionClientEffect::Native {
                service_epoch,
                action:
                    agent_session_inventory::sessions::source_ssh_protocol::SourceNativeClaudeAction::New {
                        directory,
                        arguments: actual,
                    },
            } if client_name == "Claude" => {
                assert_eq!(
                    String::from(service_epoch.clone()),
                    "00000000-0000-4000-8000-000000000002"
                );
                assert_eq!(directory.as_str(), "/repo/project-a");
                assert_eq!(actual, &arguments);
                assert!(
                    !detached
                        .unknown_description()
                        .contains("Retained operation")
                );
            }
            SessionClientEffect::Managed {
                binding: actual,
                action:
                    agent_session_inventory::sessions::source_ssh_protocol::SourceManagedAction::Create { directory },
                operation_id,
            } if client_name == "Cursor" => {
                assert_eq!(actual, &binding);
                assert_eq!(directory.as_str(), "/repo/project-a");
                assert!(
                    detached
                        .unknown_description()
                        .contains(operation_id.as_str())
                );
            }
            other => panic!("wrong client/action: {other:?}"),
        }
    }
}
