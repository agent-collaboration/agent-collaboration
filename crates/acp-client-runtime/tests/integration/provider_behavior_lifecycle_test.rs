//! Same-ID fixture lifecycle through the real ACP client, including actual history replay.

use super::{CountingEventSink, NoopInteractionPort};
use acp_client_runtime::{AgentSessionClient, ExternalProviderLaunch, ProviderPersistenceTarget};
use session_event_model::{
    InputId, PromptContent, ProviderSubmission, SessionEvent, SessionItemKind,
};
use std::{
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn require(condition: bool, message: &str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(std::io::Error::other(message).into())
    }
}

async fn prompt_and_require_reply(
    client: &AgentSessionClient<NoopInteractionPort>,
    session: &str,
    text: &str,
) -> TestResult {
    let result = client
        .submit_with_approval_dispatch_for_input(
            session.to_owned(),
            InputId::generate(),
            acp_client_runtime::ProviderTurnInput::Prompt(ProviderSubmission::plain(vec![
                PromptContent::text(text.to_owned())?,
            ])),
            (),
            None,
        )
        .await?;
    require(
        result.output == "beforeafter"
            && result.stop_reason == session_event_model::StopReason::EndTurn,
        "prompt must return its real nonempty reply and terminal",
    )
}

/// Oracle: the locked ACP close/resume/load contract retains provider identity,
/// resumes without replay and loads the actual earlier messages on that same wire.
#[tokio::test]
async fn lifecycle_profile_reopens_same_id_and_replays_only_on_load() {
    // Arrange: real behavior peer and real AgentSessionClient, no application backend stand-in.
    let root = tempfile::tempdir().expect("fixture root");
    let receipt = root.path().join("lifecycle.jsonl");
    let sink = Arc::new(CountingEventSink::default());
    let client = tokio::time::timeout(
        Duration::from_secs(3),
        AgentSessionClient::initialize(
            ExternalProviderLaunch {
                executable: PathBuf::from("node"),
                arguments: vec![
                    concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/tests/support/behavior-agent.ts"
                    )
                    .to_owned(),
                    receipt.display().to_string(),
                    "lifecycle".to_owned(),
                ],
                environment: Vec::new(),
                persistence_target: ProviderPersistenceTarget::Unspecified,
            },
            Arc::new(NoopInteractionPort),
            sink.clone(),
            Arc::new(crate::test_behavior::TestProviderBehavior),
        ),
    )
    .await
    .expect("bounded initialization")
    .expect("peer initializes");
    // Act/Assert inside a fallible scenario so every failed observation still shuts the client down.
    let observed = tokio::time::timeout(Duration::from_secs(10), async {
        let session = client.create_session(root.path().to_path_buf()).await?;
        let capabilities = client.capability_report(&session).await;
        require(
            capabilities.supports_load
                && capabilities.supports_resume
                && capabilities.supports_close,
            "named lifecycle profile must advertise all three operations",
        )?;
        prompt_and_require_reply(&client, &session, "first owned input").await?;
        client.close_session(session.clone()).await?;
        sink.1
            .lock()
            .map_err(|_| std::io::Error::other("events poisoned"))?
            .clear();
        client
            .resume_session(session.clone(), root.path().to_path_buf())
            .await?;
        require(
            sink.0.load(Ordering::Relaxed) == 0,
            "resume must not begin history replay",
        )?;
        require(
            !sink
                .1
                .lock()
                .map_err(|_| std::io::Error::other("events poisoned"))?
                .iter()
                .any(|event| {
                    matches!(
                        event,
                        SessionEvent::ItemStarted { .. } | SessionEvent::ItemUpdated { .. }
                    )
                }),
            "resume must not replay prior message items",
        )?;
        prompt_and_require_reply(&client, &session, "second owned input").await?;
        client.close_session(session.clone()).await?;
        sink.1
            .lock()
            .map_err(|_| std::io::Error::other("events poisoned"))?
            .clear();
        client
            .load_session(session.clone(), root.path().to_path_buf())
            .await?;
        require(
            sink.0.load(Ordering::Relaxed) == 1,
            "load must begin the actual replay owner",
        )?;
        let replayed = sink
            .1
            .lock()
            .map_err(|_| std::io::Error::other("events poisoned"))?
            .iter()
            .filter_map(|event| match event {
                SessionEvent::ItemStarted { item } | SessionEvent::ItemUpdated { item }
                    if item.kind == SessionItemKind::UserMessage =>
                {
                    item.text.clone()
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        require(
            replayed.iter().any(|text| text == "first owned input")
                && replayed.iter().any(|text| text == "second owned input"),
            "load must replay the actual two accepted inputs, not canned history",
        )?;
        prompt_and_require_reply(&client, &session, "after real load").await?;
        client.close_session(session.clone()).await?;
        let requests = std::fs::read_to_string(&receipt)?
            .lines()
            .map(serde_json::from_str::<serde_json::Value>)
            .collect::<Result<Vec<_>, _>>()?;
        let methods = requests
            .iter()
            .filter_map(|request| request.get("method").and_then(serde_json::Value::as_str))
            .collect::<Vec<_>>();
        require(
            methods
                == [
                    "session/new",
                    "session/prompt",
                    "session/close",
                    "session/resume",
                    "session/prompt",
                    "session/close",
                    "session/load",
                    "session/prompt",
                    "session/close",
                ],
            "actual provider lifecycle method order changed",
        )?;
        require(
            requests
                .iter()
                .filter(|request| {
                    request.get("method").and_then(serde_json::Value::as_str) != Some("session/new")
                })
                .all(|request| {
                    request
                        .pointer("/params/sessionId")
                        .and_then(serde_json::Value::as_str)
                        == Some(session.as_str())
                }),
            "restore and subsequent work must keep the actual created Session ID",
        )?;
        Ok::<(), Box<dyn std::error::Error>>(())
    })
    .await;
    tokio::time::timeout(Duration::from_secs(3), client.shutdown())
        .await
        .expect("owned client shutdown");
    assert!(observed.is_ok(), "lifecycle deadline: {observed:?}");
    assert!(
        matches!(observed, Ok(Ok(()))),
        "lifecycle evidence: {observed:?}"
    );
}
