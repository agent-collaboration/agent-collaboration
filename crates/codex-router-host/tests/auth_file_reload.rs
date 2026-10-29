//! Filesystem events must reach the managed native cache without replacing its process.
use std::{path::Path, time::Duration};

use codex_native_integration::NativeProtocolConnection;
use codex_router_host::{
    AppServerLaunchPlan, ChildCommandSpec, ChildOutput, HostConfig, HostConfigInputs,
    HostCoordinationPaths, HostDeadlines, HostRuntime, ManagedChildLaunchPlans,
    ManagedUpdateInputs, OperatorFrame, OperatorRequest,
};
use codex_router_test_support::router_health::PersistentRouterHealthFixture;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};

#[path = "auth_file_reload/debounce_clock.rs"]
mod debounce_clock;
#[path = "support/operator_client.rs"]
mod operator_client;

#[tokio::test]
async fn auth_file_atomic_rotation_reloads_once_and_refuses_account_switch()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;
    let (subscriber, mut debounce_clock) = debounce_clock::observe_armed_deadlines();
    let _subscriber = tracing::subscriber::set_default(subscriber);
    let directory = tempfile::Builder::new()
        .prefix("auth-reload-")
        .tempdir_in("/tmp")?;
    let home = directory.path().join("codex");
    std::fs::create_dir(&home)?;
    write_auth(&home, "account-x", "x1")?;
    let socket = directory.path().join("native.sock");
    let executable = directory.path().join("managed-codex");
    let process_log = directory.path().join("process.log");
    std::fs::write(
        &executable,
        "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'codex-cli 1.2.3'; else exec \"$AUTH_RELOAD_FIXTURE_BINARY\" --exact auth_reload_fixture_child --nocapture; fi\n",
    )?;
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))?;
    let identity = codex_native_integration::executable_identity(&executable).await?;
    let command = ChildCommandSpec::new(executable.clone())
        .with_environment("AUTH_RELOAD_FIXTURE_BINARY", std::env::current_exe()?)
        .with_environment("AUTH_RELOAD_FIXTURE_HOME", &home)
        .with_environment("AUTH_RELOAD_FIXTURE_SOCKET", &socket)
        .with_environment("AUTH_RELOAD_FIXTURE_LOG", &process_log)
        .with_output(ChildOutput::Null);
    let router = PersistentRouterHealthFixture::start().await?;
    let coordination = HostCoordinationPaths::for_router_root(directory.path());
    let config = HostConfig::new(HostConfigInputs {
        coordination_paths: coordination.clone(),
        router_endpoint: router.address(),
        mcp_bind: ([127, 0, 0, 1], 0).into(),
        app_server_socket: socket.clone(),
        managed_executable: executable,
        deadlines: HostDeadlines::production(),
    })
    .with_collaboration_directory(directory.path().join("collaboration"), home.clone());
    let runtime = tokio::spawn(HostRuntime::run(
        config,
        ManagedChildLaunchPlans::new(
            None,
            AppServerLaunchPlan::new(command, identity, "1.2.3".into()),
        ),
        ManagedUpdateInputs::production(),
    ));
    let result = async {
        operator_client::send_operator_request(
            coordination.operator_socket(),
            OperatorRequest::AwaitHostStart,
            Duration::from_secs(20),
        )
        .await?;
        let initial_process = std::fs::read_to_string(&process_log)?;
        // Both writes land before the watcher can observe the burst. The final write
        // atomically replaces auth.json, rather than mutating its original inode.
        debounce_clock.before_write();
        write_auth(&home, "account-x", "intermediate")?;
        write_auth(&home, "account-x", "x2")?;
        debounce_clock
            .complete(wait_for_reload(coordination.operator_socket(), "reloaded"))
            .await?;
        let mut client = NativeProtocolConnection::connect(&socket).await?;
        let account = client
            .request("account/read", json!({"refreshToken":false}))
            .await?;
        check_equal(
            &account["account"]["email"],
            &json!("x2@example.invalid"),
            "rotated account was not adopted",
        )?;
        check_equal(
            &account["refreshCount"],
            &json!(1),
            "the burst must cause one refresh",
        )?;
        // Alternate observable outcomes so every file write waits for the Host's
        // completed verification, rather than an upstream cache change mid-RPC.
        debounce_clock.before_write();
        write_auth(&home, "account-y", "y1")?;
        debounce_clock
            .complete(wait_for_reload(
                coordination.operator_socket(),
                "refused_different_account",
            ))
            .await?;
        let account = client
            .request("account/read", json!({"refreshToken":false}))
            .await?;
        check_equal(
            &account["account"]["email"],
            &json!("x2@example.invalid"),
            "different account must not replace the cache",
        )?;
        check_equal(
            &account["refreshCount"],
            &json!(2),
            &format!(
                "different-account revision must be attempted once; observed {}",
                account["refreshCount"]
            ),
        )?;

        // The directory subscription must survive replacement of auth.json again.
        debounce_clock.before_write();
        write_auth(&home, "account-x", "x3")?;
        debounce_clock
            .complete(wait_for_reload(coordination.operator_socket(), "reloaded"))
            .await?;
        let account = client
            .request("account/read", json!({"refreshToken":false}))
            .await?;
        check_equal(
            &account["account"]["email"],
            &json!("x3@example.invalid"),
            "later rotation must be adopted",
        )?;
        check_equal(
            &account["refreshCount"],
            &json!(3),
            "watch must survive atomic replacement",
        )?;
        let mut matching_account_document: Value =
            serde_json::from_slice(&std::fs::read(home.join("auth.json"))?)?;

        // Account switches may reuse an access token; account identity still wins.
        debounce_clock.before_write();
        write_auth(&home, "account-y", "x3")?;
        debounce_clock
            .complete(wait_for_reload(
                coordination.operator_socket(),
                "refused_different_account",
            ))
            .await?;
        let account = client
            .request("account/read", json!({"refreshToken":false}))
            .await?;
        check_equal(
            &account["refreshCount"],
            &json!(4),
            "shared-token account switch must be attempted once",
        )?;
        check_equal(
            &account["account"]["email"],
            &json!("x3@example.invalid"),
            "shared-token account switch must keep the old cache",
        )?;

        // Restore the same account by changing only its refresh token. This
        // in-place write also proves the current inode remains watched.
        matching_account_document["tokens"]["refresh_token"] = json!("refresh-only-revision");
        debounce_clock.before_write();
        std::fs::write(
            home.join("auth.json"),
            serde_json::to_vec(&matching_account_document)?,
        )?;
        debounce_clock
            .complete(wait_for_reload(coordination.operator_socket(), "reloaded"))
            .await?;
        let account = client
            .request("account/read", json!({"refreshToken":false}))
            .await?;
        check_equal(
            &account["refreshCount"],
            &json!(5),
            "refresh-token-only revision must request one guarded reload",
        )?;
        check_equal(
            &std::fs::read_to_string(&process_log)?,
            &initial_process,
            "auth changes must never restart the child",
        )?;
        Ok::<_, Box<dyn std::error::Error>>(())
    }
    .await;
    runtime.abort();
    let _settled = runtime.await;
    if let Ok(raw_pid) = std::fs::read_to_string(&process_log) {
        for raw_pid in raw_pid.lines() {
            let signed_pid: i32 = raw_pid.parse()?;
            let pid = rustix::process::Pid::from_raw(signed_pid).ok_or("invalid fixture PID")?;
            let _terminated = rustix::process::kill_process(pid, rustix::process::Signal::TERM);
            tokio::time::timeout(Duration::from_secs(5), async {
                while rustix::process::test_kill_process(pid).is_ok() {
                    tokio::task::yield_now().await;
                }
            })
            .await?;
        }
    }
    router.finish().await?;
    result
}

