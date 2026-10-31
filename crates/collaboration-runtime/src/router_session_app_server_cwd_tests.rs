use super::test_support::ScriptedSessionBackend;
use super::*;
use serde_json::json;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn thread_start_uses_the_client_existing_working_directory()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let backend = ScriptedSessionBackend::new()?;
    let started = handle_app_server_thread_request(
        "thread/start",
        json!({"cwd":directory.path(),"model":"provider-default"}),
        Arc::clone(&backend) as Arc<dyn SessionCommandPort>,
        Arc::clone(&backend) as Arc<dyn SessionEventHub>,
        backend.endpoint.clone(),
        ScriptedSessionBackend::actor()?,
        &[],
    )
    .await?;
    assert_eq!(started["thread"]["cwd"], json!(directory.path()));
    assert_eq!(
        backend.create_commands.lock().expect("test lock")[0].working_directory,
        directory.path()
    );
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn thread_start_rejects_a_missing_client_directory_before_create()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let missing = directory.path().join("does-not-exist");
    let backend = ScriptedSessionBackend::new()?;
    let result = handle_app_server_thread_request(
        "thread/start",
        json!({"cwd":missing}),
        Arc::clone(&backend) as Arc<dyn SessionCommandPort>,
        Arc::clone(&backend) as Arc<dyn SessionEventHub>,
        backend.endpoint.clone(),
        ScriptedSessionBackend::actor()?,
        &[],
    )
    .await;
    assert!(matches!(result, Err(ThreadMethodError::InvalidParams)));
    assert!(
        backend
            .create_commands
            .lock()
            .expect("test lock")
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn thread_start_requires_an_explicit_client_directory()
-> Result<(), Box<dyn std::error::Error>> {
    let backend = ScriptedSessionBackend::new()?;
    let result = handle_app_server_thread_request(
        "thread/start",
        json!({"model":"provider-default"}),
        Arc::clone(&backend) as Arc<dyn SessionCommandPort>,
        Arc::clone(&backend) as Arc<dyn SessionEventHub>,
        backend.endpoint.clone(),
        ScriptedSessionBackend::actor()?,
        &[],
    )
    .await;
    assert!(matches!(
        result,
        Err(ThreadMethodError::WorkingDirectoryRequired)
    ));
    assert!(
        backend
            .create_commands
            .lock()
            .expect("test lock")
            .is_empty()
    );
    Ok(())
}

/// Oracle: pinned Codex tui/src/config_update.rs:225-235, 346-375 and
/// tui/src/onboarding/onboarding_screen.rs:687-725.
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn remote_trust_read_is_narrow_and_trust_write_is_refused() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = tempfile::tempdir()?;
    let home = directory.path().join("codex-home");
    std::fs::create_dir(&home)?;
    std::fs::write(
        home.join("config.toml"),
        format!(
            "[projects.\"{}\"]\ntrust_level = \"trusted\"\n",
            directory.path().display()
        ),
    )?;
    let lookup = codex_native_integration::CodexHomeProjectTrust::new(home);
    let trusted = handle_app_server_request(
        json!(1),
        "config/read",
        &json!({"includeLayers":true,"cwd":directory.path()}),
        &[],
        Some(&lookup),
    );
    assert_eq!(
        trusted["result"]["config"]["projects"][directory.path().to_str().ok_or("utf8")?]["trust_level"],
        "trusted"
    );
    assert_eq!(trusted["result"]["layers"], json!([]));
    assert_eq!(
        handle_app_server_request(json!(2), "config/read", &json!({}), &[], Some(&lookup))["error"]
            ["code"],
        -32601
    );
    assert_eq!(
        handle_app_server_request(
            json!(3),
            "config/batchWrite",
            &json!({}),
            &[],
            Some(&lookup)
        )["error"]["code"],
        -32000
    );
    assert_eq!(
        handle_app_server_request(json!(4), "config/write", &json!({}), &[], Some(&lookup))["error"]
            ["code"],
        -32000
    );
    Ok(())
}

