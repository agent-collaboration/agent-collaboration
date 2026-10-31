//! Read-only native cwd membership, with no provider or model involved.
use super::test_support::{ScriptedSessionBackend, next_non_skill_frame};
use super::*;
use session_event_model::SessionState;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio_tungstenite::{WebSocketStream, client_async};

type NativeClient = WebSocketStream<UnixStream>;
type TestResult<T> = Result<T, Box<dyn std::error::Error>>;
const A: &str = "11111111-1111-4111-8111-111111111111";
const B: &str = "22222222-2222-4222-8222-222222222222";
const FOREIGN: &str = "33333333-3333-4333-8333-333333333333";
const BOUND: Duration = Duration::from_secs(5);

fn seeded_backend(a: &Path, b: &Path) -> TestResult<Arc<ScriptedSessionBackend>> {
    let backend = ScriptedSessionBackend::new()?;
    for (id, cwd, endpoint) in [
        (A, a, "claude-local"),
        (B, b, "claude-local"),
        (FOREIGN, a, "cursor-local"),
    ] {
        let session = serde_json::from_value(json!({"sessionId":id,"endpoint":{
            "serviceId":backend.endpoint.service_id,"endpointId":endpoint
        }}))?;
        backend
            .durable_inventory
            .lock()
            .map_err(|_| "fixture inventory lock poisoned")?
            .push(HubSessionSummary {
                session,
                approver: backend.approver.clone(),
                working_directory: cwd.to_owned(),
                updated_at_seconds: 1_700_000_000,
                preview: String::new(),
                name: None,
                model: None,
                mode: None,
                state: SessionState::Unloaded,
            });
    }
    Ok(backend)
}

async fn connect_native(
    backend: &Arc<ScriptedSessionBackend>,
    endpoint: &str,
) -> TestResult<(
    NativeClient,
    JoinHandle<Result<(), AppServerConnectionError>>,
)> {
    let mut selected_endpoint = backend.endpoint.clone();
    selected_endpoint.endpoint_id = endpoint.to_owned().try_into()?;
    let context = Arc::new(RouterSessionAppServerContext::new(
        selected_endpoint,
        ScriptedSessionBackend::actor()?,
        Arc::clone(backend) as Arc<dyn SessionCommandPort>,
        Arc::clone(backend) as Arc<dyn SessionEventHub>,
        super::test_support::model_catalog(tokio::sync::watch::channel(Vec::new()).1),
    ));
    let (client, server) = UnixStream::pair()?;
    let task = tokio::spawn(serve_router_session_app_server_connection(server, context));
    let (client, _) = client_async("ws://localhost/rpc", client).await?;
    Ok((client, task))
}

async fn list_threads(client: &mut NativeClient, params: Value) -> TestResult<Value> {
    client
        .send(Message::Text(
            json!({"id":17,"method":"thread/list","params":params})
                .to_string()
                .into(),
        ))
        .await?;
    let frame = tokio::time::timeout(BOUND, next_non_skill_frame(client))
        .await?
        .ok_or("missing reply")??;
    let response: Value = serde_json::from_str(frame.to_text()?)?;
    assert_eq!(response["id"], 17);
    Ok(response)
}

fn assert_ids(response: &Value, expected: &[&str]) {
    assert!(response.get("error").is_none(), "{response}");
    let ids: Vec<&str> = response["result"]["data"]
        .as_array()
        .expect("thread rows")
        .iter()
        .map(|row| row["id"].as_str().expect("thread id"))
        .collect();
    assert_eq!(ids, expected);
    assert_eq!(response["result"]["nextCursor"], Value::Null);
    assert_eq!(response["result"]["backwardsCursor"], Value::Null);
}

fn assert_no_effects(backend: &ScriptedSessionBackend) {
    assert!(
        backend
            .create_commands
            .lock()
            .expect("test lock")
            .is_empty()
    );
    assert!(backend.load_commands.lock().expect("test lock").is_empty());
    assert!(
        backend
            .prompt_commands
            .lock()
            .expect("test lock")
            .is_empty()
    );
    assert!(backend.steer_commands.lock().expect("test lock").is_empty());
    assert!(
        backend
            .cancel_commands
            .lock()
            .expect("test lock")
            .is_empty()
    );
    assert!(
        backend
            .compact_commands
            .lock()
            .expect("test lock")
            .is_empty()
    );
    assert!(
        backend
            .setting_commands
            .lock()
            .expect("test lock")
            .is_empty()
    );
    assert!(backend.history.lock().expect("test lock").is_empty());
    assert!(backend.live_states.lock().expect("test lock").is_empty());
    assert_eq!(
        backend.durable_inventory.lock().expect("test lock").len(),
        3
    );
}

