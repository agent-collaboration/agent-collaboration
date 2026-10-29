use super::test_support::ScriptedSessionBackend;
use super::tui_output::TuiOutput;
use super::*;
use crate::{NativeControlBackend, NativeGenerationGate, ServiceInteractionBroker};
use session_event_model::{PendingInteraction, QuestionRequest, SessionEvent};
use std::{os::unix::fs::PermissionsExt, process::Stdio, sync::Arc};
use tokio::{
    io::AsyncWriteExt,
    process::Command,
    time::{Duration, timeout},
};
use tokio_util::sync::CancellationToken;

/// Oracle: pinned Codex 0.157.1 tui/bottom_pane/mcp_server_elicitation.rs:617-619.
/// MultiSelect cannot be answered in this TUI, so it remains pending elsewhere
/// and appears as a completed read-only notice here.
#[tokio::test]
#[ignore = "requires installed codex-cli 0.157.1 and a pseudo-terminal"]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn pinned_codex_tui_shows_unsupported_multichoice_question()
-> Result<(), Box<dyn std::error::Error>> {
    let version = Command::new("codex").arg("--version").output().await?;
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout)?.trim(),
        "codex-cli 0.157.1"
    );
    let directory = tempfile::tempdir()?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    let socket_path = directory.path().join("claude.sock");
    let project = directory.path().join("project");
    let codex_home = directory.path().join("codex-home");
    let terminal_log = directory.path().join("multichoice-terminal.log");
    std::fs::create_dir(&project)?;
    std::fs::create_dir(&codex_home)?;
    std::fs::write(
        codex_home.join("config.toml"),
        format!(
            "[projects.\"{}\"]\ntrust_level = \"trusted\"\n",
            project.display()
        ),
    )?;
    let backend = ScriptedSessionBackend::new()?;
    let actor = ScriptedSessionBackend::actor()?;
    let native_endpoint = serde_json::from_value(json!({
        "serviceId":backend.endpoint.service_id,
        "endpointId":"claude-local"
    }))?;
    let broker = ServiceInteractionBroker::load(
        backend.endpoint.service_id.as_str().to_owned().try_into()?,
        NativeControlBackend {
            endpoint: native_endpoint,
            gate: NativeGenerationGate::default(),
            codex_home: codex_home.clone(),
        },
        directory.path().join("questions.json"),
    )
    .await?;
    let context = Arc::new(
        RouterSessionAppServerContext::new(
            backend.endpoint.clone(),
            actor.clone(),
            Arc::clone(&backend) as Arc<dyn SessionCommandPort>,
            Arc::clone(&backend) as Arc<dyn SessionEventHub>,
            tokio::sync::watch::channel(Vec::new()).1,
        )
        .with_interaction_broker(Arc::clone(&broker))
        .with_project_trust(Arc::new(
            codex_native_integration::CodexHomeProjectTrust::new(codex_home.clone()),
        )),
    );
    let listener = RouterSessionAppServerListener::bind(&socket_path, context)?;
    let shutdown = CancellationToken::new();
    let running = tokio::spawn(listener.run(shutdown.clone()));
    let mut tui = Command::new("script")
        .arg("-q")
        .arg(&terminal_log)
        .arg("/bin/sh")
        .arg("-c")
        .arg("stty cols 100 rows 30; exec \"$@\"")
        .arg("sh")
        .arg("codex")
        .arg("hello")
        .arg("--remote")
        .arg(format!("unix://{}", socket_path.display()))
        .arg("--no-alt-screen")
        .arg("--cd")
        .arg(&project)
        .current_dir(&project)
        .env("CODEX_HOME", &codex_home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let mut output = TuiOutput::capture(tui.stdout.take().ok_or("TUI stdout")?);
    timeout(Duration::from_secs(20), backend.created.notified()).await?;
    timeout(Duration::from_secs(10), backend.prompt_submitted.notified()).await?;
    assert!(
        !backend
            .prompt_commands
            .lock()
            .expect("test lock")
            .is_empty(),
        "pinned TUI did not start a provider Turn"
    );
    let question: QuestionRequest = serde_json::from_value(json!({
        "requestId":"multichoice-tui","prompt":"Pick project checks","fields":[
            {"kind":"multiChoice","fieldId":"checks","label":"Checks","description":null,
             "required":true,"min":1,"max":2,"options":[
                {"optionId":"lint","label":"Run lint"},
                {"optionId":"tests","label":"Run tests"}
             ]}
        ]
    }))?;
    let _agent_reply = broker
        .request_question(
            backend.session.clone(),
            actor.clone(),
            question.clone(),
            None,
        )
        .await?;
    assert!(
        backend.events.receiver_count() > 0,
        "TUI event stream detached"
    );
    let sent = backend.events.send(collaboration_contracts::HubEvent {
        sequence: 2,
        event: SessionEvent::InteractionRequested {
            interaction: PendingInteraction::Question {
                approver: actor,
                request: Box::new(question),
            },
        },
    });
    assert!(sent.is_ok(), "multi-choice event had no TUI receiver");
    let rendered = output
        .wait_for_text("unsupportedHere", Duration::from_secs(10))
        .await
        .is_ok();
    if let Some(mut stdin) = tui.stdin.take() {
        let _ = stdin.write_all(b"\x03\x03").await;
    }
    if timeout(Duration::from_secs(3), tui.wait()).await.is_err() {
        let _ = tui.kill().await;
    }
    shutdown.cancel();
    running.await??;
    let terminal = output.finish().await?;
    let tail = terminal
        .chars()
        .rev()
        .take(1_500)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    assert!(
        rendered,
        "pinned TUI did not show unsupportedHere notice while the Turn was active: pending={}, unsupported={}, answer={}, tail={tail:?}",
        terminal.contains("Question pending"),
        terminal.contains("unsupported"),
        terminal.contains("Answer with")
    );
    assert!(
        broker
            .list_questions(true)
            .await
            .iter()
            .any(|record| record.request_id() == "multichoice-tui")
    );
    Ok(())
}