async fn wait_for_reload(socket: &Path, expected: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut last_snapshot = Value::Null;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let frames = operator_client::send_operator_request(
                socket,
                OperatorRequest::Status,
                Duration::from_secs(3),
            )
            .await?;
            for frame in frames {
                if let OperatorFrame::Terminal(response) = frame {
                    let snapshot = serde_json::to_value(response.snapshot())?;
                    if snapshot
                        .pointer("/last_login_reload/kind")
                        .and_then(Value::as_str)
                        == Some(expected)
                    {
                        return Ok::<_, Box<dyn std::error::Error>>(());
                    }
                    last_snapshot = snapshot;
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| {
        format!("Host did not report login reload: {expected}; last snapshot: {last_snapshot}")
    })?
}

fn write_auth(home: &Path, account: &str, revision: &str) -> std::io::Result<()> {
    std::fs::write(
        home.join("auth.next"),
        serde_json::to_vec(&json!({
            "auth_mode":"chatgpt", "tokens": {
                "account_id":account, "access_token":revision,
                "id_token":format!("id-{revision}"), "refresh_token":format!("refresh-{revision}")
            }
        }))?,
    )?;
    std::fs::rename(home.join("auth.next"), home.join("auth.json"))
}

#[tokio::test]
async fn auth_reload_fixture_child() -> Result<(), Box<dyn std::error::Error>> {
    let Some(home) = std::env::var_os("AUTH_RELOAD_FIXTURE_HOME") else {
        return Ok(());
    };
    let home = std::path::PathBuf::from(home);
    let socket = std::env::var_os("AUTH_RELOAD_FIXTURE_SOCKET").ok_or("missing socket")?;
    let process_log = std::env::var_os("AUTH_RELOAD_FIXTURE_LOG").ok_or("missing log")?;
    use std::io::Write;
    writeln!(
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(process_log)?,
        "{}",
        std::process::id()
    )?;
    let cache = std::sync::Arc::new(tokio::sync::Mutex::new((read_tokens(&home)?, 0_u64)));
    let listener = tokio::net::UnixListener::bind(&socket)?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut connections =
        tokio::task::JoinSet::<Result<(), Box<dyn std::error::Error + Send + Sync>>>::new();
    loop {
        tokio::select! {
            _ = terminate.recv() => break,
            Some(result) = connections.join_next() => { result?.map_err(std::io::Error::other)?; },
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let cache = cache.clone();
                let home = home.clone();
                connections.spawn(async move {
                    let mut socket = tokio_tungstenite::accept_async(stream).await?;
                    while let Some(message) = socket.next().await {
                        use tokio_tungstenite::tungstenite::{Message, Error, error::ProtocolError};
                        let message_text = match message {
                            Ok(Message::Text(text)) => text,
                            Ok(Message::Close(_)) | Err(Error::ConnectionClosed | Error::AlreadyClosed | Error::Protocol(ProtocolError::ResetWithoutClosingHandshake)) => break,
                            Ok(_) => continue,
                            Err(error) => return Err(error.into()),
                        };
                        let request: Value = serde_json::from_str(&message_text)?;
                        let Some(id) = request.get("id") else { continue; };
                        let mut cache = cache.lock().await;
                        let result = match request["method"].as_str() {
                            Some("initialize") => json!({"userAgent":"codex-cli/1.2.3"}),
                            Some("remoteControl/status/read") => json!({"status":"connected","serverName":"auth-fixture","environmentId":"fixture"}),
                            Some("getAuthStatus") => json!({"authMethod":"chatgpt","authToken":cache.0["access_token"]}),
                            Some("account/read") => {
                                if request["params"]["refreshToken"] == true {
                                    cache.1 += 1;
                                    let tokens = read_tokens(&home)?;
                                    if tokens["account_id"] == cache.0["account_id"] { cache.0 = tokens; }
                                }
                                json!({"account":{"type":"chatgpt","email":format!("{}@example.invalid",cache.0["access_token"].as_str().ok_or("token missing")?),"planType":"pro"},"workspaceRouting":{"chatgptAccountId":cache.0["account_id"]},"refreshCount":cache.1})
                            },
                            _ => json!({}),
                        };
                        socket.send(tokio_tungstenite::tungstenite::Message::Text(json!({"id":id,"result":result}).to_string().into())).await?;
                    }
                    Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
                });
            }
        }
    }
    connections.abort_all();
    while connections.join_next().await.is_some() {}
    std::fs::remove_file(socket)?;
    Ok(())
}

fn read_tokens(home: &Path) -> std::io::Result<Value> {
    let document: Value = serde_json::from_slice(&std::fs::read(home.join("auth.json"))?)?;
    document
        .get("tokens")
        .cloned()
        .ok_or_else(|| std::io::Error::other("fixture auth tokens missing"))
}

fn check_equal<T: PartialEq>(actual: &T, expected: &T, reason: &str) -> std::io::Result<()> {
    if actual == expected {
        Ok(())
    } else {
        Err(std::io::Error::other(reason))
    }
}
