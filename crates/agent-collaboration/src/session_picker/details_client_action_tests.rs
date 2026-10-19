//! Mounted Details actions capture identity before an external effect port starts.
use super::*;
use crate::presentation::session_picker::{
    PickerSourceContext, SessionsPickerOutcome, picker_component::SessionsPickerComponent,
    test_support::picker_request,
};
use crate::sessions::{
    SessionPickerRecord,
    source_client_action::{CapturedClientAction, SessionClientEffect, SessionClientExecutor},
    source_ssh_protocol::SourceManagedAction,
};
use futures_util::StreamExt;
use iocraft::prelude::*;
use std::sync::Mutex;
fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}
fn control(character: char) -> TerminalEvent {
    let mut key = KeyEvent::new(KeyEventKind::Press, KeyCode::Char(character));
    key.modifiers = KeyModifiers::CONTROL;
    TerminalEvent::Key(key)
}
fn managed_binding() -> ProviderBindingIdentity {
    serde_json::from_value(serde_json::json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"cursor-local"},
        "bindingId":"actual-cursor-binding","runtime":{"provider":"cursor","runtimeName":"fixture"},"transport":"stdioAcp",
        "generation":{"serviceEpoch":"00000000-0000-4000-8000-000000000002","generation":7},
        "capabilities":[{"name":"load","status":"supported","evidence":"advertised"}]})).unwrap()
}
#[tokio::test]
async fn managed_details_load_is_explicit_once_and_keeps_captured_source_target_and_operation() {
    let binding = managed_binding();
    let target: collaboration_client::protocol::SessionRef = serde_json::from_value(
        serde_json::json!({"endpoint":binding.endpoint,"sessionId":"captured-stored-cursor"}),
    )
    .unwrap();
    let summary=serde_json::from_value(serde_json::json!({"origin":"hostedProvider","target":target,"workingDirectory":"/source/saved-directory","updatedAt":2,"state":"unloaded","requestedPolicy":{"access":"write-restricted"},"approver":{"humanId":"saved-approver"},"createdBy":{"humanId":"creator"}})).unwrap();
    let mut record = SessionPickerRecord::from_provider_summary(&summary, "Captured Cursor");
    record.source_context = Some(PickerSourceContext::DefaultHosted);
    let inspector: DetailsClientInspector = Arc::new({
        let binding = binding.clone();
        let target = target.clone();
        move |query| {
            assert_eq!(query.source, PickerSourceContext::DefaultHosted);
            assert!(
                matches!(&query.record.identity,crate::sessions::SessionPickerIdentity::HostedProvider(actual) if *actual==target)
            );
            let binding = binding.clone();
            Box::pin(async move {
                Ok(DetailsClientCapabilities::Managed {
                    binding: Box::new(binding),
                    actions: ManagedRestoreActions::LoadAndResume,
                })
            })
        }
    });
    let effects = Arc::new(Mutex::new(Vec::<CapturedClientAction>::new()));
    let executor: SessionClientExecutor = Arc::new({
        let effects = effects.clone();
        move |request| {
            effects.lock().unwrap().push(request);
            Box::pin(std::future::pending())
        }
    });
    let mut request = picker_request();
    request.records = vec![record];
    request.root = crate::presentation::session_picker::SessionsPickerRoot::Any;
    request.source = crate::sessions::SessionsSource::All;
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    sender.send(key(KeyCode::F(4))).unwrap();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut outcome = None;
    let mut phase = 0;
    let mut last = String::new();
    {
        let mut component = element!(SessionsPickerComponent(request,details_client_inspector:Some(inspector),client_executor:Some(executor),width:76usize,height:30usize,selected_outcome_out:&mut outcome));
        let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        let journey = async {
            while let Some(canvas) = frames.next().await {
                let frame = canvas.to_string();
                if phase == 0
                    && frame.contains("Session details")
                    && frame
                        .lines()
                        .any(|line| line.contains("Load") && line.contains("Resume"))
                {
                    assert!(
                        effects.lock().unwrap().is_empty(),
                        "inspection is read-only"
                    );
                    assert!(
                        !frame.contains("Read-only here; provider resume is unavailable"),
                        "qualified actions must not carry an unavailable claim: {frame}"
                    );
                    sender.send(key(KeyCode::Tab)).unwrap();
                    sender.send(key(KeyCode::Enter)).unwrap();
                    phase = 1;
                } else if phase == 1 && frame.contains("Waiting for the selected source result") {
                    assert_eq!(effects.lock().unwrap().len(), 1);
                    sender.send(key(KeyCode::Enter)).unwrap();
                    sender.send(key(KeyCode::Enter)).unwrap();
                    sender.send(control('n')).unwrap();
                    sender.send(control('d')).unwrap();
                    phase = 2;
                }
                last = frame;
            }
        };
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(2), journey)
                .await
                .is_ok(),
            "phase={phase}: {last}"
        );
    }
    assert_eq!(phase, 2);
    let effects = effects.lock().unwrap();
    assert_eq!(effects.len(), 1);
    let effect = effects.first().unwrap();
    assert_eq!(effect.source, PickerSourceContext::DefaultHosted);
    assert_eq!(
        String::from(effect.service_id.clone()),
        "00000000-0000-4000-8000-000000000001"
    );
    assert!(
        matches!(&effect.effect,SessionClientEffect::Managed {binding:actual,action:SourceManagedAction::Load {target:actual_target},..} if *actual==binding&&*actual_target==target)
    );
    assert!(
        matches!(outcome,Some(SessionsPickerOutcome::InterruptedClientAction(retained)) if retained.as_ref()==effect)
    );
}

