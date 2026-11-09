//! Both native connections consume the session's shared settings and state events.
use super::test_support::{ScriptedSessionBackend, next_non_skill_frame};
use super::*;
use session_event_model::{SessionSettings, SessionState};
use std::time::Duration;
use tokio_tungstenite::{WebSocketStream, client_async};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
type Client = WebSocketStream<UnixStream>;
const BOUND: Duration = Duration::from_secs(5);

async fn connect(
    context: Arc<RouterSessionAppServerContext>,
) -> TestResult<(
    Client,
    tokio::task::JoinHandle<Result<(), AppServerConnectionError>>,
)> {
    let (client, server) = UnixStream::pair()?;
    let task = tokio::spawn(serve_router_session_app_server_connection(server, context));
    Ok((client_async("ws://localhost/rpc", client).await?.0, task))
}

async fn next_json(client: &mut Client) -> TestResult<Value> {
    let frame = tokio::time::timeout(BOUND, next_non_skill_frame(client))
        .await?
        .ok_or("missing frame")??;
    Ok(serde_json::from_str(frame.to_text()?)?)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Protocol assertions fail the test; Result propagates transport errors."
)]
async fn a_new_window_receives_only_the_latest_settings_from_the_attachment() -> TestResult {
    // Arrange: settings changed twice before this window subscribed.
    let backend = ScriptedSessionBackend::new()?;
    for (sequence, model, effort) in [(1, "old-model", "low"), (2, "current-model", "high")] {
        backend.history.lock().expect("history").push(HubEvent {
            sequence,
            event: SessionEvent::SettingsChanged {
                settings: SessionSettings {
                    model: Some(model.into()),
                    effort: Some(effort.into()),
                    ..Default::default()
                },
            },
        });
    }
    let context = Arc::new(RouterSessionAppServerContext::new(
        backend.endpoint.clone(),
        ScriptedSessionBackend::actor()?,
        backend.clone(),
        backend,
        test_support::model_catalog(tokio::sync::watch::channel(Vec::new()).1),
    ));
    let (mut client, task) = connect(context).await?;
    let directory = tempfile::tempdir()?;
    // Act: attach via the native startup path.
    client
        .send(Message::Text(
            json!({"id":1,"method":"thread/start","params":{"cwd":directory.path()}})
                .to_string()
                .into(),
        ))
        .await?;
    assert_eq!(next_json(&mut client).await?["id"], 1);
    // Assert: current settings arrive without replaying superseded values.
    let settings = next_json(&mut client).await?;
    assert_eq!(settings["method"], "thread/settings/updated");
    assert_eq!(
        settings["params"]["threadSettings"]["model"],
        "current-model"
    );
    assert_eq!(settings["params"]["threadSettings"]["effort"], "high");
    client
        .send(Message::Text(
            json!({"id":2,"method":"initialize"}).to_string().into(),
        ))
        .await?;
    assert_eq!(
        next_json(&mut client).await?["id"],
        2,
        "no older settings notification follows"
    );
    client.close(None).await?;
    tokio::time::timeout(BOUND, task).await???;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Protocol assertions fail the test; Result propagates transport errors."
)]
async fn both_windows_receive_native_settings_and_state_notifications() -> TestResult {
    // Arrange: two independent native connections attached to the same session.
    let backend = ScriptedSessionBackend::new()?;
    let context = Arc::new(RouterSessionAppServerContext::new(
        backend.endpoint.clone(),
        ScriptedSessionBackend::actor()?,
        backend.clone(),
        backend.clone(),
        test_support::model_catalog(tokio::sync::watch::channel(Vec::new()).1),
    ));
    let (mut first, first_task) = connect(context.clone()).await?;
    let (mut second, second_task) = connect(context).await?;
    let directory = tempfile::tempdir()?;
    first
        .send(Message::Text(
            json!({"id":1,"method":"thread/start","params":{"cwd":directory.path()}})
                .to_string()
                .into(),
        ))
        .await?;
    let started = next_json(&mut first).await?;
    let thread_id = started["result"]["thread"]["id"]
        .as_str()
        .ok_or("thread id")?;
    second
        .send(Message::Text(
            json!({"id":2,"method":"thread/read","params":{"threadId":thread_id}})
                .to_string()
                .into(),
        ))
        .await?;
    assert_eq!(next_json(&mut second).await?["id"], 2);
    tokio::time::timeout(BOUND, async {
        while backend
            .attachment_count
            .load(std::sync::atomic::Ordering::Acquire)
            < 2
        {
            backend.attachment_created.notified().await;
        }
    })
    .await?;

    // Act: existing session-layer facts, independent of which connection writes.
    backend.events.send(HubEvent {
        sequence: 1,
        event: SessionEvent::SettingsChanged {
            settings: SessionSettings {
                model: Some("provider-model-b".into()),
                effort: Some("high".into()),
                ..Default::default()
            },
        },
    })?;
    // Assert: complete native settings shape, with actual cwd/model/effort.
    for client in [&mut first, &mut second] {
        let notification = next_json(client).await?;
        assert_eq!(notification["method"], "thread/settings/updated");
        assert_eq!(notification["params"]["threadId"], thread_id);
        let settings = &notification["params"]["threadSettings"];
        assert_eq!(settings["cwd"], json!(directory.path()));
        assert_eq!(settings["model"], "provider-model-b");
        assert_eq!(settings["modelProvider"], "claude-local");
        assert_eq!(settings["effort"], "high");
        assert_eq!(settings["collaborationMode"]["mode"], "default");
        assert_eq!(
            settings["collaborationMode"]["settings"]["model"],
            "provider-model-b"
        );
        assert_eq!(
            settings["collaborationMode"]["settings"]["reasoning_effort"],
            "high"
        );
        assert_eq!(settings["approvalPolicy"], "on-request");
        assert_eq!(settings["approvalsReviewer"], "user");
        assert_eq!(
            settings["sandboxPolicy"],
            json!({"type":"dangerFullAccess"})
        );
    }
    for (index, state, expected) in [
        (
            2,
            SessionState::Running,
            json!({"type":"active","activeFlags":[]}),
        ),
        (3, SessionState::Idle, json!({"type":"idle"})),
        (
            4,
            SessionState::AuthenticationRequired,
            json!({"type":"systemError"}),
        ),
        (5, SessionState::Closed, json!({"type":"notLoaded"})),
    ] {
        backend.events.send(HubEvent {
            sequence: index,
            event: SessionEvent::StateChanged {
                state: state.clone(),
            },
        })?;
        for client in [&mut first, &mut second] {
            assert_eq!(
                next_json(client).await?,
                json!({"method":"thread/status/changed","params":{"threadId":thread_id,"status":expected}})
            );
            if state == SessionState::Closed {
                assert_eq!(
                    next_json(client).await?,
                    json!({"method":"thread/closed","params":{"threadId":thread_id}})
                );
            }
        }
    }
    first.close(None).await?;
    second.close(None).await?;
    tokio::time::timeout(BOUND, first_task).await???;
    tokio::time::timeout(BOUND, second_task).await???;
    Ok(())
}
