use super::*;
use codex_acp_adapter::bounded_acp_output;
use collaboration_contracts::{
    CommandFuture, HubEvent, HubFuture, HubSessionSummary, QueuedSessionInput,
    SessionEventAttachment, SessionTurnHandle, SetSessionSettingCommand, SkillViewAttachment,
};
use session_event_model::CapabilityReport;
use std::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Records the create command and answers just enough of the hub for `session/new`.
struct RecordingSessionBackend {
    session: SessionRef,
    approver: Identity,
    created: Mutex<Vec<CreateSessionCommand>>,
}

impl RecordingSessionBackend {
    fn new(endpoint: SessionEndpointRef) -> Result<Arc<Self>, serde_json::Error> {
        let session: SessionRef = serde_json::from_value(json!({
            "endpoint":endpoint,
            "sessionId":"19e49a31-daa3-428c-b985-e0c7373a89ed"
        }))?;
        Ok(Arc::new(Self {
            session,
            approver: acp_actor()?,
            created: Mutex::new(Vec::new()),
        }))
    }
}

fn acp_actor() -> Result<Identity, serde_json::Error> {
    serde_json::from_value(json!({"kind":"human","humanId":"owner"}))
}

impl SessionCommandPort for RecordingSessionBackend {
    fn create(&self, command: CreateSessionCommand) -> CommandFuture<'_, SessionRef> {
        self.created.lock().expect("test lock").push(command);
        let session = self.session.clone();
        Box::pin(async move { Ok(session) })
    }
    fn load_session(&self, _: SessionTargetCommand) -> CommandFuture<'_, ()> {
        Box::pin(async { Err(CommandFailure::Unsupported) })
    }
    fn resume_session(&self, _: SessionTargetCommand) -> CommandFuture<'_, ()> {
        Box::pin(async { Err(CommandFailure::Unsupported) })
    }
    fn prompt(&self, _: PromptSessionCommand) -> CommandFuture<'_, SessionTurnHandle> {
        Box::pin(async { Err(CommandFailure::Unsupported) })
    }
    fn compact(&self, _: SessionTargetCommand) -> CommandFuture<'_, SessionTurnHandle> {
        Box::pin(async { Err(CommandFailure::Unsupported) })
    }
    fn steer(&self, _: SteerSessionCommand) -> CommandFuture<'_, SessionSteerOutcome> {
        Box::pin(async { Err(CommandFailure::Unsupported) })
    }
    fn queue_add(&self, _: QueueInputCommand) -> CommandFuture<'_, QueuedSessionInput> {
        Box::pin(async { Err(CommandFailure::Unsupported) })
    }
    fn queue_list(&self, _: SessionTargetCommand) -> CommandFuture<'_, Vec<QueuedSessionInput>> {
        Box::pin(async { Err(CommandFailure::Unsupported) })
    }
    fn queue_cancel(&self, _: SessionTargetCommand, _: String) -> CommandFuture<'_, ()> {
        Box::pin(async { Err(CommandFailure::Unsupported) })
    }
    fn cancel(&self, _: SessionTargetCommand) -> CommandFuture<'_, ()> {
        Box::pin(async { Err(CommandFailure::Unsupported) })
    }
    fn close(&self, _: SessionTargetCommand) -> CommandFuture<'_, ()> {
        Box::pin(async { Err(CommandFailure::Unsupported) })
    }
    fn set_setting(&self, _: SetSessionSettingCommand) -> CommandFuture<'_, ()> {
        Box::pin(async { Err(CommandFailure::Unsupported) })
    }
}

impl SessionEventHub for RecordingSessionBackend {
    fn skill_view(&self, _: SessionRef, _: PathBuf) -> HubFuture<'_, SkillViewAttachment> {
        Box::pin(async { Err(collaboration_contracts::SessionEventHubError::Unavailable) })
    }
    fn attach(&self, _: SessionRef) -> HubFuture<'_, SessionEventAttachment> {
        let (events, receiver) = tokio::sync::broadcast::channel::<HubEvent>(1);
        let snapshot = vec![HubEvent {
            sequence: 1,
            event: SessionEvent::CapabilitiesChanged {
                capabilities: CapabilityReport::default(),
            },
        }];
        Box::pin(async move {
            drop(events);
            Ok(SessionEventAttachment {
                snapshot,
                receiver,
                epoch: 0,
            })
        })
    }
    fn state(&self, _: SessionRef) -> HubFuture<'_, SessionState> {
        Box::pin(async { Ok(SessionState::Idle) })
    }
    fn sessions(&self, _: SessionEndpointRef) -> HubFuture<'_, Vec<HubSessionSummary>> {
        let summary = HubSessionSummary {
            session: self.session.clone(),
            approver: self.approver.clone(),
            working_directory: PathBuf::from("/"),
            updated_at_seconds: 1_700_000_000,
            preview: String::new(),
            name: None,
            model: None,
            mode: None,
            state: SessionState::Idle,
        };
        Box::pin(async move { Ok(vec![summary]) })
    }
}

/// Oracle: spec R1 "Those doors ask for auto by name". ACP `session/new` has no
/// field for a permission mode, so the Router's ACP socket door names `auto` itself.
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn session_new_asks_for_auto() -> Result<(), Box<dyn std::error::Error>> {
    // Arrange: the route over an in-memory frame pair and a recording backend.
    let directory = tempfile::tempdir()?;
    let endpoint: SessionEndpointRef = serde_json::from_value(json!({
        "serviceId":"0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89",
        "endpointId":"claude-local"
    }))?;
    let backend = RecordingSessionBackend::new(endpoint.clone())?;
    let closed = CancellationToken::new();
    let (frame_sender, frame_receiver) = bounded_acp_output(closed.clone());
    let (output_sender, mut output_receiver) = bounded_acp_output(closed.clone());
    let route = Box::new(ProviderAcpSessionRoute::new(
        endpoint,
        Arc::clone(&backend) as Arc<dyn SessionCommandPort>,
        Arc::clone(&backend) as Arc<dyn SessionEventHub>,
    ));
    let served = tokio::spawn(route.run(
        codex_acp_adapter::AcpRouterChannels {
            input: frame_receiver,
            output: output_sender,
            closed: closed.clone(),
        },
        AcpConnectionContext {
            actor: Some(acp_actor()?),
            client_profile: None,
            client_supports_elicitation_form: false,
        },
    ));

    // Act: one valid session/new request, then wait for its response.
    frame_sender
        .send(json!({"jsonrpc":"2.0","id":7,"method":"session/new",
            "params":{"cwd":directory.path(),"mcpServers":[]}}))
        .await?;
    let reply = tokio::time::timeout(std::time::Duration::from_secs(5), output_receiver.recv())
        .await?
        .ok_or("route closed before replying")?;

    // Assert: the create succeeded and asked for mode auto, nothing else set.
    assert_eq!(reply["id"], 7, "{}", *reply);
    assert!(reply.get("error").is_none(), "{}", *reply);
    let created = backend.created.lock().expect("test lock");
    assert_eq!(created.len(), 1);
    assert_eq!(
        created[0].settings,
        SessionSettingsCommand {
            model: None,
            mode: Some("auto".into()),
            effort: None,
        }
    );
    closed.cancel();
    drop(created);
    tokio::time::timeout(std::time::Duration::from_secs(5), served).await???;
    Ok(())
}
