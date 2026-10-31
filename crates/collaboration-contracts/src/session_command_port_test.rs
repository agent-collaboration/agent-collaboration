use super::*;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Eq, PartialEq)]
enum RecordedCommand {
    Create(CreateSessionCommand),
    Load(SessionTargetCommand),
    Resume(SessionTargetCommand),
    Prompt(PromptSessionCommand),
    Steer(SteerSessionCommand),
    QueueAdd(QueueInputCommand),
    QueueList(SessionTargetCommand),
    QueueCancel(SessionTargetCommand, String),
    Cancel(SessionTargetCommand),
    Close(SessionTargetCommand),
    SetSetting(SetSessionSettingCommand),
}

/// A scripted stand-in records the exact front-door command. It does not
/// prove Host composition, provider state, or delivery.
struct ScriptedCommandPort {
    recorded: Arc<Mutex<Vec<RecordedCommand>>>,
    created_session: SessionRef,
}

impl ScriptedCommandPort {
    fn record(&self, command: RecordedCommand) {
        self.recorded.lock().expect("test lock").push(command);
    }
}

impl SessionCommandPort for ScriptedCommandPort {
    fn compact(&self, _command: SessionTargetCommand) -> CommandFuture<'_, SessionTurnHandle> {
        Box::pin(async {
            Ok(SessionTurnHandle {
                turn_id: "compact-turn".into(),
            })
        })
    }
    fn create(&self, command: CreateSessionCommand) -> CommandFuture<'_, SessionRef> {
        self.record(RecordedCommand::Create(command));
        let session = self.created_session.clone();
        Box::pin(async move { Ok(session) })
    }
    fn load_session(&self, command: SessionTargetCommand) -> CommandFuture<'_, ()> {
        self.record(RecordedCommand::Load(command));
        Box::pin(async { Ok(()) })
    }
    fn resume_session(&self, command: SessionTargetCommand) -> CommandFuture<'_, ()> {
        self.record(RecordedCommand::Resume(command));
        Box::pin(async { Ok(()) })
    }
    fn prompt(&self, command: PromptSessionCommand) -> CommandFuture<'_, SessionTurnHandle> {
        self.record(RecordedCommand::Prompt(command));
        Box::pin(async {
            Ok(SessionTurnHandle {
                turn_id: "turn-1".into(),
            })
        })
    }
    fn steer(&self, command: SteerSessionCommand) -> CommandFuture<'_, SessionSteerOutcome> {
        self.record(RecordedCommand::Steer(command));
        Box::pin(async {
            Ok(SessionSteerOutcome::Injected {
                turn_id: "turn-1".into(),
            })
        })
    }
    fn queue_add(&self, command: QueueInputCommand) -> CommandFuture<'_, QueuedSessionInput> {
        self.record(RecordedCommand::QueueAdd(command));
        Box::pin(async {
            Ok(QueuedSessionInput {
                input_id: "input-1".into(),
                position: 1,
                preview: "hello".into(),
            })
        })
    }
    fn queue_list(
        &self,
        command: SessionTargetCommand,
    ) -> CommandFuture<'_, Vec<QueuedSessionInput>> {
        self.record(RecordedCommand::QueueList(command));
        Box::pin(async { Ok(Vec::new()) })
    }
    fn queue_cancel(
        &self,
        command: SessionTargetCommand,
        input_id: String,
    ) -> CommandFuture<'_, ()> {
        self.record(RecordedCommand::QueueCancel(command, input_id));
        Box::pin(async { Ok(()) })
    }
    fn cancel(&self, command: SessionTargetCommand) -> CommandFuture<'_, ()> {
        self.record(RecordedCommand::Cancel(command));
        Box::pin(async { Ok(()) })
    }
    fn close(&self, command: SessionTargetCommand) -> CommandFuture<'_, ()> {
        self.record(RecordedCommand::Close(command));
        Box::pin(async { Ok(()) })
    }
    fn set_setting(&self, command: SetSessionSettingCommand) -> CommandFuture<'_, ()> {
        self.record(RecordedCommand::SetSetting(command));
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn every_command_keeps_the_typed_actor() {
    let actor: Identity =
        serde_json::from_value(serde_json::json!({"kind":"human","humanId":"owner"}))
            .expect("typed human actor");
    let session: SessionRef = serde_json::from_value(serde_json::json!({
        "endpoint":{"serviceId":"0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89","endpointId":"claude-local"},
        "sessionId":"19e49a31-daa3-428c-b985-e0c7373a89ed"
    }))
    .expect("session reference");
    let endpoint: SessionEndpointRef = serde_json::from_value(serde_json::json!({
        "serviceId":"0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89","endpointId":"claude-local"
    }))
    .expect("endpoint");
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let port = ScriptedCommandPort {
        recorded: Arc::clone(&recorded),
        created_session: session.clone(),
    };
    let target = SessionTargetCommand {
        target: session.clone(),
        actor: actor.clone(),
    };
    let content = vec![CommandContent::text("hello".into()).expect("test content")];

    assert_eq!(
        port.create(CreateSessionCommand {
            endpoint,
            working_directory: PathBuf::from("/tmp"),
            settings: SessionSettingsCommand::default(),
            actor: actor.clone(),
        })
        .await
        .expect("create"),
        session
    );
    port.prompt(PromptSessionCommand {
        target: session.clone(),
        input_id: session_event_model::InputId::new("input-prompt").expect("input id"),
        submission: session_event_model::ProviderSubmission::plain(content.clone()),
        actor: actor.clone(),
    })
    .await
    .expect("prompt");
    port.load_session(target.clone()).await.expect("load");
    port.resume_session(target.clone()).await.expect("resume");
    port.steer(SteerSessionCommand {
        target: session.clone(),
        input_id: session_event_model::InputId::new("input-steer").expect("input id"),
        expected_turn_id: "turn-1".into(),
        submission: session_event_model::ProviderSubmission::plain(content.clone()),
        actor: actor.clone(),
    })
    .await
    .expect("steer");
    port.queue_add(QueueInputCommand {
        target: session.clone(),
        submission: session_event_model::ProviderSubmission::plain(content),
        actor: actor.clone(),
    })
    .await
    .expect("queue add");
    port.queue_list(target.clone()).await.expect("queue list");
    port.queue_cancel(target.clone(), "input-1".into())
        .await
        .expect("queue cancel");
    port.cancel(target.clone()).await.expect("cancel");
    port.close(target.clone()).await.expect("close");
    port.set_setting(SetSessionSettingCommand {
        target: session,
        setting_id: "model".into(),
        value: "default".into(),
        intent: session_event_model::SettingApplicationIntent::ExplicitChange,
        expected_model: None,
        actor: actor.clone(),
    })
    .await
    .expect("set setting");

    let recorded = recorded.lock().expect("test lock");
    assert_eq!(recorded.len(), 11);
    assert!(recorded.iter().any(|command| matches!(command,
            RecordedCommand::Prompt(prompt) if prompt.input_id.as_str() == "input-prompt")));
    assert!(recorded.iter().any(|command| matches!(command,
            RecordedCommand::Steer(steer) if steer.input_id.as_str() == "input-steer")));
    assert!(recorded.iter().all(|command| match command {
        RecordedCommand::Create(command) => command.actor == actor,
        RecordedCommand::Prompt(command) => command.actor == actor,
        RecordedCommand::Steer(command) => command.actor == actor,
        RecordedCommand::QueueAdd(command) => command.actor == actor,
        RecordedCommand::Load(command)
        | RecordedCommand::Resume(command)
        | RecordedCommand::QueueList(command)
        | RecordedCommand::QueueCancel(command, _)
        | RecordedCommand::Cancel(command)
        | RecordedCommand::Close(command) => command.actor == actor,
        RecordedCommand::SetSetting(command) => command.actor == actor,
    }));
}