/// Oracle: pinned Codex 0.165 tui/src/config_update.rs:87-104,155-171
/// and the read-only ACP configuration boundary.
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn default_model_batch_write_is_refused_without_project_trust_advice()
-> Result<(), Box<dyn std::error::Error>> {
    // Arrange: the native model picker persists model and effort together.
    let directory = tempfile::tempdir()?;
    let config_path = directory.path().join("config.toml");
    let original = "model = \"original-model\"\n";
    std::fs::write(&config_path, original)?;
    let lookup = codex_native_integration::CodexHomeProjectTrust::new(directory.path().into());
    for effort in [json!("medium"), Value::Null] {
        // Act through the same handler that receives the native batch request.
        let response = handle_app_server_request(
            json!("tui-config-write-model"),
            "config/batchWrite",
            &json!({
                "edits": [
                    {"keyPath":"model","value":"haiku","mergeStrategy":"replace"},
                    {"keyPath":"model_reasoning_effort","value":effort,"mergeStrategy":"replace"}
                ],
                "filePath":config_path,
                "expectedVersion":null,
                "reloadUserConfig":true
            }),
            &[],
            Some(&lookup),
        );

        // Assert: refuse persistence honestly without suggesting a trust change.
        assert_eq!(response["id"], "tui-config-write-model");
        assert_eq!(response["error"]["code"], -32000);
        assert_eq!(
            response["error"]["message"],
            "Saving Codex configuration/defaults is not supported on this ACP endpoint"
        );
        assert!(response.get("result").is_none());
        assert_eq!(std::fs::read_to_string(&config_path)?, original);
    }
    Ok(())
}

