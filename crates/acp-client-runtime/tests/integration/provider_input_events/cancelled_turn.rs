use super::*;
use acp_client_runtime::ExternalProviderRuntimeError;
use serde_json::Value;
use session_event_model::{PromptContent, ProviderSubmission, SessionItemKind};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

#[tokio::test]
async fn ready_cancelled_turn_update_is_not_a_successor_turn_item() {
    let root = tempfile::tempdir_in("/tmp").expect("fixture root");
    let socket = root.path().join("control.sock");
    let receipt = root.path().join("wire-prompts.json");
    let listener = tokio::net::UnixListener::bind(&socket).expect("control socket");
    let (event_sender, mut events) = tokio::sync::mpsc::unbounded_channel();
    let client = AgentSessionClient::initialize(
        ExternalProviderLaunch {
            executable: PathBuf::from("node"),
            arguments: vec![
                concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../verification/fixtures/provider-failed-turn-agent.ts"
                )
                .to_owned(),
                "cancel".to_owned(),
                socket.display().to_string(),
                receipt.display().to_string(),
            ],
            environment: Vec::new(),
            persistence_target: ProviderPersistenceTarget::Unspecified,
        },
        Arc::new(NoopInteractionPort),
        Arc::new(CaptureEventSink(event_sender)),
        Arc::new(crate::test_behavior::TestProviderBehavior),
    )
    .await
    .expect("fixture initializes");
    let (mut control, _) = listener.accept().await.expect("control connection");
    let session = client
        .create_session(root.path().to_owned())
        .await
        .expect("session opens");
    let submit = |text: &str| {
        client.submit_with_approval_dispatch_for_input(
            session.clone(),
            InputId::generate(),
            acp_client_runtime::ProviderTurnInput::Prompt(ProviderSubmission::plain(vec![
                PromptContent::text(text.to_owned()).expect("prompt text"),
            ])),
            (),
            None,
        )
    };
    assert!(matches!(
        submit("overflow").await,
        Err(ExternalProviderRuntimeError::PromptOutputLimitExceeded)
    ));
    control
        .write_all(b"g")
        .await
        .expect("release late update before a successor is admitted");
    let mut acknowledged = [0; 1];
    control
        .read_exact(&mut acknowledged)
        .await
        .expect("provider acknowledged late update");
    assert_eq!(&acknowledged, b"r");
    let late = loop {
        match events.recv().await.expect("idle late update") {
            SessionEvent::ItemStarted { item } if item.text.as_deref() == Some("LATE_OUTPUT") => {
                break item.item_id;
            }
            _ => {}
        }
    };
    let next = submit("next").await.expect("successor prompt settles");
    assert_eq!(next.output, "CURRENT_OUTPUT");
    let mut successor = Vec::new();
    let started = next_turn_start(&mut events).await;
    let SessionEvent::TurnStarted { turn_id, .. } = started else {
        panic!("expected successor TurnStarted");
    };
    loop {
        let event = events.recv().await.expect("successor events");
        if matches!(&event, SessionEvent::TurnEnded { turn_id: ended, .. } if ended == &turn_id) {
            break;
        }
        successor.push(event);
    }
    let user = successor
        .iter()
        .find_map(|event| match event {
            SessionEvent::ItemStarted { item } if item.kind == SessionItemKind::UserMessage => {
                Some(item)
            }
            _ => None,
        })
        .expect("successor user item");
    assert_eq!(user.text.as_deref(), Some("next"));
    assert!(successor.iter().any(
        |event| matches!(event, SessionEvent::ItemCompleted { item_id } if item_id == &user.item_id)
    ));
    assert!(successor.iter().any(|event| matches!(event, SessionEvent::ItemStarted { item } if item.text.as_deref() == Some("CURRENT_OUTPUT") && item.item_id != late)));
    assert!(
        successor.iter().all(|event| match event {
            SessionEvent::ItemStarted { item } | SessionEvent::ItemUpdated { item } =>
                item.item_id != late
                    && (item.item_id == user.item_id
                        || item.text.as_deref() == Some("CURRENT_OUTPUT")),
            SessionEvent::ItemCompleted { item_id } => item_id != &late,
            _ => true,
        }),
        "late item must not enter the successor stream: {successor:?}"
    );
    let wire: Value =
        serde_json::from_slice(&std::fs::read(receipt).expect("wire receipt")).expect("wire JSON");
    assert_eq!(
        wire,
        serde_json::json!({"promptCount":2,"promptTexts":["overflow","next"]})
    );
    client.shutdown().await;
}