#[tokio::test]
async fn native_details_fork_confirms_captured_directory_and_submits_one_native_effect() {
    use crate::sessions::{
        SessionPickerIdentity,
        source_ssh_protocol::{SourceNativeBinding, SourceNativeClaudeAction},
    };
    let target:collaboration_client::protocol::SessionRef=serde_json::from_value(serde_json::json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},"sessionId":"00000000-0000-4000-8000-000000000042"})).unwrap();
    let summary=serde_json::from_value(serde_json::json!({"origin":"claudeCodeInteractive","target":target,"workingDirectory":"/source/native-directory","name":"Captured native Claude","status":"idle","startedAt":1,"updatedAt":2,"statusUpdatedAt":2,"kind":"interactive","entrypoint":"cli"})).unwrap();
    let mut record = SessionPickerRecord::from_provider_summary(&summary, "Native Claude");
    record.source_context = Some(PickerSourceContext::DefaultHosted);
    let inspector: DetailsClientInspector = Arc::new(move |query| {
        assert_eq!(query.source, PickerSourceContext::DefaultHosted);
        assert!(matches!(
            query.record.identity,
            SessionPickerIdentity::NativeClaude(_)
        ));
        Box::pin(async {
            Ok(DetailsClientCapabilities::Native {
                source: SourceNativeBinding {
                    service_id: "00000000-0000-4000-8000-000000000001"
                        .to_owned()
                        .try_into()
                        .unwrap(),
                    service_epoch: "00000000-0000-4000-8000-000000000002"
                        .to_owned()
                        .try_into()
                        .unwrap(),
                },
            })
        })
    });
    let effects = Arc::new(Mutex::new(Vec::<CapturedClientAction>::new()));
    let executor: SessionClientExecutor = Arc::new({
        let effects = effects.clone();
        move |request| {
            effects.lock().unwrap().push(request);
            Box::pin(std::future::pending())
        }
    });
    let mut request = picker_request();
    request.records = vec![record];
    request.root = crate::presentation::session_picker::SessionsPickerRoot::Any;
    request.source = crate::sessions::SessionsSource::All;
    request.current_dir = "/wrong/new-form-directory".into();
    request.native_arguments = ["--name", "literal --bg value"]
        .map(std::ffi::OsString::from)
        .to_vec();
    let arguments = request.native_arguments.clone();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    sender.send(key(KeyCode::F(4))).unwrap();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut outcome = None;
    let mut phase = 0;
    let mut last = String::new();
    {
        let mut component = element!(SessionsPickerComponent(request,details_client_inspector:Some(inspector),client_executor:Some(executor),width:76usize,height:30usize,selected_outcome_out:&mut outcome));
        let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        let journey = async {
            while let Some(canvas) = frames.next().await {
                let frame = canvas.to_string();
                if phase == 0
                    && frame.contains("Session details")
                    && frame
                        .lines()
                        .any(|line| line.contains("Fork") && line.contains("Resume"))
                {
                    assert!(effects.lock().unwrap().is_empty());
                    sender.send(key(KeyCode::Tab)).unwrap();
                    sender.send(key(KeyCode::Enter)).unwrap();
                    phase = 1;
                } else if phase == 1 && frame.contains("Fork session") {
                    assert!(
                        frame.contains("/source/native-directory"),
                        "native fork must use captured source directory: {frame}"
                    );
                    assert!(!frame.contains("/wrong/new-form-directory"));
                    assert!(
                        effects.lock().unwrap().is_empty(),
                        "confirmation must precede effects"
                    );
                    sender.send(key(KeyCode::Enter)).unwrap();
                    phase = 2;
                } else if phase == 2 && frame.contains("Waiting for the selected source result") {
                    assert!(
                        !frame.contains("Operation:"),
                        "native actions have no Router operation ID: {frame}"
                    );
                    sender.send(key(KeyCode::Enter)).unwrap();
                    sender.send(control('n')).unwrap();
                    sender.send(control('d')).unwrap();
                    phase = 3;
                }
                last = frame;
            }
        };
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(2), journey)
                .await
                .is_ok(),
            "phase={phase}: {last}"
        );
    }
    assert_eq!(phase, 3);
    let effects = effects.lock().unwrap();
    assert_eq!(effects.len(), 1);
    let effect = effects.first().unwrap();
    assert_eq!(effect.source, PickerSourceContext::DefaultHosted);
    assert_eq!(
        String::from(effect.service_id.clone()),
        "00000000-0000-4000-8000-000000000001"
    );
    assert!(
        matches!(&effect.effect,SessionClientEffect::Native {service_epoch,action:SourceNativeClaudeAction::Fork {target:actual,directory,arguments:actual_arguments}} if String::from(service_epoch.clone())=="00000000-0000-4000-8000-000000000002"&&*actual==target&&directory.as_str()=="/source/native-directory"&&*actual_arguments==arguments)
    );
    assert!(
        matches!(outcome,Some(SessionsPickerOutcome::InterruptedClientAction(retained)) if retained.as_ref()==effect)
    );
}