/// Oracle: pinned Codex 0.165 tui/src/config_update.rs:77-104. Only
/// project-trust edits warrant instructions to change project trust in Codex.
#[test]
fn configuration_write_refusals_only_give_trust_advice_for_project_trust_edits() {
    for (key_path, trust_edit) in [
        (r#"projects."/workspace/a.b".trust_level"#, true),
        (
            r#"projects."/workspace/a\"quoted\"\\directory".trust_level"#,
            true,
        ),
        ("model", false),
        ("model_reasoning_effort", false),
        ("service_tier", false),
        (r#"projects."".trust_level"#, false),
        (r#"projects."/workspace".trust_level.extra"#, false),
        (r#"projects."/workspace.trust_level".model"#, false),
        (r#"projects."/workspace"."other".trust_level"#, false),
    ] {
        let edit = json!({"keyPath":key_path,"value":"trusted","mergeStrategy":"replace"});
        for (method, params) in [
            ("config/batchWrite", json!({"edits":[edit.clone()]})),
            ("config/write", edit),
        ] {
            let response = handle_app_server_request(json!(1), method, &params, &[], None);
            assert_eq!(response["error"]["code"], -32000);
            assert!(response.get("result").is_none());
            let expected = if trust_edit {
                "Provider session trust is read-only here; trust this project in Codex itself"
            } else {
                "Saving Codex configuration/defaults is not supported on this ACP endpoint"
            };
            assert_eq!(
                response["error"]["message"], expected,
                "{method}: {key_path}"
            );
        }
    }

    // A mixed, empty or malformed batch must not describe every refusal as trust.
    for params in [
        json!({"edits":[
            {"keyPath":"projects.\"/workspace\".trust_level","value":"trusted","mergeStrategy":"replace"},
            {"keyPath":"model","value":"haiku","mergeStrategy":"replace"}
        ]}),
        json!({"edits":[]}),
        json!({"edits":[{"value":"projects.\"/workspace\".trust_level"}]}),
        json!({"edits":"projects.\"/workspace\".trust_level"}),
    ] {
        let response = handle_app_server_request(json!(2), "config/batchWrite", &params, &[], None);
        assert_eq!(response["error"]["code"], -32000);
        assert_eq!(
            response["error"]["message"],
            "Saving Codex configuration/defaults is not supported on this ACP endpoint"
        );
        assert!(response.get("result").is_none());
    }
    // This repair adds no new write method or changes its existing RPC code.
    assert_eq!(
        handle_app_server_request(
            json!(3),
            "config/value/write",
            &json!({"keyPath":"model","value":"haiku","mergeStrategy":"replace"}),
            &[],
            None
        )["error"]["code"],
        -32601
    );
}

/// Oracle: Codex tui/src/config_update.rs:235-261,294-375. A remote TUI
/// needs a project layer whenever the trusted key differs from its literal cwd.
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn canonical_trust_key_produces_a_project_layer() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let home = directory.path().join("codex-home");
    let project = directory.path().join("project");
    let alias = directory.path().join("alias");
    std::fs::create_dir(&home)?;
    std::fs::create_dir(&project)?;
    std::os::unix::fs::symlink(&project, &alias)?;
    let canonical = project.canonicalize()?;
    std::fs::write(
        home.join("config.toml"),
        format!(
            "[projects.\"{}\"]\ntrust_level = \"trusted\"\n",
            canonical.display()
        ),
    )?;
    let lookup = codex_native_integration::CodexHomeProjectTrust::new(home);
    let result = handle_app_server_request(
        json!(1),
        "config/read",
        &json!({"includeLayers":true,"cwd":alias}),
        &[],
        Some(&lookup),
    );
    assert_eq!(
        result["result"]["layers"][0]["name"]["dotCodexFolder"],
        format!("{}/.codex", canonical.display())
    );
    Ok(())
}

/// Oracle: pinned Codex tui/src/config_update.rs:235-261, 294-375.
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn repository_root_trust_is_visible_to_remote_tui() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let home = directory.path().join("codex-home");
    let repo = directory.path().join("repo");
    let cwd = repo.join("child");
    std::fs::create_dir_all(&cwd)?;
    std::fs::create_dir(repo.join(".git"))?;
    std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/main\n")?;
    std::fs::create_dir(&home)?;
    let config_path = home.join("config.toml");
    let lookup = codex_native_integration::CodexHomeProjectTrust::new(home);
    for (level, expected_reason) in [("trusted", false), ("untrusted", true)] {
        std::fs::write(
            &config_path,
            format!(
                "[projects.\"{}\"]\ntrust_level = \"{level}\"\n",
                repo.display()
            ),
        )?;
        let result = handle_app_server_request(
            json!(1),
            "config/read",
            &json!({"includeLayers":true,"cwd":cwd}),
            &[],
            Some(&lookup),
        );
        assert_eq!(
            result["result"]["config"]["projects"][repo.to_str().ok_or("utf8")?]["trust_level"],
            level
        );
        assert_eq!(result["result"]["layers"][0]["name"]["type"], "project");
        assert_eq!(
            result["result"]["layers"][0]
                .get("disabledReason")
                .is_some(),
            expected_reason
        );
    }
    Ok(())
}

/// Oracle: pinned Codex tui/src/config_update.rs:294-375. Without a saved
/// decision the remote TUI must still receive the repository trust target.
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn untrusted_repository_root_remains_the_remote_trust_target()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let home = directory.path().join("codex-home");
    let repo = directory.path().join("repo");
    let cwd = repo.join("child");
    std::fs::create_dir_all(&cwd)?;
    std::fs::create_dir(repo.join(".git"))?;
    std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/main\n")?;
    std::fs::create_dir(&home)?;
    let lookup = codex_native_integration::CodexHomeProjectTrust::new(home);
    let response = handle_app_server_request(
        json!(1),
        "config/read",
        &json!({"includeLayers":true,"cwd":cwd}),
        &[],
        Some(&lookup),
    );
    let reason = response["result"]["layers"][0]["disabledReason"]
        .as_str()
        .ok_or("missing root trust target")?;
    assert!(reason.contains(&format!(
        ", add {} as a trusted project in ",
        repo.display()
    )));
    Ok(())
}

/// Oracle: pinned Codex tui/src/app_server_session.rs:2172-2181 omits cwd
/// when --cd was absent. The face returns a typed instruction to relaunch.
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn omitted_tui_cwd_gets_a_typed_launch_instruction() -> Result<(), Box<dyn std::error::Error>>
{
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::{client_async, tungstenite::Message};
    let directory = tempfile::tempdir()?;
    let socket_path = directory.path().join("provider.sock");
    let listener = tokio::net::UnixListener::bind(&socket_path)?;
    let backend = ScriptedSessionBackend::new()?;
    let context = Arc::new(RouterSessionAppServerContext::new(
        backend.endpoint.clone(),
        ScriptedSessionBackend::actor()?,
        Arc::clone(&backend) as Arc<dyn SessionCommandPort>,
        Arc::clone(&backend) as Arc<dyn SessionEventHub>,
        super::test_support::model_catalog(tokio::sync::watch::channel(Vec::new()).1),
    ));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        serve_router_session_app_server_connection(stream, context)
            .await
            .expect("serve");
    });
    let stream = UnixStream::connect(&socket_path).await?;
    let (mut client, _) = client_async("ws://localhost/rpc", stream).await?;
    client
        .send(Message::Text(
            json!({"id":1,"method":"thread/start","params":{}})
                .to_string()
                .into(),
        ))
        .await?;
    let response: Value = serde_json::from_str(client.next().await.ok_or("response")??.to_text()?)?;
    assert_eq!(response["error"]["code"], -32602);
    assert!(
        response["error"]["message"]
            .as_str()
            .ok_or("message")?
            .contains("--cd <project>")
    );
    assert!(
        backend
            .create_commands
            .lock()
            .expect("test lock")
            .is_empty()
    );
    client.close(None).await?;
    server.await?;
    Ok(())
}
