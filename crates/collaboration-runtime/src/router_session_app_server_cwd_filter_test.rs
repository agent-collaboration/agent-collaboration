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

fn listed_ids(response: &Value) -> Vec<&str> {
    response["result"]["data"]
        .as_array()
        .expect("rows")
        .iter()
        .map(|row| row["id"].as_str().expect("thread id"))
        .collect()
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; setup and transport may fail."
)]
async fn list_limits_match_the_native_default_floor_and_ceiling() -> TestResult<()> {
    // Arrange: enough rows to distinguish the native default and maximum.
    let directory = tempfile::tempdir()?;
    let backend = seeded_backend(directory.path(), directory.path())?;
    {
        let mut inventory = backend.durable_inventory.lock().expect("inventory");
        let template = inventory[0].clone();
        inventory.clear();
        for index in 0..120 {
            let mut summary = template.clone();
            summary.session.session_id = Uuid::now_v7().to_string().try_into()?;
            summary.updated_at_seconds = index;
            inventory.push(summary);
        }
    }
    let (mut client, task) = connect_native(&backend, "claude-local").await?;
    // Act / Assert: pinned upstream thread_processor.rs:2782-2786 and :46-47.
    for (params, expected) in [
        (json!({}), 25),
        (json!({"limit":0}), 1),
        (json!({"limit":10000}), 100),
    ] {
        let response = list_threads(&mut client, params).await?;
        assert_eq!(listed_ids(&response).len(), expected);
        assert!(response["result"]["nextCursor"].is_string());
    }
    client.close(None).await?;
    tokio::time::timeout(BOUND, task).await???;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; setup and transport may fail."
)]
async fn updated_order_pages_use_timestamp_and_session_id_and_reverse_at_the_page_timestamp()
-> TestResult<()> {
    // Arrange: inventory insertion order differs from update order; two rows tie.
    let directory = tempfile::tempdir()?;
    let backend = seeded_backend(directory.path(), directory.path())?;
    const C: &str = "01a1c066-9000-7000-8000-000000000001";
    {
        let mut inventory = backend.durable_inventory.lock().expect("inventory");
        for row in inventory.iter_mut() {
            row.updated_at_seconds = 100;
        }
        let mut newest = inventory[0].clone();
        newest.session.session_id = C.to_owned().try_into()?;
        newest.updated_at_seconds = 200;
        inventory.push(newest);
    }
    let (mut client, task) = connect_native(&backend, "claude-local").await?;
    // Act / Assert: each cursor advances after its exact timestamp/id anchor.
    let first = list_threads(
        &mut client,
        json!({"sortKey":"updated_at","limit":1,"cwd":directory.path()}),
    )
    .await?;
    assert_eq!(listed_ids(&first), [C]);
    let second = list_threads(&mut client, json!({"sortKey":"updated_at","limit":1,"cursor":first["result"]["nextCursor"],"cwd":directory.path()})).await?;
    assert_eq!(listed_ids(&second), [A]);
    let third = list_threads(&mut client, json!({"sortKey":"updated_at","limit":1,"cursor":second["result"]["nextCursor"],"cwd":directory.path()})).await?;
    assert_eq!(listed_ids(&third), [B]);
    assert_eq!(third["result"]["nextCursor"], Value::Null);
    let backwards = list_threads(&mut client, json!({"sortKey":"updated_at","sortDirection":"asc","limit":3,"cursor":second["result"]["backwardsCursor"]})).await?;
    assert_eq!(
        listed_ids(&backwards),
        [A, B, C],
        "reversing includes every row at the page's first timestamp"
    );
    // A deleted anchor does not turn the cursor into a missing-row lookup.
    backend
        .durable_inventory
        .lock()
        .expect("inventory")
        .retain(|row| row.session.session_id.as_str() != C);
    let after_deleted = list_threads(
        &mut client,
        json!({"sortKey":"updated_at","limit":2,"cursor":first["result"]["nextCursor"]}),
    )
    .await?;
    assert_eq!(listed_ids(&after_deleted), [A, B]);
    client.close(None).await?;
    tokio::time::timeout(BOUND, task).await???;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; setup and transport may fail."
)]
async fn created_order_reports_unavailable_instead_of_inventing_creation_time() -> TestResult<()> {
    let directory = tempfile::tempdir()?;
    let backend = seeded_backend(directory.path(), directory.path())?;
    let (mut client, task) = connect_native(&backend, "claude-local").await?;
    let response = list_threads(&mut client, json!({"sortKey":"created_at"})).await?;
    assert_eq!(response["error"]["code"], -32000);
    let message = response["error"]["message"]
        .as_str()
        .expect("error message");
    assert!(message.contains("not available through the Router"));
    assert!(message.contains("creation time"));
    assert_no_effects(&backend);
    client.close(None).await?;
    tokio::time::timeout(BOUND, task).await???;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; setup and transport may fail."
)]
async fn malformed_list_paging_is_rejected_without_widening_inventory() -> TestResult<()> {
    let directory = tempfile::tempdir()?;
    let backend = seeded_backend(directory.path(), directory.path())?;
    let (mut client, task) = connect_native(&backend, "claude-local").await?;
    for params in [
        json!({"cursor":"not-a-cursor"}),
        json!({"limit":-1}),
        json!({"limit":"1"}),
        json!({"sortDirection":"sideways"}),
        json!({"sortKey":"made-up"}),
        json!({"archived":"true"}),
    ] {
        let response = list_threads(&mut client, params.clone()).await?;
        assert_eq!(response["error"]["code"], -32602, "{params}: {response}");
        assert!(response.get("result").is_none());
    }
    assert_no_effects(&backend);
    client.close(None).await?;
    tokio::time::timeout(BOUND, task).await???;
    Ok(())
}

#[tokio::test]
async fn archived_thread_list_is_empty_without_loading_provider_sessions() -> TestResult<()> {
    // Arrange: normal inventory contains matching sessions, but none is archived.
    let directory = tempfile::tempdir()?;
    let backend = seeded_backend(directory.path(), directory.path())?;
    let (mut client, task) = connect_native(&backend, "claude-local").await?;
    // Act: the archived tab asks the same endpoint for archived sessions only.
    let response =
        list_threads(&mut client, json!({"archived":true,"cwd":directory.path()})).await?;
    // Assert: do not show the ordinary sessions in that tab or load a provider.
    assert_ids(&response, &[]);
    assert_no_effects(&backend);
    client.close(None).await?;
    tokio::time::timeout(BOUND, task).await???;
    Ok(())
}

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
    if expected.is_empty() {
        assert_eq!(response["result"]["backwardsCursor"], Value::Null);
    } else {
        assert!(
            response["result"]["backwardsCursor"]
                .as_str()
                .is_some_and(|cursor| !cursor.is_empty())
        );
    }
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