/// Oracle: pinned360 thread.rs1471-1474/1530, thread_processor264-285/6051-6054.
#[tokio::test]
async fn cwd_filters_are_exact_any_matches_and_keep_endpoint_routing() -> TestResult<()> {
    let directory = tempfile::tempdir()?;
    let a = directory.path().join("a");
    let b = directory.path().join("b");
    std::fs::create_dir(&a)?;
    std::fs::create_dir(&b)?;
    let backend = seeded_backend(&a, &b)?;
    let (mut client, task) = connect_native(&backend, "claude-local").await?;
    for (params, expected) in [
        (json!({"cwd":a}), vec![A]),
        (json!({"cwd":b}), vec![B]),
        (json!({"cwd":[b,a]}), vec![A, B]),
        (json!({}), vec![A, B]),
        (json!({"cwd":null}), vec![A, B]),
        (json!({"cwd":[]}), vec![]),
        (json!({"cwd":directory.path()}), vec![]),
        (json!({"cwd":a.join("child")}), vec![]),
    ] {
        assert_ids(&list_threads(&mut client, params).await?, &expected);
        assert_no_effects(&backend);
    }
    client.close(None).await?;
    tokio::time::timeout(BOUND, task).await???;
    let (mut client, task) = connect_native(&backend, "cursor-local").await?;
    assert_ids(
        &list_threads(&mut client, json!({"cwd":a})).await?,
        &[FOREIGN],
    );
    assert_no_effects(&backend);
    client.close(None).await?;
    tokio::time::timeout(BOUND, task).await???;
    Ok(())
}

/// Relative filters use server cwd and lexical dot removal, never client cwd.
#[tokio::test]
async fn relative_cwd_filters_resolve_from_the_server_without_requiring_exists() -> TestResult<()> {
    let server_cwd = std::env::current_dir()?;
    let other = tempfile::tempdir()?;
    let backend = seeded_backend(&server_cwd, other.path())?;
    let (mut client, task) = connect_native(&backend, "claude-local").await?;
    for cwd in [".", "", "a-missing-directory/.."] {
        assert_ids(&list_threads(&mut client, json!({"cwd":cwd})).await?, &[A]);
    }
    assert_no_effects(&backend);
    client.close(None).await?;
    tokio::time::timeout(BOUND, task).await???;
    Ok(())
}

/// Oracle: pinned path-utils28-43: BOTH canonical paths, otherwise original pair.
#[cfg(unix)]
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Wire assertions; fixture errors propagate."
)]
async fn cwd_comparison_normalizes_symlinks_and_falls_back_for_missing_paths() -> TestResult<()> {
    let directory = tempfile::tempdir()?;
    let a = directory.path().join("a");
    let b = directory.path().join("b");
    let alias = directory.path().join("alias");
    std::fs::create_dir(&a)?;
    std::fs::create_dir(&b)?;
    std::os::unix::fs::symlink(&a, &alias)?;
    let backend = seeded_backend(&alias, &b)?;
    let (mut client, task) = connect_native(&backend, "claude-local").await?;
    let response = list_threads(&mut client, json!({"cwd":a})).await?;
    assert_ids(&response, &[A]);
    assert_eq!(response["result"]["data"][0]["cwd"], json!(alias));
    assert_no_effects(&backend);
    client.close(None).await?;
    tokio::time::timeout(BOUND, task).await???;

    let missing = directory.path().join("missing");
    let unnormalized_missing = directory.path().join("a/../other-missing");
    let backend = seeded_backend(&missing, &unnormalized_missing)?;
    let (mut client, task) = connect_native(&backend, "claude-local").await?;
    assert_ids(
        &list_threads(&mut client, json!({"cwd":missing})).await?,
        &[A],
    );
    assert_ids(
        &list_threads(
            &mut client,
            json!({"cwd":directory.path().join("other-missing")}),
        )
        .await?,
        &[],
    );
    assert_no_effects(&backend);
    client.close(None).await?;
    tokio::time::timeout(BOUND, task).await???;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Wire assertions; fixture errors propagate."
)]
async fn malformed_cwd_filters_return_invalid_params_instead_of_widening_results() -> TestResult<()>
{
    let directory = tempfile::tempdir()?;
    let backend = seeded_backend(directory.path(), directory.path())?;
    let (mut client, task) = connect_native(&backend, "claude-local").await?;
    for cwd in [
        json!(true),
        json!(17),
        json!({}),
        json!([null]),
        json!([directory.path(), 17]),
        json!([[]]),
    ] {
        let response = list_threads(&mut client, json!({"cwd":cwd})).await?;
        assert_eq!(response["error"]["code"], -32602);
        assert!(response.get("result").is_none());
        assert_no_effects(&backend);
    }
    assert_ids(&list_threads(&mut client, json!({})).await?, &[A, B]);
    client.close(None).await?;
    tokio::time::timeout(BOUND, task).await???;
    Ok(())
}

/// Pinned AbsolutePathBuf expands a home key before resolving server-relative paths.
#[tokio::test]
async fn home_relative_cwd_filters_keep_the_native_expansion_rule() -> TestResult<()> {
    let home = std::env::home_dir().ok_or("home directory unavailable")?;
    let directory = tempfile::tempdir()?;
    let backend = seeded_backend(&home, directory.path())?;
    let (mut client, task) = connect_native(&backend, "claude-local").await?;
    for cwd in ["~", "~//", "~/a-missing-directory/.."] {
        assert_ids(&list_threads(&mut client, json!({"cwd":cwd})).await?, &[A]);
    }
    assert_no_effects(&backend);
    client.close(None).await?;
    tokio::time::timeout(BOUND, task).await???;
    Ok(())
}
